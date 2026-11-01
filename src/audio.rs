//! Audio output for the presentation.
//!
//! Video goes through Rune's `NativeSurface`; sound has no Rune element on Windows (Rune's own
//! `Player` is macOS-only), so vidshow runs a second ffmpeg for the audio track and feeds the
//! samples to the default output device through cpal. Any failure here is reported as a note in
//! the control window: losing sound never stops the picture.

#[cfg(windows)]
mod imp {
    use crate::media::{self, MediaInfo, Tools};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::collections::VecDeque;
    use std::io::Read;
    use std::process::Child;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// Slack in the ring: enough to ride out a slow frame, short enough that a pause is instant.
    const BUFFER_MS: u32 = 250;

    struct Ring {
        samples: VecDeque<f32>,
        cap: usize,
    }

    struct Ctl {
        paused: bool,
        seek: Option<f64>,
        stop: bool,
    }

    pub struct Audio {
        ctl: Arc<Mutex<Ctl>>,
        gain: Arc<AtomicU32>,
        process: Arc<Mutex<Option<Child>>>,
        reader: Option<std::thread::JoinHandle<()>>,
        host: Option<std::thread::JoinHandle<()>>,
        host_stop: Arc<AtomicBool>,
    }

    impl Audio {
        /// Opens the default output device and starts decoding at `start` seconds, paused.
        pub fn start(tools: &Tools, info: &MediaInfo, start: f64) -> Result<Self, String> {
            if !info.has_audio {
                return Err("this file has no audio track".into());
            }
            let ctl = Arc::new(Mutex::new(Ctl { paused: true, seek: None, stop: false }));
            let gain = Arc::new(AtomicU32::new(1f32.to_bits()));
            let ring = Arc::new(Mutex::new(Ring { samples: VecDeque::new(), cap: 1 }));
            let host_stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = mpsc::channel();
            let host = {
                let (ring, ctl, gain, host_stop) = (ring.clone(), ctl.clone(), gain.clone(), host_stop.clone());
                std::thread::spawn(move || host_loop(ready_tx, ring, ctl, gain, host_stop))
            };
            let (rate, channels) = match ready_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(format)) => format,
                Ok(Err(error)) => {
                    host_stop.store(true, SeqCst);
                    return Err(error);
                }
                Err(_) => {
                    host_stop.store(true, SeqCst);
                    return Err("the audio device did not start".into());
                }
            };
            if let Ok(mut r) = ring.lock() {
                r.cap = (rate as usize * channels as usize * BUFFER_MS as usize / 1000).max(1024);
            }
            let process = Arc::new(Mutex::new(None));
            let reader = {
                let (process, ring, ctl) = (process.clone(), ring.clone(), ctl.clone());
                let (tools, info) = (tools.clone(), info.clone());
                std::thread::spawn(move || reader_loop(tools, info, start, rate, channels, ctl, ring, process))
            };
            Ok(Self { ctl, gain, process, reader: Some(reader), host: Some(host), host_stop })
        }

        pub fn play(&self) {
            if let Ok(mut ctl) = self.ctl.lock() {
                ctl.paused = false;
            }
        }

        pub fn pause(&self) {
            if let Ok(mut ctl) = self.ctl.lock() {
                ctl.paused = true;
            }
        }

        pub fn seek(&self, seconds: f64) {
            if let Ok(mut ctl) = self.ctl.lock() {
                ctl.seek = Some(seconds.max(0.0));
            }
            if let Ok(mut process) = self.process.lock() {
                if let Some(child) = process.as_mut() {
                    let _ = child.kill();
                }
            }
        }

        pub fn set_gain(&self, gain: f32) {
            self.gain.store(gain.clamp(0.0, 1.0).to_bits(), SeqCst);
        }

        pub fn stop(&mut self) {
            if let Ok(mut ctl) = self.ctl.lock() {
                ctl.stop = true;
            }
            if let Ok(mut process) = self.process.lock() {
                if let Some(child) = process.as_mut() {
                    let _ = child.kill();
                }
            }
            self.host_stop.store(true, SeqCst);
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
            if let Some(host) = self.host.take() {
                let _ = host.join();
            }
        }
    }

    impl Drop for Audio {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Owns the cpal stream, which is not `Send`: it lives and dies on this thread.
    fn host_loop(
        ready: mpsc::Sender<Result<(u32, u16), String>>,
        ring: Arc<Mutex<Ring>>,
        ctl: Arc<Mutex<Ctl>>,
        gain: Arc<AtomicU32>,
        stop: Arc<AtomicBool>,
    ) {
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            let _ = ready.send(Err("no audio output device".into()));
            return;
        };
        let supported = match device.default_output_config() {
            Ok(config) => config,
            Err(error) => {
                let _ = ready.send(Err(format!("the audio device has no output format: {error}")));
                return;
            }
        };
        let config: cpal::StreamConfig = supported.config();
        let (rate, channels) = (config.sample_rate.0, config.channels);
        let paused = {
            let ctl = ctl.clone();
            move || ctl.lock().map(|c| c.paused).unwrap_or(true)
        };
        let stream = device.build_output_stream::<f32, _, _>(
            &config,
            {
                let ring = ring.clone();
                let gain = gain.clone();
                let paused = paused.clone();
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let silent = paused();
                    let level = f32::from_bits(gain.load(SeqCst));
                    if silent {
                        data.fill(0.0);
                        return;
                    }
                    match ring.try_lock() {
                        Ok(mut ring) => {
                            for sample in data.iter_mut() {
                                *sample = ring.samples.pop_front().unwrap_or(0.0) * level;
                            }
                        }
                        // The decoder holds the ring for a few microseconds; a short silence
                        // beats blocking the audio thread.
                        Err(_) => data.fill(0.0),
                    }
                }
            },
            |error| eprintln!("vidshow: audio device error: {error}"),
            None,
        );
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                let _ = ready.send(Err(format!("can't open the audio device: {error}")));
                return;
            }
        };
        if let Err(error) = stream.play() {
            let _ = ready.send(Err(format!("can't start the audio device: {error}")));
            return;
        }
        let _ = ready.send(Ok((rate, channels)));
        while !stop.load(SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(stream);
    }

    /// Decodes PCM into the ring, restarting ffmpeg for seeks.
    fn reader_loop(
        tools: Tools,
        info: MediaInfo,
        start: f64,
        rate: u32,
        channels: u16,
        ctl: Arc<Mutex<Ctl>>,
        ring: Arc<Mutex<Ring>>,
        process: Arc<Mutex<Option<Child>>>,
    ) {
        let mut pending_start = Some(start);
        let mut stdout: Option<std::process::ChildStdout> = None;
        let mut finished = false;
        let mut buffer = vec![0u8; 16 * 1024];

        loop {
            let (stop, paused, seek) = {
                let Ok(mut ctl) = ctl.lock() else { return };
                (ctl.stop, ctl.paused, ctl.seek.take())
            };
            if stop {
                if let Ok(mut process) = process.lock() {
                    if let Some(mut child) = process.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
                return;
            }
            if let Some(seconds) = seek {
                pending_start = Some(seconds);
                finished = false;
                stdout = None;
                if let Ok(mut process) = process.lock() {
                    if let Some(mut child) = process.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
                if let Ok(mut ring) = ring.lock() {
                    ring.samples.clear();
                }
            }
            if let Some(seconds) = pending_start.take() {
                match media::spawn_audio(&tools, &info, seconds, rate, channels) {
                    Ok((mut child, log)) => {
                        stdout = child.stdout.take();
                        if let Ok(mut process) = process.lock() {
                            *process = Some(child);
                        }
                        // Surfaces a decode error once, without stopping the picture.
                        let _log = log;
                    }
                    Err(_) => {
                        stdout = None;
                        finished = true;
                    }
                }
            }
            if finished {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let Some(out) = stdout.as_mut() else {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            };
            if paused {
                std::thread::sleep(Duration::from_millis(15));
                continue;
            }

            // Wait for the device to drain the ring, so ffmpeg is paced by playback rather than
            // decoding the whole track into memory.
            loop {
                let full = ring.lock().is_ok_and(|ring| ring.samples.len() + 2048 > ring.cap);
                if !full {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
                let (stop, seek) = ctl.lock().map(|c| (c.stop, c.seek.is_some())).unwrap_or((true, false));
                if stop || seek {
                    break;
                }
            }

            match out.read(&mut buffer) {
                Ok(0) => {
                    stdout = None;
                    finished = true;
                    if let Ok(mut process) = process.lock() {
                        if let Some(mut child) = process.take() {
                            let _ = child.wait();
                        }
                    }
                }
                Ok(n) => {
                    if let Ok(mut ring) = ring.lock() {
                        for chunk in buffer[..n].chunks_exact(4) {
                            ring.samples.push_back(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                        }
                    }
                }
                Err(_) => {
                    stdout = None;
                    finished = true;
                }
            }
        }
    }
}

/// Platforms other than Windows have no audio back end yet: video still presents, silently.
#[cfg(not(windows))]
mod imp {
    use crate::media::{MediaInfo, Tools};

    pub struct Audio;

    impl Audio {
        pub fn start(_tools: &Tools, _info: &MediaInfo, _start: f64) -> Result<Self, String> {
            Err("sound is only implemented on Windows for now".into())
        }
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn seek(&self, _seconds: f64) {}
        pub fn set_gain(&self, _gain: f32) {}
        pub fn stop(&mut self) {}
    }
}

pub use imp::Audio;
