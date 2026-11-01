//! Playback: one ffmpeg decode thread, a wall-clock, and the surfaces frames are pushed into.
//!
//! The clock is the source of truth. The decode thread sleeps until each frame's presentation
//! time, drops frames that are more than 200 ms late (so a slow decode skips instead of dragging
//! the picture behind the sound), and reopens ffmpeg for seeks. Pauses stop the thread consuming
//! frames, and the pipe's back pressure stops ffmpeg in turn.

use crate::audio::Audio;
use crate::media::{self, MediaInfo, Tools};
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use rune::{NativeSurface, Waker};
use std::io::BufReader;
use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Loading,
    Playing,
    Paused,
    Ended,
    Failed(String),
}

/// The wakers of the windows that draw this player. Windows register theirs as they start; a
/// surface push or a state change wakes every one of them.
#[derive(Default)]
pub struct Wakers(Mutex<Vec<Waker>>);

impl Wakers {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn add(&self, waker: Waker) {
        self.0.lock().unwrap().push(waker);
        // A file loaded before this window existed still has a frame to show.
        if let Some(last) = self.0.lock().unwrap().last() {
            last();
        }
    }

    pub fn wake(&self) {
        let list: Vec<Waker> = self.0.lock().unwrap().clone();
        for waker in list {
            waker();
        }
    }

    /// One waker that wakes every registered window, read at call time so late windows count.
    pub fn combined(self: &Arc<Self>) -> Waker {
        let wakers = self.clone();
        Arc::new(move || wakers.wake())
    }
}

#[derive(Clone, Copy)]
struct Clock {
    base: f64,
    at: Instant,
    playing: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self { base: 0.0, at: Instant::now(), playing: false }
    }
}

impl Clock {
    fn now(&self) -> f64 {
        if self.playing {
            self.base + self.at.elapsed().as_secs_f64()
        } else {
            self.base
        }
    }

    fn set(&mut self, seconds: f64) {
        self.base = seconds.max(0.0);
        self.at = Instant::now();
    }

    fn play(&mut self) {
        self.at = Instant::now();
        self.playing = true;
    }

    fn pause(&mut self) {
        let now = self.now();
        self.set(now);
        self.playing = false;
    }
}

#[derive(Default)]
struct Ctl {
    paused: bool,
    seek: Option<f64>,
}

struct Inner {
    state: Mutex<State>,
    clock: Mutex<Clock>,
    ctl: Mutex<Ctl>,
    cv: Condvar,
    stop: AtomicBool,
    presenting: AtomicBool,
    preview: AtomicBool,
    looping: AtomicBool,
    volume: std::sync::atomic::AtomicU32,
    muted: AtomicBool,
    surface: NativeSurface,
    preview_surface: NativeSurface,
    last: Mutex<Option<Arc<Vec<u8>>>>,
    /// Whether the projector surface has ever been handed a frame. A surface keeps its last
    /// frame while its window is hidden, so re-presenting must not overwrite it with an old one.
    surface_has_frame: AtomicBool,
    frame_id: AtomicU64,
    audio: Mutex<Option<Audio>>,
    audio_note: Mutex<Option<String>>,
    video_process: Mutex<Option<Child>>,
    wakers: Arc<Wakers>,
    info: MediaInfo,
}

impl Inner {
    fn new(info: MediaInfo, wakers: Arc<Wakers>) -> Arc<Self> {
        let surface = NativeSurface::new();
        let preview_surface = NativeSurface::new();
        let wake = wakers.combined();
        surface.set_waker(wake.clone());
        preview_surface.set_waker(wake);
        Arc::new(Self {
            state: Mutex::new(State::Loading),
            clock: Mutex::new(Clock::default()),
            ctl: Mutex::new(Ctl::default()),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
            presenting: AtomicBool::new(false),
            preview: AtomicBool::new(true),
            looping: AtomicBool::new(false),
            volume: std::sync::atomic::AtomicU32::new(1f32.to_bits()),
            muted: AtomicBool::new(false),
            surface,
            preview_surface,
            last: Mutex::new(None),
            surface_has_frame: AtomicBool::new(false),
            frame_id: AtomicU64::new(0),
            audio: Mutex::new(None),
            audio_note: Mutex::new(None),
            video_process: Mutex::new(None),
            wakers,
            info,
        })
    }

