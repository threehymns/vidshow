//! The control window: every control, what pressing it does, and the state behind them.
//!
//! The picture itself goes out through [`crate::projection`], which is also the only code that
//! knows how a window is placed on a display. This module decides what to show and where; the
//! view module draws it.

mod view;

pub use view::*;

use crate::keybinds::Keybinds;
use crate::media::Tools;
use crate::player::{Player, State, Wakers};
use crate::projection::Projector;
use crate::settings::{DisplayKey, Settings, TargetDisplay};
use rune::{Display, FileDialogOptions, Tx};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The formats the file picker offers first. ffmpeg itself reads far more; this is only a filter.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "flv", "f4v", "mpg", "mpeg", "m2v",
    "ts", "m2ts", "mts", "ogv", "ogm", "3gp", "3g2", "vob", "mxf", "divx", "rm", "rmvb", "y4m",
];

/// The control window.
pub struct Presenter {
    pub tools: Result<Tools, String>,
    /// The shortcut table this window answers to.
    pub keybinds: Keybinds,
    pub player: Option<Arc<Player>>,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
    pub cfg: Settings,
    pub displays: Vec<Display>,
    pub presenting: bool,
    /// Whether the "Present on" picker is open over the control window.
    pub picker_open: bool,
    /// Whether the settings popup is open over the control window.
    pub settings_open: bool,
    pub projection: Projector,
    pub wakers: Arc<Wakers>,
    /// A file from the command line, opened once the event loop is up.
    pub pending: Option<PathBuf>,
    pub auto_present: bool,
    pub autoplay: bool,
    /// While the pointer drags the seek bar: the fraction it is over.
    pub scrub: Option<f64>,
}

impl Presenter {
    fn save(&self) {
        self.cfg.save();
    }

    pub fn open(&mut self, path: PathBuf) {
        self.error = None;
        // Drops the previous clip, which stops its decoder and audio threads.
        self.player = None;
        let tools = match &self.tools {
            Ok(tools) => tools.clone(),
            Err(error) => {
                self.error = Some(error.clone());
                return;
            }
        };
        match Player::load(&tools, &path, self.wakers.clone()) {
            Ok(player) => {
                player.set_preview(self.cfg.preview);
                player.set_looping(self.cfg.looping);
                player.set_volume(self.cfg.volume());
                player.set_muted(self.cfg.muted);
                // A clip opened while presenting goes straight to the projector.
                player.set_presenting(self.presenting);
                if !self.autoplay {
                    player.pause();
                }
                self.cfg.last_dir = path.parent().map(std::path::Path::to_path_buf);
                self.path = Some(path);
                self.save();
                self.player = Some(Arc::new(player));
            }
            Err(error) => {
                self.error = Some(error);
                self.player = None;
            }
        }
        // Either way, the projector lets go of the old clip (which stops its threads).
        self.sync_projection();
    }

    pub(crate) fn play_pause(&mut self) {
        if let Some(player) = &self.player {
            player.toggle();
        }
    }

    pub(crate) fn seek_by(&mut self, delta: f64) {
        if let Some(player) = &self.player {
            player.seek_by(delta);
        }
    }

    pub(crate) fn restart(&mut self) {
        if let Some(player) = &self.player {
            player.seek(0.0);
        }
    }

    pub(crate) fn toggle_loop(&mut self) {
        self.cfg.looping = !self.cfg.looping;
        if let Some(player) = &self.player {
            player.set_looping(self.cfg.looping);
        }
        self.save();
    }

    pub(crate) fn toggle_mute(&mut self) {
        self.cfg.muted = !self.cfg.muted;
        if let Some(player) = &self.player {
            player.set_muted(self.cfg.muted);
        }
        self.save();
    }

    pub(crate) fn change_volume(&mut self, delta: f32) {
        self.cfg.volume = (self.cfg.volume() + delta).clamp(0.0, 1.0);
        if self.cfg.volume > 0.0 {
            self.cfg.muted = false;
        }
        if let Some(player) = &self.player {
            player.set_volume(self.cfg.volume());
            player.set_muted(self.cfg.muted);
        }
        self.save();
    }

