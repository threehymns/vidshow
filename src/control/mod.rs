//! The control window: the clip, the settings, and what every control does.
//!
//! The picture itself goes out through [`crate::projection`]. This module decides what state there
//! is and what each action means; the view module draws it, and the keys module maps chords onto
//! the same actions the buttons use.

mod keys;
mod settings_popup;
mod view;

pub use keys::presenter_key;
pub use view::presenter_view;

use crate::cli::Startup;
use crate::keybinds::{Action, Keybinds};
use crate::media::{Rotation, Tools};
use crate::player::{Player, State, Wakers};
use crate::projection::Projector;
use crate::settings::{DisplayKey, Settings, TargetDisplay};
use rune::{Display, FileDialogOptions, Tx};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The formats the file picker offers first. ffmpeg itself reads far more; this is only a filter.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "flv", "f4v", "mpg", "mpeg", "m2v",
    "ts", "m2ts", "mts", "ogv", "ogm", "3gp", "3g2", "vob", "mxf", "divx", "rm", "rmvb", "y4m",
];

/// The control window's one open file.
pub enum Clip {
    None,
    /// The file could not be opened, and why.
    Failed(String),
    Open(Arc<Player>),
}

impl Clip {
    pub fn player(&self) -> Option<&Arc<Player>> {
        match self {
            Self::Open(player) => Some(player),
            _ => None,
        }
    }

    /// Why there is no picture, when a file was asked for and did not open.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// The one thing open over the control window, if any. Two panels cannot be up at once, so there
/// is no pair of flags to disagree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlay {
    #[default]
    None,
    /// The display picker, over the header.
    DisplayPicker,
    /// The settings dialog, which is modal.
    Settings,
}

/// The control window.
pub struct Presenter {
    pub tools: Result<Tools, String>,
    pub keybinds: Keybinds,
    pub clip: Clip,
    pub cfg: Settings,
    pub displays: Vec<Display>,
    pub overlay: Overlay,
    pub projection: Projector,
    pub wakers: Arc<Wakers>,
    /// The latest problem the window itself knows about: a save that failed, an unreadable settings
    /// file, an argument vidshow could not follow. The clip's own errors are read from the clip.
    pub notice: Option<String>,
    /// The problem whose toast was closed. A later problem replaces it and shows again; the same
    /// one stays closed, so a standing failure does not keep coming back.
    pub dismissed: Option<String>,
    /// A file from the command line, opened once the event loop is up.
    pub startup: Option<Startup>,
    /// While the pointer drags the seek bar: the fraction it is over.
    pub scrub: Option<f64>,
}

impl Presenter {
    /// Runs an action asked for from this window.
    pub fn perform(&mut self, action: Action, tx: &mut Tx) -> bool {
        match action {
            Action::Open => {
                self.pick(tx);
                true
            }
            Action::Close => self.close_top(),
            Action::Present => {
                self.toggle_present();
                true
            }
            Action::Preview => {
                self.toggle_preview();
                true
            }
            Action::Cursor => {
                self.toggle_cursor();
                true
            }
            transport => self.perform_playback(transport),
        }
    }

    /// Runs an action that only moves the clip, which the projector's window can ask for too.
    /// Returns false for the actions that need the control window itself.
    pub fn perform_playback(&mut self, action: Action) -> bool {
        if !action.is_transport() {
            return false;
        }
        match action {
            Action::PlayPause => self.toggle_playing(),
            Action::Mute => self.toggle_mute(),
            Action::Loop => self.toggle_loop(),
            Action::Restart => self.restart(),
            Action::SeekBack => self.seek_by(-5.0),
            Action::SeekBackFast => self.seek_by(-30.0),
            Action::SeekForward => self.seek_by(5.0),
            Action::SeekForwardFast => self.seek_by(30.0),
            Action::VolumeUp => self.change_volume(0.05),
            Action::VolumeDown => self.change_volume(-0.05),
            _ => return false,
        }
        true
    }

    /// Opens a file in place of the clip that was there, and plays it.
    pub fn load(&mut self, path: PathBuf) {
        self.load_with(path, true);
    }

    /// Opens a file for `--no-play`, which waits on its first frame.
    pub fn load_paused(&mut self, path: PathBuf) {
        self.load_with(path, false);
    }