    fn set_state(&self, state: State) {
        let changed = {
            let mut slot = self.state.lock().unwrap();
            if *slot == state {
                false
            } else {
                *slot = state;
                true
            }
        };
        if changed {
            self.wakers.wake();
        }
    }

    fn fail(&self, error: String) {
        self.set_state(State::Failed(error));
    }

    fn kill_video(&self) {
        if let Ok(mut process) = self.video_process.lock() {
            if let Some(mut child) = process.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// Hand a decoded frame to the projector and, if it is on, the preview.
    fn publish(&self, bytes: Vec<u8>) {
        let id = self.frame_id.fetch_add(1, SeqCst);
        let frame = |bytes: Vec<u8>| ByteFrame {
            width: self.info.width,
            height: self.info.height,
            format: Format::Bgra,
            color: ColorInfo::default(),
            planes: [bytes, Vec::new()],
        };
        let preview = self.preview.load(SeqCst);
        if self.presenting.load(SeqCst) {
            if preview {
                let _ = self.preview_surface.push_bytes(frame(bytes.clone()), id);
            }
            let _ = self.surface.push_bytes(frame(bytes), id);
            self.surface_has_frame.store(true, SeqCst);
        } else {
            if preview {
                let _ = self.preview_surface.push_bytes(frame(bytes.clone()), id);
            }
            *self.last.lock().unwrap() = Some(Arc::new(bytes));
        }
    }

    /// Put the frame already decoded on the projector, so a clip that is paused before its first
    /// present still shows up.
    fn repaint(&self) {
        let Some(last) = self.last.lock().unwrap().clone() else { return };
        let id = self.frame_id.fetch_add(1, SeqCst);
        let _ = self.surface.push_bytes(
            ByteFrame {
                width: self.info.width,
                height: self.info.height,
                format: Format::Bgra,
                color: ColorInfo::default(),
                planes: [(*last).clone(), Vec::new()],
            },
            id,
        );
        self.surface_has_frame.store(true, SeqCst);
    }

    fn set_gain(&self) {
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            audio.set_gain(if self.muted() { 0.0 } else { self.volume() });
        }
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(SeqCst))
    }

    fn muted(&self) -> bool {
        self.muted.load(SeqCst)
    }

    fn shutdown(&self) {
        self.stop.store(true, SeqCst);
        self.cv.notify_all();
        self.kill_video();
        if let Some(mut audio) = self.audio.lock().unwrap().take() {
            audio.stop();
        }
    }
}

pub struct Player {
    pub info: MediaInfo,
    inner: Arc<Inner>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.inner.shutdown();
    }
}

impl Player {
    /// Reads the file's basics and starts decoding from the beginning.
    pub fn load(tools: &Tools, path: &Path, wakers: Arc<Wakers>) -> Result<Self, String> {
        let info = media::probe(tools, path)?;
        let inner = Inner::new(info.clone(), wakers);
        let player = Self { info: info.clone(), inner: inner.clone() };
        // A silent clip needs no audio device, and should not look like a failure.
        let audio = if info.has_audio {
            match Audio::start(tools, &info, 0.0) {
                Ok(audio) => Some(audio),
                Err(note) => {
                    *inner.audio_note.lock().unwrap() = Some(note);
                    None
                }
            }
        } else {
            None
        };
        // The clip autoplays, so sound starts with the picture.
        if let Some(audio) = audio.as_ref() {
            audio.play();
            audio.set_gain(1.0);
        }
        *inner.audio.lock().unwrap() = audio;
        *inner.clock.lock().unwrap() = Clock { base: 0.0, at: Instant::now(), playing: true };
        let tools = tools.clone();
        let decode = inner.clone();
        std::thread::spawn(move || decode_loop(decode, tools, info));
        player.inner.set_state(State::Playing);
        Ok(player)
    }

    pub fn state(&self) -> State {
        self.inner.state.lock().unwrap().clone()
    }

    pub fn is_playing(&self) -> bool {
        matches!(self.state(), State::Playing)
    }

    pub fn time(&self) -> f64 {
        let time = self.inner.clock.lock().unwrap().now();
        if self.info.duration > 0.0 {
            time.clamp(0.0, self.info.duration)
        } else {
            time.max(0.0)
        }
    }