    pub(crate) fn toggle_fit(&mut self) {
        self.cfg.fit = self.cfg.fit.other();
        self.save();
        self.sync_projection();
    }

    pub(crate) fn toggle_preview(&mut self) {
        self.cfg.preview = !self.cfg.preview;
        if let Some(player) = &self.player {
            player.set_preview(self.cfg.preview);
        }
        self.save();
    }

    pub(crate) fn toggle_on_top(&mut self) {
        self.cfg.always_on_top = !self.cfg.always_on_top;
        self.projection.set_always_on_top(self.cfg.always_on_top);
        self.save();
    }

    pub(crate) fn toggle_cursor(&mut self) {
        self.cfg.hide_cursor = !self.cfg.hide_cursor;
        self.save();
        self.sync_projection();
    }

    pub(crate) fn choose_display(&mut self, index: usize) {
        let Some(display) = self.displays.get(index) else {
            return;
        };
        self.cfg.display = if display.primary {
            TargetDisplay::Primary
        } else {
            TargetDisplay::Display(DisplayKey::of(display))
        };
        self.save();
        if self.presenting {
            self.start_presenting();
        }
    }

    /// Push the current clip, fit and cursor policy to the projector.
    fn sync_projection(&self) {
        self.projection
            .sync(self.player.clone(), self.cfg.fit, self.cfg.hide_cursor);
    }

    pub fn toggle_present(&mut self) {
        if self.presenting {
            self.stop_presenting();
        } else {
            self.start_presenting();
        }
    }

    pub fn start_presenting(&mut self) {
        let Some(index) = self.cfg.resolve(&self.displays) else {
            self.error = Some("no display is available to present on".into());
            return;
        };
        let display = self.displays[index].clone();
        self.projection.show(&display);
        self.projection.set_always_on_top(self.cfg.always_on_top);
        self.presenting = true;
        self.error = None;
        if let Some(player) = &self.player {
            player.set_presenting(true);
            if !player.is_playing() && matches!(player.state(), State::Paused) {
                player.play();
            }
        }
        self.sync_projection();
    }

    pub fn stop_presenting(&mut self) {
        self.projection.hide();
        self.presenting = false;
        if let Some(player) = &self.player {
            player.set_presenting(false);
        }
    }

    /// The clip's own details, shown as the header info icon's tooltip.
    pub(crate) fn video_info(&self) -> Option<String> {
        let player = self.player.as_ref()?;
        let mut parts = vec![
            format!("{}\u{d7}{}", player.info.width, player.info.height),
            format!("{:.2} fps", player.info.fps),
        ];
        if player.info.rotation != 0 {
            parts.push(format!("rotated {}°", player.info.rotation));
        }
        if let Some(note) = player.audio_note() {
            parts.push(note);
        }
        Some(parts.join("  ·  "))
    }
}

pub(crate) fn pick_file(s: &mut Presenter, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Open a video".into(),
        directory: s.cfg.last_dir.clone(),
        file_name: None,
        filters: vec![
            (
                "Video".to_string(),
                VIDEO_EXTENSIONS
                    .iter()
                    .map(|ext| (*ext).to_string())
                    .collect(),
            ),
            ("All files".to_string(), vec!["*".to_string()]),
        ],
    };
    tx.open_file(options, |s: &mut Presenter, result, _tx| match result {
        Ok(Some(path)) => s.open(path),
        Ok(None) => {}
        Err(error) => s.error = Some(format!("the file dialog failed: {}", error.0)),
    });
}

pub fn presenter_setup(s: &mut Presenter, waker: rune::Waker) {
    s.wakers.add(waker);
    if let Some(path) = s.pending.take() {
        s.open(path);
    }
}

pub fn presenter_poll(s: &mut Presenter, _now: f64, tx: &mut Tx) -> bool {
    if s.auto_present {
        s.auto_present = false;
        s.start_presenting();
        return true;
    }
    // Keep the clock and progress bar moving while the clip plays.
    if s.player.as_ref().is_some_and(|player| player.is_playing()) {
        tx.wake_after(Duration::from_millis(60));
        return true;
    }
    false
}
