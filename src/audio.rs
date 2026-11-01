//! Audio output for the presentation.
//!
//! Video goes through Rune's `NativeSurface`; sound has no Rune element to play it (Rune's own
//! `Player` is macOS-only), so vidshow runs a second ffmpeg for the audio track and feeds the
//! samples to the default output device through cpal — WASAPI on Windows, ALSA on Linux. Any
//! failure here is reported as a note in the control window: losing sound never stops the picture.

#[cfg(any(windows, target_os = "linux"))]
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

    /// Whether the device is taking samples.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Phase {
        Running,
        Paused,
        Stopped,
    }

    struct Ctl {
        phase: Phase,
        seek: Option<f64>,
    }

    /// What the reader thread, the audio callback and the player all share.
    #[derive(Clone)]
    struct Shared {
        ctl: Arc<Mutex<Ctl>>,
        ring: Arc<Mutex<Ring>>,
        process: Arc<Mutex<Option<Child>>>,
        /// What went wrong with the decoder, for the control window to say.
        note: Arc<Mutex<Option<String>>>,
    }

    impl Shared {
        /// Stops the decoder ffmpeg, reaping it so the next one can start straight away.
        fn stop_process(&self) {
            if let Ok(mut process) = self.process.lock()
                && let Some(mut child) = process.take()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        }

        /// Keeps a problem for the control window instead of dropping it.
        fn record(&self, text: impl Into<String>) {
            let text = text.into();
            let text = text.trim().to_string();
            if text.is_empty() {
                return;
            }
            if let Ok(mut note) = self.note.lock() {
                *note = Some(text);
            }
        }
    }

    pub struct Audio {
        shared: Shared,
        gain: Arc<AtomicU32>,
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
            let shared = Shared {
                ctl: Arc::new(Mutex::new(Ctl {
                    phase: Phase::Paused,
                    seek: None,
                })),
                ring: Arc::new(Mutex::new(Ring {
                    samples: VecDeque::new(),
                    cap: 1,
                })),
                process: Arc::new(Mutex::new(None)),
                note: Arc::new(Mutex::new(None)),
            };
            let gain = Arc::new(AtomicU32::new(1f32.to_bits()));
            let host_stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = mpsc::channel();
            let host = {
                let (ring, ctl, gain, host_stop) = (
                    shared.ring.clone(),
                    shared.ctl.clone(),
                    gain.clone(),
                    host_stop.clone(),
                );
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
            if let Ok(mut ring) = shared.ring.lock() {
                ring.cap =
                    (rate as usize * channels as usize * BUFFER_MS as usize / 1000).max(1024);
            }
            let reader = {
                let shared = shared.clone();
                let (tools, info) = (tools.clone(), info.clone());
                std::thread::spawn(move || reader_loop(tools, info, start, rate, channels, shared))
            };
            Ok(Self {
                shared,
                gain,
                reader: Some(reader),
                host: Some(host),
                host_stop,
            })
        }

        pub fn play(&self) {
            if let Ok(mut ctl) = self.shared.ctl.lock() {
                ctl.phase = Phase::Running;
            }
        }

        pub fn pause(&self) {
            if let Ok(mut ctl) = self.shared.ctl.lock() {
                ctl.phase = Phase::Paused;
            }
        }

        pub fn seek(&self, seconds: f64) {
            if let Ok(mut ctl) = self.shared.ctl.lock() {
                ctl.seek = Some(seconds.max(0.0));
            }
            // The reader starts the next ffmpeg at the new place.
            self.shared.stop_process();
        }

        pub fn set_gain(&self, gain: f32) {
            self.gain.store(gain.clamp(0.0, 1.0).to_bits(), SeqCst);
        }

        /// Anything the decoder said, for the control window.
        pub fn note(&self) -> Option<String> {
            self.shared.note.lock().ok().and_then(|note| note.clone())
        }

        pub fn stop(&mut self) {
            if let Ok(mut ctl) = self.shared.ctl.lock() {
                ctl.phase = Phase::Stopped;
            }
            self.shared.stop_process();
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
                let _ = ready.send(Err(format!(
                    "the audio device has no output format: {error}"
                )));
                return;
            }
        };
        let config: cpal::StreamConfig = supported.config();
        let (rate, channels) = (config.sample_rate.0, config.channels);
        let silent = {
            let ctl = ctl.clone();
            move || {
                ctl.lock()
                    .map(|c| c.phase != Phase::Running)
                    .unwrap_or(true)
            }
        };
        let stream = device.build_output_stream::<f32, _, _>(
            &config,
            {
                let ring = ring.clone();
                let gain = gain.clone();
                let silent = silent.clone();
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if silent() {
                        data.fill(0.0);
                        return;
                    }
                    let level = f32::from_bits(gain.load(SeqCst));
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
        shared: Shared,
    ) {
        let mut pending_start = Some(start);
        let mut stdout: Option<std::process::ChildStdout> = None;
        let mut log: Option<crate::media::Stderr> = None;
        let mut finished = false;
        let mut buffer = vec![0u8; 16 * 1024];

        loop {
            let (stopped, paused, seek) = {
                let Ok(mut ctl) = shared.ctl.lock() else {
                    return;
                };
                (
                    ctl.phase == Phase::Stopped,
                    ctl.phase == Phase::Paused,
                    ctl.seek.take(),
                )
            };
            if stopped {
                shared.stop_process();
                return;
            }
            if let Some(seconds) = seek {
                pending_start = Some(seconds);
                finished = false;
                stdout = None;
                shared.stop_process();
                if let Ok(mut ring) = shared.ring.lock() {
                    ring.samples.clear();
                }
            }
            if let Some(seconds) = pending_start.take() {
                match media::spawn_audio(&tools, &info, seconds, rate, channels) {
                    Ok((mut child, stderr)) => {
                        stdout = child.stdout.take();
                        log = Some(stderr);
                        if let Ok(mut process) = shared.process.lock() {
                            *process = Some(child);
                        }
                    }
                    Err(error) => {
                        stdout = None;
                        finished = true;
                        shared.record(error);
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
                let full = shared
                    .ring
                    .lock()
                    .is_ok_and(|ring| ring.samples.len() + 2048 > ring.cap);
                if !full {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
                let (stopped, seek) = shared
                    .ctl
                    .lock()
                    .map(|c| (c.phase == Phase::Stopped, c.seek.is_some()))
                    .unwrap_or((true, false));
                if stopped || seek {
                    break;
                }
            }

            match out.read(&mut buffer) {
                Ok(0) => {
                    stdout = None;
                    finished = true;
                    if let Ok(mut process) = shared.process.lock()
                        && let Some(mut child) = process.take()
                    {
                        let _ = child.wait();
                    }
                }
                Ok(n) => {
                    if let Ok(mut ring) = shared.ring.lock() {
                        let (samples, _rest) = buffer[..n].as_chunks::<4>();
                        for chunk in samples {
                            ring.samples.push_back(f32::from_le_bytes(*chunk));
                        }
                    }
                }
                Err(error) => {
                    stdout = None;
                    finished = true;
                    // The picture carries on; the control window says what ffmpeg complained of.
                    let said = log
                        .as_ref()
                        .and_then(|log| log.lock().ok().map(|text| text.trim().to_string()))
                        .unwrap_or_default();
                    shared.record(if said.is_empty() {
                        error.to_string()
                    } else {
                        said
                    });
                }
            }
        }
    }
}

/// Platforms without a cpal back end: video still presents, silently.
#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use crate::media::{MediaInfo, Tools};

    pub struct Audio;

    impl Audio {
        pub fn start(_tools: &Tools, _info: &MediaInfo, _start: f64) -> Result<Self, String> {
            Err("sound is not available on this platform".into())
        }
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn seek(&self, _seconds: f64) {}
        pub fn set_gain(&self, _gain: f32) {}
        pub fn note(&self) -> Option<String> {
            None
        }
        pub fn stop(&mut self) {}
    }
}

pub use imp::Audio;