    pub fn duration(&self) -> f64 {
        self.info.duration
    }

    pub fn progress(&self) -> f32 {
        if self.info.duration <= 0.0 {
            0.0
        } else {
            (self.time() / self.info.duration).clamp(0.0, 1.0) as f32
        }
    }

    pub fn audio_note(&self) -> Option<String> {
        self.inner.audio_note.lock().unwrap().clone()
    }

    pub fn surface(&self) -> &NativeSurface {
        &self.inner.surface
    }

    pub fn preview_surface(&self) -> &NativeSurface {
        &self.inner.preview_surface
    }

    pub fn play(&self) {
        if matches!(self.state(), State::Ended) {
            self.seek(0.0);
        }
        self.inner.clock.lock().unwrap().play();
        {
            let mut ctl = self.inner.ctl.lock().unwrap();
            ctl.paused = false;
        }
        self.inner.cv.notify_all();
        if let Some(audio) = self.inner.audio.lock().unwrap().as_ref() {
            audio.play();
        }
        self.inner.set_state(State::Playing);
        self.inner.wakers.wake();
    }

    pub fn pause(&self) {
        self.inner.clock.lock().unwrap().pause();
        {
            let mut ctl = self.inner.ctl.lock().unwrap();
            ctl.paused = true;
        }
        self.inner.cv.notify_all();
        if let Some(audio) = self.inner.audio.lock().unwrap().as_ref() {
            audio.pause();
        }
        self.inner.set_state(State::Paused);
        self.inner.wakers.wake();
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, seconds: f64) {
        let target = if self.info.duration > 0.0 {
            seconds.clamp(0.0, self.info.duration)
        } else {
            seconds.max(0.0)
        };
        self.inner.clock.lock().unwrap().set(target);
        {
            let mut ctl = self.inner.ctl.lock().unwrap();
            ctl.seek = Some(target);
        }
        self.inner.cv.notify_all();
        if let Some(audio) = self.inner.audio.lock().unwrap().as_ref() {
            audio.seek(target);
        }
        if matches!(self.state(), State::Ended) {
            self.inner.set_state(State::Paused);
        }
        self.inner.wakers.wake();
    }

    pub fn seek_by(&self, delta: f64) {
        self.seek(self.time() + delta);
    }

    pub fn seek_fraction(&self, fraction: f64) {
        self.seek(fraction.clamp(0.0, 1.0) * self.info.duration);
    }

    pub fn looping(&self) -> bool {
        self.inner.looping.load(SeqCst)
    }

    pub fn set_looping(&self, looping: bool) {
        self.inner.looping.store(looping, SeqCst);
    }

    pub fn muted(&self) -> bool {
        self.inner.muted()
    }

    pub fn set_muted(&self, muted: bool) {
        self.inner.muted.store(muted, SeqCst);
        self.inner.set_gain();
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner.volume.store(volume.clamp(0.0, 1.0).to_bits(), SeqCst);
        self.inner.set_gain();
    }

    pub fn set_preview(&self, on: bool) {
        self.inner.preview.store(on, SeqCst);
    }

    /// Show the picture on the projector surface (and stop decoding into a hidden window's one).
    pub fn set_presenting(&self, presenting: bool) {
        let was = self.inner.presenting.swap(presenting, SeqCst);
        if presenting && !was && !self.inner.surface_has_frame.load(SeqCst) {
            self.inner.repaint();
        }
        self.inner.wakers.wake();
    }
}