    fn load_with(&mut self, path: PathBuf, autoplay: bool) {
        // Drops the previous clip first: that stops its decoder and audio threads.
        self.clip = Clip::None;
        self.notice = None;
        self.dismissed = None;
        let loaded = match &self.tools {
            Ok(tools) => Player::load(tools, &path, self.wakers.clone()),
            Err(error) => Err(error.clone()),
        };
        self.clip = match loaded {
            Ok(player) => {
                if !autoplay {
                    player.pause();
                }
                self.cfg.last_dir = path.parent().map(Path::to_path_buf);
                Clip::Open(Arc::new(player))
            }
            Err(error) => Clip::Failed(error),
        };
        // The new clip, or the loss of the old one, reaches the projector here.
        self.apply();
    }

    fn toggle_playing(&mut self) {
        if let Some(player) = self.clip.player() {
            player.toggle();
        }
    }

    fn seek_by(&mut self, delta: f64) {
        if let Some(player) = self.clip.player() {
            player.seek_by(delta);
        }
    }

    fn restart(&mut self) {
        if let Some(player) = self.clip.player() {
            player.seek(0.0);
        }
    }

    fn toggle_loop(&mut self) {
        self.cfg.looping = !self.cfg.looping;
        self.apply();
    }

    fn toggle_mute(&mut self) {
        self.cfg.muted = !self.cfg.muted;
        self.apply();
    }

    fn change_volume(&mut self, delta: f32) {
        self.cfg.volume = (self.cfg.volume + delta).clamp(0.0, 1.0);
        if self.cfg.volume > 0.0 {
            self.cfg.muted = false;
        }
        self.apply();
    }

    fn toggle_fit(&mut self) {
        self.cfg.fit = self.cfg.fit.other();
        self.apply();
    }

    fn toggle_preview(&mut self) {
        self.cfg.preview = !self.cfg.preview;
        self.apply();
    }

    fn toggle_on_top(&mut self) {
        self.cfg.always_on_top = !self.cfg.always_on_top;
        self.apply();
    }

    fn toggle_cursor(&mut self) {
        self.cfg.hide_cursor = !self.cfg.hide_cursor;
        self.apply();
    }

    pub fn toggle_present(&mut self) {
        if self.projection.is_presenting() {
            self.stop_presenting();
        } else {
            self.start_presenting();
        }
    }

    pub fn start_presenting(&mut self) {
        let Some(index) = self.selected_display() else {
            self.report("no display is available to present on".into());
            return;
        };
        let display = self.displays[index].clone();
        self.projection.show(&display);
        self.notice = None;
        self.dismissed = None;
        // Presenting a paused clip means showing it.
        if let Some(player) = self.clip.player()
            && player.state() == State::Paused
        {
            player.play();
        }
        self.apply();
    }

    pub fn stop_presenting(&mut self) {
        self.projection.hide();
        self.apply();
    }

    /// Closes `overlay` if it is the one up, so a dismissal that arrives late cannot close a panel
    /// opened since.
    pub fn dismiss(&mut self, overlay: Overlay) {
        if self.overlay == overlay {
            self.overlay = Overlay::None;
        }
    }

    /// Opens the display picker, or closes it when it is already open.
    pub fn toggle_display_picker(&mut self) {
        self.overlay = if self.overlay == Overlay::DisplayPicker || self.displays.is_empty() {
            Overlay::None
        } else {
            Overlay::DisplayPicker
        };
    }

    fn choose_display(&mut self, index: usize) {
        let Some(display) = self.displays.get(index) else {
            return;
        };
        self.cfg.display = if display.primary {
            TargetDisplay::Primary
        } else {
            TargetDisplay::Display(DisplayKey::of(display))
        };
        self.overlay = Overlay::None;
        if self.projection.is_presenting() {
            // The picture has to move to the display that was just chosen.
            self.start_presenting();
        } else {
            self.apply();
        }
    }

    /// Applies a choice made in the picker, which reports the label rather than the index.
    fn choose_display_label(&mut self, label: &str) -> bool {
        let Some(index) = self.display_labels().iter().position(|text| text == label) else {
            return false;
        };
        self.choose_display(index);
        true
    }

    /// The display the presentation goes to.
    fn selected_display(&self) -> Option<usize> {
        self.cfg.resolve(&self.displays)
    }

    /// How each display is named in the picker, in display order.
    fn display_labels(&self) -> Vec<String> {
        self.displays
            .iter()
            .enumerate()
            .map(|(index, display)| display_label(display, index))
            .collect()
    }

