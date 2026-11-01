//! What the control window draws: the header, the stage, the seek bar and the transport.
//!
//! The marks at the top are drawn here because the icon catalog has no monitor, repeat or speaker
//! glyph; everything else comes from the kit.

use super::{Presenter, pick_file};
use crate::settings::DisplayKey;
use rune::{Color, Cx, Display, Drag, El, Icon, Phase, Tx, col, el, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Progress, Select,
};
use rune_theme_opencode::icons as oc;

/// The marks OpenCode's icon catalog has no glyph for — the display picker's monitor and the two
/// playback toggles — drawn on the catalog's own grid: 16pt, 1pt stroke.
static MONITOR: Icon = Icon {
    name: "vidshow-monitor",
    view: 16.0,
    body: r#"<rect x="2" y="3" width="12" height="8" rx="1" stroke="currentColor"/><path d="M6 13h4M8 11v2" stroke="currentColor" stroke-linecap="round"/>"#,
};

/// Loop, the way a player draws it: two arrows chasing each other around the loop.
static LOOP: Icon = Icon {
    name: "vidshow-loop",
    view: 16.0,
    body: r#"<path d="M11.3 1.3 14 4l-2.7 2.7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/><path d="M2 7.3V6.7A2.7 2.7 0 0 1 4.7 4H14" stroke="currentColor" stroke-linecap="round"/><path d="M4.7 14.7 2 12l2.7-2.7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/><path d="M14 8.7V9.3A2.7 2.7 0 0 1 11.3 12H2" stroke="currentColor" stroke-linecap="round"/>"#,
};

/// Loop off: the loop with its bottom arrow taken away, leaving a single arrow that goes up and
/// over. Centred in the same 16pt box, so the two states sit on the same line.
static LOOP_OFF: Icon = Icon {
    name: "vidshow-loop-off",
    view: 16.0,
    body: r#"<path d="M11.3 5 14 7.7l-2.7 2.7" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"/><path d="M2 11V10.4A2.7 2.7 0 0 1 4.7 7.7H14" stroke="currentColor" stroke-linecap="round"/>"#,
};

/// Sound on: a speaker cone with two waves.
static SPEAKER: Icon = Icon {
    name: "vidshow-speaker",
    view: 16.0,
    body: r#"<path d="M9 4 5.5 6.5H2.5v3h3L9 12z" stroke="currentColor" stroke-linejoin="round"/><path d="M10.7 6.3a2.6 2.6 0 0 1 0 3.4M12.5 4.8a4.8 4.8 0 0 1 0 6.4" stroke="currentColor" stroke-linecap="round"/>"#,
};

/// Sound off: the same cone, with the waves replaced by a cross.
static SPEAKER_MUTED: Icon = Icon {
    name: "vidshow-speaker-muted",
    view: 16.0,
    body: r#"<path d="M9 4 5.5 6.5H2.5v3h3L9 12z" stroke="currentColor" stroke-linejoin="round"/><path d="M11 6.2l3.2 3.2M14.2 6.2 11 9.4" stroke="currentColor" stroke-linecap="round"/>"#,
};

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

/// How a display is named in the picker: "2 · 1920×1080 (primary)".
fn display_label(display: &Display, index: usize) -> String {
    let key = DisplayKey::of(display);
    let mut label = format!("{} · {}", index + 1, key.resolution());
    if display.primary {
        label.push_str(" (primary)");
    }
    label
}

