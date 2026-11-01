//! Playback: one ffmpeg decode thread, a wall clock, and the surfaces frames are pushed into.
//!
//! The clock is the source of truth. The decode thread sleeps until each frame's presentation
//! time, drops frames more than 200 ms late (so a slow decode skips instead of dragging the
//! picture behind the sound), and reopens ffmpeg for seeks. Pauses stop the thread consuming
//! frames, and the pipe's back pressure stops ffmpeg in turn.
//!
//! The state, the clock and any seek still to be picked up live in one [`Playback`] behind one
//! lock, so what the windows draw, what the sound does and what the decode thread reads cannot
//! drift apart.

mod decode;

use crate::audio::Audio;
use crate::media::{self, MediaInfo, Tools};
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use rune::{NativeSurface, Waker};
use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What the clip is doing.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Loading,
    Playing,
    Paused,
    Ended,
    Failed(String),
}

impl State {
    /// Whether the clock advances: loading and playing do, everything else is parked.
    fn running(&self) -> bool {
        matches!(self, Self::Loading | Self::Playing)
    }
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
        // A file loaded before this window existed still has a frame to show it.
        waker();
        self.0.lock().unwrap().push(waker);
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

/// Where playback is in the clip, and when it last moved.
#[derive(Clone, Copy)]
struct Clock {
    base: f64,
    at: Instant,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            base: 0.0,
            at: Instant::now(),
        }
    }
}

impl Clock {
    fn now(&self, running: bool) -> f64 {
        if running {
            self.base + self.at.elapsed().as_secs_f64()
        } else {
            self.base
        }
    }

    fn set(&mut self, seconds: f64) {
        self.base = seconds.max(0.0);
        self.at = Instant::now();
    }
}

/// Everything the windows and the decode thread have to agree on.
#[derive(Default)]
struct Playback {
    state: State,
    clock: Clock,
    /// A seek the decode thread has not picked up yet.
    seek: Option<f64>,
}

impl Playback {
    fn time(&self) -> f64 {
        self.clock.now(self.state.running())
    }

    fn paused(&self) -> bool {
        !self.state.running()
    }

    /// Moves to `state`, parking or restarting the clock with it so the two stay in step.
    fn set_state(&mut self, state: State) -> bool {
        if self.state == state {
            return false;
        }
        let now = self.time();
        self.state = state;
        self.clock.set(now);
        true
    }

    /// Loading becomes playing once the first frame is up. Nothing else moves: a pause pressed
    /// while a frame was in flight still holds.
    fn promote(&mut self) -> bool {
        if self.state != State::Loading {
            return false;
        }
        self.set_state(State::Playing)
    }
}

