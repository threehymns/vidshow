//! The control panel, and the projector that opens on the display you chose.
//!
//! The control window is the one with every control; the projector covers its display exactly
//! and shows nothing but the picture — the same shape of thing as PowerPoint's presenter output
//! or an OBS projector, without exclusive full screen mode. It is a window wherever a window can
//! be placed (Windows, X11, macOS); on Wayland, which gives clients no say in where a window
//! goes, it is a layer-shell overlay pinned to the chosen output instead.

use crate::media::Tools;
use crate::player::{Player, State, Wakers};
use crate::settings::{DisplayKey, FitMode, Settings, TargetDisplay};
use rune::tok::{self, type_};
use rune::{
    Align, AppHandle, Color, Cursor, Cx, Display, DisplayTarget, Drag, El, FileDialogOptions, Icon,
    Key, KeyEvent, OverlayHandle, OverlayPlacement, Phase, Side, Tx, WindowHandle, col, el, icon,
    row, spacer, text,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The formats the file picker offers first. ffmpeg itself reads far more; this is only a filter.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "flv", "f4v", "mpg", "mpeg", "m2v",
    "ts", "m2ts", "mts", "ogv", "ogm", "3gp", "3g2", "vob", "mxf", "divx", "rm", "rmvb", "y4m",
];

const STOP: Color = Color { r: 0.80, g: 0.26, b: 0.26, a: 1.0 };
const DANGER: Color = Color { r: 0.95, g: 0.45, b: 0.42, a: 1.0 };

/// The caret on the display picker, drawn as a path so it does not depend on the font's glyphs.
static CHEVRON_DOWN: Icon = Icon {
    name: "vidshow-chevron-down",
    view: 16.0,
    body: r#"<path d="M5 6.5L8 9.5L11 6.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// The control window.
pub struct Presenter {
    pub tools: Result<Tools, String>,
    pub player: Option<Arc<Player>>,
    pub path: Option<PathBuf>,
    pub error: Option<String>,
    pub cfg: Settings,
    pub displays: Vec<Display>,
    pub presenting: bool,
    /// Whether the "Present on" picker is open over the control window.
    pub picker_open: bool,
    pub projection: Projector,
    pub wakers: Arc<Wakers>,
    /// A file from the command line, opened once the event loop is up.
    pub pending: Option<PathBuf>,
    pub auto_present: bool,
    pub autoplay: bool,
    /// While the pointer drags the seek bar: the fraction it is over.
    pub scrub: Option<f64>,
}

/// The projector.
pub struct Projection {
    pub player: Option<Arc<Player>>,
    pub fit: FitMode,
    pub hide_cursor: bool,
    pub presenter: AppHandle<Presenter>,
    pub wakers: Arc<Wakers>,
}

/// A display's size in points: what window sizes are asked for in, and the size the projector
/// window follows when you present on a different display.
pub fn display_points(display: &Display) -> (f32, f32) {
    let scale = if display.scale > 0.0 { display.scale } else { 1.0 };
    (display.size.0 as f32 / scale as f32, display.size.1 as f32 / scale as f32)
}

/// How rune names a display, for placing a surface on it.
pub fn display_target(display: &Display) -> DisplayTarget {
    match &display.id {
        Some(id) => DisplayTarget::Id(id.clone()),
        None => DisplayTarget::Primary,
    }
}

/// Where the picture goes. A window can be placed on a display by every platform vidshow runs on
/// except Wayland, which leaves the output to the compositor; there the picture is a layer-shell
/// overlay, the one surface a client can pin to a chosen output.
pub enum Projector {
    Window {
        handle: WindowHandle,
        state: AppHandle<Projection>,
    },
    Overlay(OverlayHandle<Projection>),
}

impl Projector {
    pub fn window(handle: WindowHandle, state: AppHandle<Projection>) -> Self {
        Self::Window { handle, state }
    }

    pub fn overlay(handle: OverlayHandle<Projection>) -> Self {
        Self::Overlay(handle)
    }

    /// Show the picture on `display`, covering it whole.
    fn show(&self, display: &Display) {
        match self {
            Self::Window { handle, .. } => {
                // The size follows the display chosen while presenting; where `place` also sizes
                // windows it asks for the same rectangle.
                handle.resize_instant(display_points(display));
                // A borderless window covering the display, the way PowerPoint's presenter output
                // and an OBS projector do it: no exclusive full screen, so other windows can still
                // come up.
                handle.place(display_target(display), OverlayPlacement::FullScreen);
                handle.open();
            }
            Self::Overlay(handle) => {
                handle.set_display(display_target(display));
                handle.show();
            }
        }
    }

    fn hide(&self) {
        match self {
            Self::Window { handle, .. } => handle.hide(),
            Self::Overlay(handle) => handle.hide(),
        }
    }

    /// `On top` for the window. A layer-shell overlay already sits above ordinary windows.
    fn set_always_on_top(&self, on: bool) {
        if let Self::Window { handle, .. } = self {
            handle.set_always_on_top(on);
        }
    }

    /// Push the current clip, fit and cursor policy to the projector.
    fn sync(&self, player: Option<Arc<Player>>, fit: FitMode, hide_cursor: bool) {
        match self {
            Self::Window { state, .. } => state.update(move |projection, _| {
                projection.player = player;
                projection.fit = fit;
                projection.hide_cursor = hide_cursor;
            }),
            Self::Overlay(handle) => handle.update(move |projection, _| {
                projection.player = player;
                projection.fit = fit;
                projection.hide_cursor = hide_cursor;
            }),
        }
    }
}

pub fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.floor() as i64;
    let (hours, rest) = (total / 3600, total % 3600);
    if hours > 0 {
        format!("{hours}:{:02}:{:02}", rest / 60, rest % 60)
    } else {
        format!("{}:{:02}", rest / 60, rest % 60)
    }
}