/// Loop as a toggle button: as on a player, on and off are different marks, and the brightness
/// follows the same state.
fn loop_button(k: &Kit, s: &Presenter) -> El {
    let on = s.cfg.looping;
    IconButton::new(if on { &LOOP } else { &LOOP_OFF })
        .variant(if on {
            IconButtonVariant::Ghost
        } else {
            IconButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Large)
        .help(if on { "Loop on" } else { "Loop off" })
        .on_press(Presenter::toggle_loop)
        .build(k)
}

/// Sound as a toggle button: the glyph and the brightness both follow it, so muted never looks
/// like playing in silence.
fn sound_button(k: &Kit, s: &Presenter) -> El {
    let on = !s.cfg.muted;
    IconButton::new(if on { &SPEAKER } else { &SPEAKER_MUTED })
        .variant(if on {
            IconButtonVariant::Ghost
        } else {
            IconButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Large)
        .help(if on { "Sound on" } else { "Sound off" })
        .on_press(Presenter::toggle_mute)
        .build(k)
}

fn seek_bar(k: &Kit, s: &Presenter, duration: f64, progress: f32) -> El {
    let shown = s
        .scrub
        .map(|fraction| fraction.clamp(0.0, 1.0))
        .unwrap_or(progress as f64) as f32;
    row()
        .key("seek")
        .w_full()
        .h(18.0)
        .grow(1.0)
        .center_y()
        .child(Progress::new(Some(shown)).build(k))
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

pub fn presenter_view(s: &Presenter, cx: &mut Cx<Presenter>) -> El {
    let k = rune_theme_opencode::kit(cx.system_dark());
    let (time, duration, progress, playing) = match &s.player {
        Some(player) => (
            player.time(),
            player.duration(),
            player.progress(),
            player.is_playing(),
        ),
        None => (0.0, 0.0, 0.0, false),
    };
    let shown_time = s.scrub.map(|fraction| fraction * duration).unwrap_or(time);

    let file_name = s
        .player
        .as_ref()
        .map(|player| player.info.title.clone())
        .or_else(|| {
            s.path.as_ref().and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
        });

    // The clip's own details (size, frame rate, audio) hang off an info icon beside the title.
    let mut title = row().gap(4.0).center_y();
    if let Some(name) = &file_name {
        title = title.child(text(name.clone()).font(k.ty.ui).color(k.c.text_base));
    }
    if let Some(info) = s.video_info() {
        // A plain mark in a fixed box: no button chrome, and nothing to mis-centre.
        title = title.child(
            el().key("video-info")
                .size(20.0, 20.0)
                .center()
                .hit()
                .help(info)
                .child(icon(&oc::INFO, 14.0).color(k.c.icon_faint)),
        );
    }

    let selected = s.cfg.resolve(&s.displays);
    let labels: Vec<String> = s
        .displays
        .iter()
        .enumerate()
        .map(|(index, display)| display_label(display, index))
        .collect();
    // With no display there is nothing to pick from, so the listbox never opens.
    let open = s.picker_open && !s.displays.is_empty();
    // The listbox is mounted as an overlay, above the rest of the control window.
    let mut layer = Vec::new();
    let trigger = row()
        .key("present_on_mark")
        .gap(6.0)
        .center_y()
        .child(icon(&MONITOR, 14.0).color(k.c.icon_muted))
        .child(
            Select::new("present_on", &labels)
                .current(selected.map(|index| labels[index].as_str()))
                .placeholder("No display")
                .open(open)
                .on_toggle(|s: &mut Presenter| {
                    if !s.displays.is_empty() {
                        s.picker_open = !s.picker_open;
                    }
                })
                .on_close(|s: &mut Presenter| s.picker_open = false)
                .on_choose(|s: &mut Presenter, value: &str, _tx: &mut Tx| {
                    let index = s
                        .displays
                        .iter()
                        .enumerate()
                        .position(|(index, display)| display_label(display, index) == value);
                    if let Some(index) = index {
                        s.choose_display(index);
                        s.picker_open = false;
                    }
                })
                .build(k, &mut layer),
        );

    let header = row()
        .h(36.0)
        .gap(10.0)
        .center_y()
        .w_full()
        .child(title)
        .child(trigger)
        .child(spacer())
        .child(
            IconButton::new(&oc::SETTINGS_GEAR)
                .control_size(ControlSize::Large)
                .help("Settings")
                .on_press(|s: &mut Presenter| s.settings_open = true)
                .build(k),
        )
        .child(
            Button::new("Open…")
                .control_size(ControlSize::Large)
                .on_press_tx(pick_file)
                .build(k),
        )
        .child(if s.presenting {
            Button::new("End")
                .control_size(ControlSize::Large)
                .on_press(Presenter::toggle_present)
                .build(k)
        } else {
            Button::new("Present")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Large)
                .on_press(Presenter::toggle_present)
                .build(k)
        });

    let stage = match &s.player {
        Some(player) if s.cfg.preview => el()
            .key("stage")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .radius(k.radius.xl)
            .clip_rounded(k.radius.xl)
            .fill(Color::BLACK)
            .child(
                el().key("preview")
                    .w_full()
                    .h_full()
                    .native_surface(player.preview_surface(), s.cfg.fit.surface()),
            )
            .on_press(Presenter::play_pause),
        _ => el()
            .key("stage")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .radius(k.radius.xl)
            .fill(Color::BLACK)
            .center()
            .child(
                text(if s.player.is_some() {
                    "Preview is off"
                } else {
                    "Drop a video file here"
                })
                .font(k.ty.ui_medium)
                .color(k.c.text_muted),
            )
            .on_press(Presenter::play_pause),
    };

    let scrub_row = row()
        .gap(10.0)
        .center_y()
        .w_full()
        .child(text(clock(shown_time)).font(k.ty.mono).color(k.c.text_base))
        .child(seek_bar(k, s, duration, progress))
        .child(text(clock(duration)).font(k.ty.mono).color(k.c.text_muted));

    let transport = row()
        .gap(8.0)
        .center_y()
        .w_full()
        .child(
            Button::new(if playing { "Pause" } else { "Play" })
                .control_size(ControlSize::Large)
                .on_press(Presenter::play_pause)
                .build(k),
        )
        .child(spacer())
        .child(loop_button(k, s))
        .child(sound_button(k, s))
        .child(
            Button::new(s.cfg.fit.label())
                .control_size(ControlSize::Large)
                .on_press(Presenter::toggle_fit)
                .build(k),
        );

    // Errors are the only standing text: everything else is a control or the picture.
    let problem = s
        .error
        .as_deref()
        .or_else(|| s.tools.as_ref().err().map(String::as_str));

    let mut root = col()
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .padding(14.0)
        .gap(10.0)
        .child(header)
        .child(stage)
        .child(scrub_row)
        .child(transport)
        .on_drop_files(|s: &mut Presenter, files, _tx| {
            if let Some(first) = files.first() {
                s.open(first.clone());
            }
        });
    if let Some(problem) = problem {
        root = root.child(
            text(problem)
                .font(k.ty.text_xsmall_medium)
                .color(k.c.danger.fg),
        );
    }
    crate::settings_window::settings_window(s, k, &mut layer);
    rune_kit::root(root, layer)
}