struct Inner {
    playback: Mutex<Playback>,
    cv: Condvar,
    stop: AtomicBool,
    presenting: AtomicBool,
    preview: AtomicBool,
    looping: AtomicBool,
    volume: AtomicU32,
    muted: AtomicBool,
    surface: NativeSurface,
    preview_surface: NativeSurface,
    /// The last frame decoded while nothing was presenting, so Present has something to draw.
    last: Mutex<Option<Arc<Vec<u8>>>>,
    frame_id: AtomicU64,
    audio: Mutex<Option<Audio>>,
    /// Why there is no sound, told once to the control window.
    audio_note: Mutex<Option<String>>,
    /// The ffmpeg the current decode pass is reading from.
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
            playback: Mutex::new(Playback::default()),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
            presenting: AtomicBool::new(false),
            preview: AtomicBool::new(true),
            looping: AtomicBool::new(false),
            volume: AtomicU32::new(1f32.to_bits()),
            muted: AtomicBool::new(false),
            surface,
            preview_surface,
            last: Mutex::new(None),
            frame_id: AtomicU64::new(0),
            audio: Mutex::new(None),
            audio_note: Mutex::new(None),
            video_process: Mutex::new(None),
            wakers,
            info,
        })
    }

    fn state(&self) -> State {
        self.playback.lock().unwrap().state.clone()
    }

    fn now(&self) -> f64 {
        self.playback.lock().unwrap().time()
    }

    fn paused(&self) -> bool {
        self.playback.lock().unwrap().paused()
    }

    fn stopped(&self) -> bool {
        self.stop.load(SeqCst)
    }

    fn take_seek(&self) -> Option<f64> {
        self.playback.lock().unwrap().seek.take()
    }

    fn looping(&self) -> bool {
        self.looping.load(SeqCst)
    }

    /// Waits on the playback lock for at most `limit`, waking early for a state change.
    fn idle(&self, limit: Duration) {
        let guard = self.playback.lock().unwrap();
        let _ = self.cv.wait_timeout(guard, limit);
    }

    /// Applies a state change, and wakes the windows when it took.
    fn set_state(&self, state: State) {
        let changed = self.playback.lock().unwrap().set_state(state);
        if changed {
            self.wakers.wake();
        }
    }

    /// The first frame is up: a clip that was still loading is now playing.
    fn promote(&self) {
        let changed = self.playback.lock().unwrap().promote();
        if changed {
            self.wakers.wake();
        }
    }

    /// Moves the clock, queues a seek for the decode thread and restarts the sound there.
    fn seek_to(&self, seconds: f64) {
        {
            let mut playback = self.playback.lock().unwrap();
            // A seek out of the end puts the clip back where it can play again.
            if playback.state == State::Ended {
                playback.set_state(State::Paused);
            }
            playback.clock.set(seconds);
            playback.seek = Some(seconds);
        }
        self.cv.notify_all();
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            audio.seek(seconds);
        }
        self.wakers.wake();
    }

    /// The clip ran to its end: park the clock at the end and let the windows show it as ended.
    fn finish(&self) {
        {
            let mut playback = self.playback.lock().unwrap();
            playback.clock.set(self.info.duration);
            playback.set_state(State::Ended);
        }
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            audio.pause();
        }
        self.wakers.wake();
    }

    /// The clip loops: clock and sound go back to the start, and the next pass starts there, so
    /// no seek is queued for the thread that is already doing it.
    fn rewind(&self) {
        self.playback.lock().unwrap().clock.set(0.0);
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            audio.seek(0.0);
        }
        self.wakers.wake();
    }

    fn kill_video(&self) {
        if let Ok(mut process) = self.video_process.lock()
            && let Some(mut child) = process.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Hands the decode pass's ffmpeg over, so a shutdown can still stop it.
    fn set_video_process(&self, child: Child) {
        if let Ok(mut process) = self.video_process.lock() {
            *process = Some(child);
        }
    }

    fn frame(&self, bytes: Vec<u8>) -> ByteFrame {
        ByteFrame {
            width: self.info.width,
            height: self.info.height,
            format: Format::Bgra,
            color: ColorInfo::default(),
            planes: [bytes, Vec::new()],
        }
    }

    /// Hand a decoded frame to the projector and, if it is on, the preview.
    fn publish(&self, bytes: Vec<u8>) {
        let id = self.frame_id.fetch_add(1, SeqCst);
        if self.preview.load(SeqCst) {
            let _ = self
                .preview_surface
                .push_bytes(self.frame(bytes.clone()), id);
        }
        if self.presenting.load(SeqCst) {
            let _ = self.surface.push_bytes(self.frame(bytes), id);
        } else {
            *self.last.lock().unwrap() = Some(Arc::new(bytes));
        }
    }

    /// Put the frame already decoded on the projector, so a clip paused before its first present
    /// still shows up. A no-op until a frame has been decoded.
    fn repaint(&self) {
        let Some(last) = self.last.lock().unwrap().clone() else {
            return;
        };
        let id = self.frame_id.fetch_add(1, SeqCst);
        let _ = self.surface.push_bytes(self.frame((*last).clone()), id);
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
        inner.set_state(State::Playing);
        let player = Self {
            info: info.clone(),
            inner: inner.clone(),
        };
        let tools = tools.clone();
        std::thread::spawn(move || decode::decode_loop(inner, tools, info));
        Ok(player)
    }

    pub fn state(&self) -> State {
        self.inner.state()
    }

    pub fn is_playing(&self) -> bool {
        matches!(self.state(), State::Playing)
    }

    pub fn time(&self) -> f64 {
        self.within_clip(self.inner.now())
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
        let startup = self.inner.audio_note.lock().unwrap().clone();
        startup.or_else(|| {
            self.inner
                .audio
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|audio| audio.note())
        })
    }

    pub fn surface(&self) -> &NativeSurface {
        &self.inner.surface
    }

    pub fn preview_surface(&self) -> &NativeSurface {
        &self.inner.preview_surface
    }

    pub fn play(&self) {
        if self.state() == State::Ended {
            self.seek(0.0);
        }
        self.inner.set_state(State::Playing);
        self.inner.cv.notify_all();
        if let Some(audio) = self.inner.audio.lock().unwrap().as_ref() {
            audio.play();
        }
    }

    pub fn pause(&self) {
        self.inner.set_state(State::Paused);
        self.inner.cv.notify_all();
        if let Some(audio) = self.inner.audio.lock().unwrap().as_ref() {
            audio.pause();
        }
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, seconds: f64) {
        self.inner.seek_to(self.within_clip(seconds));
    }

    pub fn seek_by(&self, delta: f64) {
        self.seek(self.time() + delta);
    }

    pub fn seek_fraction(&self, fraction: f64) {
        self.seek(fraction.clamp(0.0, 1.0) * self.info.duration);
    }

    pub fn set_looping(&self, looping: bool) {
        self.inner.looping.store(looping, SeqCst);
    }

    pub fn set_muted(&self, muted: bool) {
        self.inner.muted.store(muted, SeqCst);
        self.inner.set_gain();
    }

    pub fn set_volume(&self, volume: f32) {
        self.inner
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), SeqCst);
        self.inner.set_gain();
    }

    pub fn set_preview(&self, on: bool) {
        self.inner.preview.store(on, SeqCst);
    }

    /// Show the picture on the projector surface (and stop decoding into a hidden window's one).
    pub fn set_presenting(&self, presenting: bool) {
        self.inner.presenting.store(presenting, SeqCst);
        if presenting {
            self.inner.repaint();
        }
        self.inner.wakers.wake();
    }

    /// A position inside the clip's own length, where ffprobe reported one.
    fn within_clip(&self, seconds: f64) -> f64 {
        if self.info.duration > 0.0 {
            seconds.clamp(0.0, self.info.duration)
        } else {
            seconds.max(0.0)
        }
    }
}