    /// The clip's own details, shown as the header info icon's tooltip.
    fn video_info(&self) -> Option<String> {
        let player = self.clip.player()?;
        let mut parts = vec![
            format!("{}\u{d7}{}", player.info.width, player.info.height),
            format!("{:.2} fps", player.info.fps.value()),
        ];
        if player.info.rotation != Rotation::None {
            parts.push(format!("rotated {}°", player.info.rotation.degrees()));
        }
        if let Some(note) = player.audio_note() {
            parts.push(note);
        }
        Some(parts.join("  ·  "))
    }

    /// The one thing wrong to show: the latest notice, then the clip, then a missing ffmpeg.
    fn problem(&self) -> Option<String> {
        self.notice
            .clone()
            .or_else(|| self.clip.error().map(str::to_string))
            // A decode that failed after the clip opened never becomes a `Clip::Failed`.
            .or_else(|| self.clip.player().and_then(|player| player.error()))
            .or_else(|| self.tools.as_ref().err().cloned())
    }

    /// The problem the error toast should be showing: the latest one, unless it was closed and
    /// nothing newer has gone wrong since.
    pub fn toast(&self) -> Option<String> {
        self.problem()
            .filter(|problem| self.dismissed.as_deref() != Some(problem.as_str()))
    }

    /// Records a problem and makes sure its toast is up, even when it repeats one that was closed.
    fn report(&mut self, problem: String) {
        self.notice = Some(problem);
        self.dismissed = None;
    }

    /// Closes the error toast for the problem it is showing. A new problem raises a new one.
    pub fn dismiss_problem(&mut self) {
        self.dismissed = self.problem();
    }

    /// Pushes the settings onto the clip and the projector, and saves them. Every control ends
    /// here, so none of them has to know which of the two cares about what it changed.
    fn apply(&mut self) {
        let player = self.clip.player().cloned();
        if let Some(player) = &player {
            player.set_looping(self.cfg.looping);
            player.set_muted(self.cfg.muted);
            player.set_volume(self.cfg.volume);
            player.set_preview(self.cfg.preview);
            player.set_presenting(self.projection.is_presenting());
        }
        self.projection.set_always_on_top(self.cfg.always_on_top);
        self.projection
            .sync(player, self.cfg.fit, self.cfg.hide_cursor);
        self.save();
    }

    /// Persists the settings, showing a failure rather than dropping it.
    fn save(&mut self) {
        if let Err(error) = self.cfg.save() {
            self.report(error);
        }
    }

    /// Escape closes what is on top first: the dialog, the picker, then the presentation.
    fn close_top(&mut self) -> bool {
        match self.overlay {
            Overlay::Settings | Overlay::DisplayPicker => {
                self.overlay = Overlay::None;
                true
            }
            Overlay::None if self.projection.is_presenting() => {
                self.stop_presenting();
                true
            }
            Overlay::None => false,
        }
    }

    fn pick(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions {
            title: "Open a video".into(),
            directory: self.cfg.last_dir.clone(),
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
            Ok(Some(path)) => s.load(path),
            Ok(None) => {}
            Err(error) => s.report(format!("the file dialog failed: {}", error.0)),
        });
    }
}

/// How a display is named in the picker: "2 · 1920×1080 (primary)".
fn display_label(display: &Display, index: usize) -> String {
    let mut label = format!("{} · {}", index + 1, DisplayKey::of(display).resolution());
    if display.primary {
        label.push_str(" (primary)");
    }
    label
}

/// Registers the window's waker and applies what the command line asked for.
pub fn presenter_setup(s: &mut Presenter, waker: rune::Waker) {
    s.wakers.add(waker);
    let Some(startup) = s.startup.take() else {
        return;
    };
    if let Some(problem) = startup.problem {
        s.report(problem);
    }
    if let Some(path) = startup.file {
        if startup.autoplay {
            s.load(path);
        } else {
            s.load_paused(path);
        }
    }
    if startup.present {
        s.start_presenting();
    }
}

pub fn presenter_poll(s: &mut Presenter, _now: f64, tx: &mut Tx) -> bool {
    // Keep the clock and progress bar moving while the clip plays.
    if s.clip.player().is_some_and(|player| player.is_playing()) {
        tx.wake_after(Duration::from_millis(60));
        return true;
    }
    false
}