/// The decode thread: one ffmpeg at a time, reopened for seeks.
fn decode_loop(inner: Arc<Inner>, tools: Tools, info: MediaInfo) {
    let mut start = inner.clock.lock().unwrap().base;
    let frame_bytes = info.frame_bytes();
    let mut force_one = true;
    loop {
        if inner.stop.load(SeqCst) {
            inner.kill_video();
            return;
        }
        let (child, log) = match media::spawn_video(&tools, &info, start) {
            Ok(spawned) => spawned,
            Err(error) => {
                inner.fail(error);
                return;
            }
        };
        let mut child = child;
        let stdout = child.stdout.take();
        if let Ok(mut process) = inner.video_process.lock() {
            *process = Some(child);
        }
        let Some(stdout) = stdout else {
            inner.fail("ffmpeg gave no output".into());
            return;
        };
        let mut reader = BufReader::with_capacity(1 << 16, stdout);
        let mut index: u64 = 0;
        let mut restart: Option<f64> = None;

        loop {
            if inner.stop.load(SeqCst) {
                inner.kill_video();
                return;
            }
            // Pick up a seek before touching the pipe: the seeker already killed ffmpeg.
            {
                let mut ctl = inner.ctl.lock().unwrap();
                if let Some(target) = ctl.seek.take() {
                    restart = Some(target);
                }
            }
            if let Some(target) = restart.take() {
                start = target;
                force_one = true;
                break;
            }
            if inner.ctl.lock().unwrap().paused {
                let ctl = inner.ctl.lock().unwrap();
                let _held = inner.cv.wait_timeout(ctl, Duration::from_millis(50));
                continue;
            }

            let mut buffer = vec![0u8; frame_bytes];
            let read = media::read_frame(&mut reader, &mut buffer);
            match read {
                Ok(true) => {}
                Ok(false) => {
                    if inner.stop.load(SeqCst) {
                        inner.kill_video();
                        return;
                    }
                    // The end, a seek, or ffmpeg giving up.
                    {
                        let mut ctl = inner.ctl.lock().unwrap();
                        if let Some(target) = ctl.seek.take() {
                            restart = Some(target);
                        }
                    }
                    if let Some(target) = restart.take() {
                        start = target;
                        force_one = true;
                        break;
                    }
                    if inner.looping.load(SeqCst) {
                        inner.clock.lock().unwrap().set(0.0);
                        if let Some(audio) = inner.audio.lock().unwrap().as_ref() {
                            audio.seek(0.0);
                        }
                        start = 0.0;
                        break;
                    }
                    inner.clock.lock().unwrap().set(info.duration);
                    inner.clock.lock().unwrap().pause();
                    if let Some(audio) = inner.audio.lock().unwrap().as_ref() {
                        audio.pause();
                    }
                    inner.set_state(State::Ended);
                    // Wait for a replay, a seek or the app closing.
                    loop {
                        if inner.stop.load(SeqCst) {
                            inner.kill_video();
                            return;
                        }
                        let mut ctl = inner.ctl.lock().unwrap();
                        if let Some(target) = ctl.seek.take() {
                            start = target;
                            force_one = true;
                            break;
                        }
                        let _held = inner.cv.wait_timeout(ctl, Duration::from_millis(100));
                    }
                    break;
                }
                Err(error) => {
                    // A decode error after some frames is usually a damaged tail; report it.
                    let stderr = log.lock().map(|text| text.trim().to_string()).unwrap_or_default();
                    let detail = if stderr.is_empty() { error } else { stderr };
                    inner.fail(detail);
                    inner.kill_video();
                    return;
                }
            }

            let pts = start + index as f64 / info.fps;
            index += 1;

            // Hold the frame until its presentation time.
            let mut abort = false;
            loop {
                if inner.stop.load(SeqCst) {
                    inner.kill_video();
                    return;
                }
                let paused = {
                    let mut ctl = inner.ctl.lock().unwrap();
                    if let Some(target) = ctl.seek.take() {
                        restart = Some(target);
                    }
                    ctl.paused
                };
                if let Some(target) = restart.take() {
                    start = target;
                    index = 0;
                    force_one = true;
                    abort = true;
                    break;
                }
                if paused {
                    break;
                }
                let now = inner.clock.lock().unwrap().now();
                if now + 0.001 >= pts {
                    break;
                }
                std::thread::sleep(Duration::from_secs_f64((pts - now).min(0.02).max(0.001)));
            }
            if abort {
                break;
            }
            // While paused only the frame decoded for the seek (or the first one) is shown.
            let paused = inner.ctl.lock().unwrap().paused;
            if paused && !force_one {
                continue;
            }
            let lateness = inner.clock.lock().unwrap().now() - pts;
            if lateness > 0.20 && !force_one {
                continue;
            }
            inner.publish(buffer);
            force_one = false;
            if matches!(*inner.state.lock().unwrap(), State::Loading) {
                inner.set_state(State::Playing);
            }
        }
        inner.kill_video();
    }
}