fn button(label: impl Into<String>, primary: bool) -> El {
    let theme = tok::theme(true);
    let accent = tok::accent(0, true);
    let (fill, ink) = if primary { (accent, Color::WHITE) } else { (theme.control_off, theme.text) };
    row()
        .h(32.0)
        .px(13.0)
        .radius(8.0)
        .center_y()
        .fill(fill)
        .cursor(Cursor::Pointer)
        .child(text(label).font(type_::LABEL).color(ink))
}

fn toggle(label: impl Into<String>, on: bool) -> El {
    let theme = tok::theme(true);
    let accent = tok::accent(0, true);
    row()
        .h(30.0)
        .px(11.0)
        .radius(8.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .fill(if on { accent.alpha(0.9) } else { theme.control_off })
        .child(text(label).font(type_::LABEL).color(if on { Color::WHITE } else { theme.text_secondary }))
}

fn caption(label: impl Into<String>) -> El {
    let theme = tok::theme(true);
    text(label).font(type_::LABEL).color(theme.text_secondary)
}

/// How a display is named in the picker: "2 · 1920×1080 (primary)".
fn display_label(display: &Display, index: usize) -> String {
    let key = DisplayKey::of(display);
    let mut label = format!("{} · {}", index + 1, key.resolution());
    if display.primary {
        label.push_str(" (primary)");
    }
    label
}

impl Presenter {
    fn save(&self) {
        self.cfg.save();
    }

    fn state_label(&self) -> String {
        match &self.player {
            None => "No file".to_string(),
            Some(player) => match player.state() {
                State::Loading => "Opening".to_string(),
                State::Playing => "Playing".to_string(),
                State::Paused => "Paused".to_string(),
                State::Ended => "Finished".to_string(),
                State::Failed(error) => error,
            },
        }
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

    fn play_pause(&mut self) {
        if let Some(player) = &self.player {
            player.toggle();
        }
    }

    fn seek_by(&mut self, delta: f64) {
        if let Some(player) = &self.player {
            player.seek_by(delta);
        }
    }

    fn restart(&mut self) {
        if let Some(player) = &self.player {
            player.seek(0.0);
        }
    }

    fn toggle_loop(&mut self) {
        self.cfg.looping = !self.cfg.looping;
        if let Some(player) = &self.player {
            player.set_looping(self.cfg.looping);
        }
        self.save();
    }

    fn toggle_mute(&mut self) {
        self.cfg.muted = !self.cfg.muted;
        if let Some(player) = &self.player {
            player.set_muted(self.cfg.muted);
        }
        self.save();
    }

    fn change_volume(&mut self, delta: f32) {
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

    fn toggle_fit(&mut self) {
        self.cfg.fit = self.cfg.fit.other();
        self.save();
        self.sync_projection();
    }

    fn toggle_preview(&mut self) {
        self.cfg.preview = !self.cfg.preview;
        if let Some(player) = &self.player {
            player.set_preview(self.cfg.preview);
        }
        self.save();
    }

    fn toggle_on_top(&mut self) {
        self.cfg.always_on_top = !self.cfg.always_on_top;
        self.projection.set_always_on_top(self.cfg.always_on_top);
        self.save();
    }

    fn toggle_cursor(&mut self) {
        self.cfg.hide_cursor = !self.cfg.hide_cursor;
        self.save();
        self.sync_projection();
    }

    fn choose_display(&mut self, index: usize) {
        let Some(display) = self.displays.get(index) else { return };
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
        self.projection.sync(self.player.clone(), self.cfg.fit, self.cfg.hide_cursor);
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

    fn status_line(&self) -> String {
        match &self.player {
            None => match &self.tools {
                Ok(tools) => format!("Ready. Decoding with {}", tools.ffmpeg.display()),
                Err(_) => "ffmpeg was not found".to_string(),
            },
            Some(player) => {
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
                parts.join("  ·  ")
            }
        }
    }
}

fn pick_file(s: &mut Presenter, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Open a video".into(),
        directory: s.cfg.last_dir.clone(),
        file_name: None,
        filters: vec![
            ("Video".to_string(), VIDEO_EXTENSIONS.iter().map(|ext| (*ext).to_string()).collect()),
            ("All files".to_string(), vec!["*".to_string()]),
        ],
    };
    tx.open_file(options, |s: &mut Presenter, result, _tx| match result {
        Ok(Some(path)) => s.open(path),
        Ok(None) => {}
        Err(error) => s.error = Some(format!("the file dialog failed: {}", error.0)),
    });
}

fn seek_bar(s: &Presenter, duration: f64, progress: f32) -> El {
    let theme = tok::theme(true);
    let accent = tok::accent(0, true);
    let shown = s
        .scrub
        .map(|fraction| fraction.clamp(0.0, 1.0))
        .unwrap_or(progress as f64) as f32;
    el()
        .key("seek")
        .w_full()
        .h(18.0)
        .grow(1.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .child(
            el()
                .w_full()
                .h(6.0)
                .radius(3.0)
                .fill(theme.control_track)
                .child(el().w_frac(shown).h(6.0).radius(3.0).fill(accent)),
        )
        .on_drag(move |s: &mut Presenter, drag: &mut Drag, _tx: &mut Tx| {
            if duration <= 0.0 {
                return;
            }
            let fraction = (drag.x / drag.w.max(1.0)).clamp(0.0, 1.0) as f64;
            match drag.phase {
                Phase::Down | Phase::Up => {
                    s.scrub = None;
                    if let Some(player) = &s.player {
                        player.seek_fraction(fraction);
                    }
                }
                Phase::Move => s.scrub = Some(fraction),
            }
        })
}

pub fn presenter_view(s: &Presenter, _cx: &mut Cx<Presenter>) -> El {
    let theme = tok::theme(true);
    let accent = tok::accent(0, true);
    let (time, duration, progress, playing) = match &s.player {
        Some(player) => (player.time(), player.duration(), player.progress(), player.is_playing()),
        None => (0.0, 0.0, 0.0, false),
    };
    let shown_time = s.scrub.map(|fraction| fraction * duration).unwrap_or(time);

    let file_name = s
        .player
        .as_ref()
        .map(|player| player.info.title.clone())
        .or_else(|| {
            s.path
                .as_ref()
                .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "Open a video, drop one here, or use Open With".to_string());

    let header = row()
        .h(36.0)
        .gap(10.0)
        .center_y()
        .w_full()
        .child(text("vidshow").font(type_::TITLE).color(theme.text))
        .child(text(s.state_label()).font(type_::LABEL).color(if s.presenting { accent } else { theme.text_secondary }))
        .child(spacer())
        .child(text(file_name).font(type_::BODY).color(theme.text))
        .child(spacer())
        .child(button("Open…", false).on_press_tx(pick_file))
        .child(if s.presenting {
            button("Stop presenting", false).fill(STOP).on_press(Presenter::toggle_present)
        } else {
            button("Present", true).on_press(Presenter::toggle_present)
        });

    let stage = match &s.player {
        Some(player) if s.cfg.preview => el()
            .key("stage")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .radius(10.0)
            .clip_rounded(10.0)
            .fill(Color::BLACK)
            .child(
                el()
                    .key("preview")
                    .w_full()
                    .h_full()
                    .native_surface(player.preview_surface(), s.cfg.fit.surface()),
            ),
        _ => el()
            .key("stage")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .radius(10.0)
            .fill(Color::BLACK)
            .center()
            .child(
                text(if s.player.is_some() {
                    "Preview is off"
                } else {
                    "Drop a video file here to load it"
                })
                .font(type_::TITLE)
                .color(theme.text_secondary),
            ),
    };

    let scrub_row = row()
        .gap(10.0)
        .center_y()
        .w_full()
        .child(text(clock(shown_time)).font(type_::MONO_CAPTION).color(theme.text))
        .child(seek_bar(s, duration, progress))
        .child(text(clock(duration)).font(type_::MONO_CAPTION).color(theme.text_secondary));

    let volume = (s.cfg.volume() * 100.0).round() as i32;
    let transport = row()
        .gap(8.0)
        .center_y()
        .w_full()
        .child(button("Restart", false).on_press(Presenter::restart))
        .child(button("−10s", false).on_press(|s: &mut Presenter| s.seek_by(-10.0)))
        .child(
            button(if playing { "Pause" } else { "Play" }, true).on_press(Presenter::play_pause),
        )
        .child(button("+10s", false).on_press(|s: &mut Presenter| s.seek_by(10.0)))
        .child(spacer())
        .child(toggle("Loop", s.cfg.looping).on_press(Presenter::toggle_loop))
        .child(
            toggle(if s.cfg.muted { "Muted" } else { "Sound" }, !s.cfg.muted)
                .on_press(Presenter::toggle_mute),
        )
        .child(button("Vol −", false).on_press(|s: &mut Presenter| s.change_volume(-0.05)))
        .child(text(format!("{volume}%")).font(type_::MONO_CAPTION).color(theme.text_secondary))
        .child(button("Vol +", false).on_press(|s: &mut Presenter| s.change_volume(0.05)))
        .child(button(s.cfg.fit.label(), false).on_press(Presenter::toggle_fit));

    let selected = s.cfg.resolve(&s.displays);
    // With no display there is nothing to pick from, so the picker never opens.
    let open = s.picker_open && !s.displays.is_empty();
    let trigger_label = match selected {
        Some(index) => display_label(&s.displays[index], index),
        None => "No display".to_string(),
    };
    let trigger = row()
        .key("present_on")
        .h(30.0)
        .px(11.0)
        .gap(8.0)
        .radius(8.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .fill(if open { accent.alpha(0.9) } else { theme.control_off })
        .child(text(trigger_label).font(type_::LABEL).color(if open { Color::WHITE } else { theme.text }))
        .child(
            icon(&CHEVRON_DOWN, 14.0)
                .icon_rotation(if open { std::f32::consts::PI } else { 0.0 })
                .color(if open { Color::WHITE } else { theme.text_secondary }),
        )
        .on_press(|s: &mut Presenter| {
            if !s.displays.is_empty() {
                s.picker_open = !s.picker_open;
            }
        });
    let display_row = row()
        .gap(8.0)
        .center_y()
        .w_full()
        .flex_wrap()
        .child(caption("Present on"))
        .child(trigger)
        .child(spacer())
        .child(toggle("On top", s.cfg.always_on_top).on_press(Presenter::toggle_on_top))
        .child(toggle("Preview", s.cfg.preview).on_press(Presenter::toggle_preview))
        .child(toggle("Hide cursor", s.cfg.hide_cursor).on_press(Presenter::toggle_cursor));

    let mut footer = col().gap(2.0).w_full().child(
        text(s.status_line()).font(type_::MONO_CAPTION).color(theme.text_secondary),
    );
    if let Some(error) = &s.error {
        footer = footer.child(text(error).font(type_::LABEL).color(DANGER));
    } else if let Err(error) = &s.tools {
        footer = footer.child(text(error).font(type_::LABEL).color(DANGER));
    }

    // The list itself: one row per display, the chosen one marked, hanging under the trigger.
    let mut picker = col()
        .key("present_on_picker")
        .min_w(190.0)
        .radius(8.0)
        .fill(theme.control_off)
        .padding(4.0)
        .gap(2.0);
    for (index, display) in s.displays.iter().enumerate() {
        let on = selected == Some(index);
        let label = display_label(display, index);
        picker = picker.child(
            row()
                .key(format!("present_on_option{index}"))
                .h(28.0)
                .px(8.0)
                .radius(6.0)
                .center_y()
                .cursor(Cursor::Pointer)
                .fill(if on { accent.alpha(0.9) } else { theme.control_off })
                .child(text(label).font(type_::LABEL).color(if on { Color::WHITE } else { theme.text }))
                .on_press(move |s: &mut Presenter| {
                    s.choose_display(index);
                    s.picker_open = false;
                }),
        );
    }
    let picker = picker.anchored("present_on", Side::Bottom, Align::Start, 6.0);

    let mut root = col()
        .w_full()
        .h_full()
        .fill(theme.bg)
        .padding(14.0)
        .gap(10.0)
        .child(header)
        .child(stage)
        .child(scrub_row)
        .child(transport)
        .child(el().h(1.0).w_full().fill(theme.separator))
        .child(display_row)
        .child(footer)
        .on_drop_files(|s: &mut Presenter, files, _tx| {
            if let Some(first) = files.first() {
                s.open(first.clone());
            }
        });
    if open {
        // The list hangs under its trigger; the transparent scrim catches presses outside it, so
        // they close the picker instead of reaching the controls underneath.
        root = root
            .child(
                el()
                    .key("present_on_scm")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .on_press(|s: &mut Presenter| s.picker_open = false),
            )
            .child(picker);
    }
    root
}

pub fn projection_view(p: &Projection, _cx: &mut Cx<Projection>) -> El {
    let mut root = el().key("projector").w_full().h_full().fill(Color::BLACK);
    if let Some(player) = &p.player {
        root = root.child(
            el()
                .key("frame")
                .w_full()
                .h_full()
                .native_surface(player.surface(), p.fit.surface())
                .cursor(if p.hide_cursor { Cursor::Hidden } else { Cursor::Default }),
        );
    }
    root
}

pub fn projection_closed(p: &mut Projection, _tx: &mut Tx) {
    if let Some(player) = &p.player {
        player.set_presenting(false);
    }
    let presenter = p.presenter.clone();
    presenter.update(|s, _| s.presenting = false);
    p.wakers.wake();
}

pub fn presenter_setup(s: &mut Presenter, waker: rune::Waker) {
    s.wakers.add(waker);
    if let Some(path) = s.pending.take() {
        s.open(path);
    }
}

pub fn projection_setup(p: &mut Projection, waker: rune::Waker) {
    p.wakers.add(waker);
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

pub fn presenter_key(s: &mut Presenter, event: &KeyEvent, tx: &mut Tx) -> bool {
    match &event.key {
        Key::Text(written) => match written.as_str() {
            " " => {
                s.play_pause();
                true
            }
            "f" | "F" => {
                s.toggle_present();
                true
            }
            "m" | "M" => {
                s.toggle_mute();
                true
            }
            "l" | "L" => {
                s.toggle_loop();
                true
            }
            "v" | "V" => {
                s.toggle_preview();
                true
            }
            "c" | "C" => {
                s.toggle_cursor();
                true
            }
            _ => false,
        },
        Key::Left => {
            let step = if event.mods.shift { -30.0 } else { -5.0 };
            s.seek_by(step);
            true
        }
        Key::Right => {
            let step = if event.mods.shift { 30.0 } else { 5.0 };
            s.seek_by(step);
            true
        }
        Key::Up => {
            s.change_volume(0.05);
            true
        }
        Key::Down => {
            s.change_volume(-0.05);
            true
        }
        Key::Home => {
            s.restart();
            true
        }
        Key::Escape => {
            // Escape closes the open picker first; only then does it stop presenting.
            if s.picker_open {
                s.picker_open = false;
                true
            } else if s.presenting {
                s.stop_presenting();
                true
            } else {
                false
            }
        }
        Key::Function(5) => {
            s.toggle_present();
            true
        }
        Key::Command('o') => {
            pick_file(s, tx);
            true
        }
        _ => false,
    }
}

pub fn projection_key(p: &mut Projection, event: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(player) = p.player.clone() else {
        // Nothing to control; let Escape close the projector anyway.
        if matches!(event.key, Key::Escape) {
            tx.close_window();
            return true;
        }
        return false;
    };
    match &event.key {
        Key::Escape => {
            tx.close_window();
            true
        }
        Key::Text(written) => match written.as_str() {
            " " => {
                player.toggle();
                true
            }
            "m" | "M" => {
                player.set_muted(!player.muted());
                true
            }
            "l" | "L" => {
                player.set_looping(!player.looping());
                true
            }
            _ => false,
        },
        Key::Left => {
            player.seek_by(if event.mods.shift { -30.0 } else { -5.0 });
            true
        }
        Key::Right => {
            player.seek_by(if event.mods.shift { 30.0 } else { 5.0 });
            true
        }
        Key::Home => {
            player.seek(0.0);
            true
        }
        _ => false,
    }
}
