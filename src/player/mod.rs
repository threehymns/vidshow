//! Playback: one ffmpeg decode thread, a wall clock, and the surfaces frames are pushed into.
//!
//! The clock is the source of truth. The decode thread sleeps until each frame's presentation time,
//! drops frames more than 200 ms late (so a slow decode skips instead of dragging the picture
//! behind the sound), and reopens ffmpeg for seeks. Pauses stop the thread consuming frames, and
//! the pipe's back pressure stops ffmpeg in turn.
//!
//! [`Inner`] holds what the decode thread, the sound and the windows share. [`Player`] is the handle
//! the rest of the app holds, and every transport change goes through the same few methods on
//! [`Inner`], so state, clock and sound move together.

mod decode;
mod playback;
mod wakers;

pub use playback::State;
pub use wakers::Wakers;

use crate::audio::Audio;
use crate::media::{self, MediaInfo, Tools};
use crate::settings::MAX_VOLUME;
use playback::Playback;
use rune::NativeSurface;
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use std::path::Path;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct Inner {
    playback: Mutex<Playback>,
    cv: Condvar,
    stop: AtomicBool,
    /// Whether decoded frames go to the projector's surface rather than the preview alone.
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
        self.playback.lock().unwrap().state().clone()
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
        self.playback.lock().unwrap().take_seek()
    }

    fn looping(&self) -> bool {
        self.looping.load(SeqCst)
    }

    /// Waits on the playback lock for at most `limit`, waking early for a state change.
    fn idle(&self, limit: Duration) {
        let guard = self.playback.lock().unwrap();
        let _ = self.cv.wait_timeout(guard, limit);
    }

    /// Plays from where the clock is. A clip that ran to its end starts again from the top.
    fn play(&self) {
        let restart = {
            let mut playback = self.playback.lock().unwrap();
            let restart = playback.state() == &State::Ended;
            if restart {
                playback.seek_to(0.0);
            }
            playback.set_state(State::Playing);
            restart
        };
        self.with_audio(|audio| {
            if restart {
                audio.seek(0.0);
            }
            audio.play();
        });
        self.wake();
    }

    fn pause(&self) {
        if self.set_state(State::Paused) {
            self.with_audio(Audio::pause);
            self.wake();
        }
    }

    /// Moves the clock, queues a seek for the decode thread and restarts the sound there.
    fn seek(&self, seconds: f64) {
        {
            let mut playback = self.playback.lock().unwrap();
            // A seek out of the end puts the clip back where it can play again.
            if playback.state() == &State::Ended {
                playback.set_state(State::Paused);
            }
            playback.seek_to(seconds);
        }
        self.with_audio(|audio| audio.seek(seconds));
        self.wake();
    }

    /// The clip ran to its end: park the clock there and let the windows show it as ended.
    fn finish(&self) {
        {
            let mut playback = self.playback.lock().unwrap();
            playback.set_time(self.info.duration);
            playback.set_state(State::Ended);
        }
        self.with_audio(Audio::pause);
        self.wake();
    }

    /// The clip loops: clock and sound go back to the start, and the next pass starts there, so no
    /// seek is queued for the thread that is already doing it.
    fn restart(&self) {
        self.playback.lock().unwrap().set_time(0.0);
        self.with_audio(|audio| audio.seek(0.0));
        self.wake();
    }

    /// A decode pass could not go on: keep the reason, so the control window can say it, and leave
    /// the last frame up.
    fn fail(&self, error: String) {
        if self.set_state(State::Failed(error)) {
            self.wake();
        }
    }

    fn set_state(&self, state: State) -> bool {
        self.playback.lock().unwrap().set_state(state)
    }

    /// Wakes the decode thread and every window drawing this clip.
    fn wake(&self) {
        self.cv.notify_all();
        self.wakers.wake();
    }

    fn with_audio(&self, act: impl FnOnce(&Audio)) {
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            act(audio);
        }
    }

    fn kill_video(&self) {
        if let Ok(mut process) = self.video_process.lock()
            && let Some(mut child) = process.take()
        {
            media::reap(&mut child);
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
        // The meters follow the volume but not the mute: muting fades what the section draws
        // instead of emptying it, so the bars stay where the signal actually is.
        self.with_audio(|audio| {
            audio.set_levels(
                if self.muted() { 0.0 } else { self.volume() },
                self.volume(),
            )
        });
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

/// A loaded clip. Dropping it stops the decode thread and the sound.
///
/// The state lives in [`Inner`] rather than here because the decode thread outlives this handle:
/// the thread holds an `Arc<Inner>`, so dropping the last `Player` can still stop and reap it.
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
        *inner.audio.lock().unwrap() = audio;
        // The clip autoplays, so sound starts with the picture.
        inner.play();
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

    /// Why the picture stopped changing, when a decode pass failed after the clip opened.
    pub fn error(&self) -> Option<String> {
        match self.state() {
            State::Failed(error) => Some(error),
            _ => None,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.state() == State::Playing
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

    /// The loudest sample each output channel has played since the last call, for the meters.
    /// Silence when the file has no audio track, and on platforms without sound.
    pub fn peaks(&self) -> [f32; 2] {
        self.inner
            .audio
            .lock()
            .unwrap()
            .as_ref()
            .map(Audio::peaks)
            .unwrap_or([0.0; 2])
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
        self.inner.play();
    }

    pub fn pause(&self) {
        self.inner.pause();
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, seconds: f64) {
        self.inner.seek(self.within_clip(seconds));
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
            .store(volume.clamp(0.0, MAX_VOLUME).to_bits(), SeqCst);
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
