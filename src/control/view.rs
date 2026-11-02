//! What the control window draws: the header, the stage, the seek bar and the transport.

use super::settings_popup;
use super::{Overlay, Presenter};
use crate::settings::FitMode;
use rune::{Color, Cx, Drag, El, Icon, Phase, Tx, col, el, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Progress, Select,
    Toast, ToastRegion,
};
use rune_theme_opencode::icons as oc;

/// The empty stage's mark: a video frame with its play triangle, drawn on the catalog's grid
/// (16pt, 1pt stroke) like the rest. No filmstrip rails: at icon size they cut through the
/// triangle and turn the frame into stripes.
static FILM: Icon = Icon {
    name: "vidshow-film",
    view: 16.0,
    body: r#"<rect x="2" y="4" width="12" height="8" rx="1.6" stroke="currentColor"/><path d="M6.6 6.1 10.4 8 6.6 9.9Z" fill="currentColor" stroke="currentColor" stroke-linejoin="round"/>"#,
};

/// The mark OpenCode's icon catalog has no glyph for: the display picker's monitor, drawn on the
/// catalog's own grid (16pt, 1pt stroke).
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

pub fn presenter_view(s: &Presenter, cx: &mut Cx<Presenter>) -> El {
    let k = rune_theme_opencode::kit(cx.system_dark());
    // Panels that float above the window are collected while the rest is built.
    let mut layer = Vec::new();
    // The window is the theme's base surface, not its deepest one: the chrome should read as an
    // ordinary app window, and the picture's own well is the dark part.
    let root = col()
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .padding(14.0)
        .gap(10.0)
        .child(header(s, k, &mut layer))
        .child(stage(s, k))
        .child(scrub_row(s, k))
        .child(transport(s, k))
        .child(s.volume.section(s, k))
        .on_drop_files(|s: &mut Presenter, files, _tx| {
            if let Some(first) = files.first() {
                s.load(first.clone());
            }
        });
    settings_popup::settings_window(s, k, &mut layer);
    // Last into the layer, so a toast sits above whatever panel is open.
    problem_toast(s, k, &mut layer);
    rune_kit::root(root, layer)
}

/// The one thing wrong, in the corner: every error the window knows about arrives here rather
/// than taking up room in the layout. Closing it keeps that error away until something else goes
/// wrong.
fn problem_toast(s: &Presenter, k: &Kit, layer: &mut Vec<El>) {
    let Some(problem) = s.toast() else {
        return;
    };
    let toast = Toast::new("problem", "Problem")
        .description(problem)
        .lead(icon(&oc::WARNING, 16.0).color(k.c.danger.fg))
        .on_close(|s: &mut Presenter| s.dismiss_problem())
        .build(k);
    layer.push(ToastRegion::new().toast(toast).build(k));
}

/// Three groups, left to right: the clip (its name and how to open another), where the picture
/// goes, and the window's own settings. The gaps inside a group are tighter than the gaps
/// between them, so each group reads as one thing.
fn header(s: &Presenter, k: &Kit, layer: &mut Vec<El>) -> El {
    row()
        .h(36.0)
        .gap(12.0)
        .center_y()
        .w_full()
        .child(clip_group(s, k))
        .child(spacer())
        .child(presenting_group(s, k, layer))
        .child(
            IconButton::new(&oc::SETTINGS_GEAR)
                .control_size(ControlSize::Large)
                .help("Settings")
                .on_press(|s: &mut Presenter| s.overlay = Overlay::Settings)
                .build(k),
        )
}

/// What is open, and the button that opens something else in its place.
fn clip_group(s: &Presenter, k: &Kit) -> El {
    row().gap(8.0).center_y().child(title(s, k)).child(
        Button::new("Open…")
            .control_size(ControlSize::Large)
            .on_press_tx(Presenter::pick)
            .build(k),
    )
}

/// Everything about the presentation, side by side: where the picture goes, how it fills that
/// display, and the switch that sends it there. None of it lives down in the transport, because
/// none of it is about the clip.
fn presenting_group(s: &Presenter, k: &Kit, layer: &mut Vec<El>) -> El {
    row()
        .gap(6.0)
        .center_y()
        .child(display_trigger(s, k, layer))
        .child(fit_trigger(s, k, layer))
        .child(present_button(s, k))
}

/// The display picker: the monitor mark and the chosen display, over the listbox.
fn display_trigger(s: &Presenter, k: &Kit, layer: &mut Vec<El>) -> El {
    let selected = s.selected_display();
    let labels = s.display_labels();
    // With no display there is nothing to pick from, so the listbox never opens.
    let open = s.overlay == Overlay::DisplayPicker && !labels.is_empty();
    row()
        .key("present_on_mark")
        .gap(6.0)
        .center_y()
        .child(icon(&MONITOR, 14.0).color(k.c.icon_muted))
        .child(
            Select::new("present_on", &labels)
                .current(selected.map(|index| labels[index].as_str()))
                .placeholder("No display")
                .open(open)
                .on_toggle(|s: &mut Presenter| s.toggle_display_picker())
                .on_close(|s: &mut Presenter| s.dismiss(Overlay::DisplayPicker))
                .on_choose(|s: &mut Presenter, value: &str, _tx: &mut Tx| {
                    s.choose_display_label(value);
                })
                .build(k, layer),
        )
}

/// How the picture fills the display: Fit or Fill, chosen from a list rather than cycled with a
/// button, so the mode it will switch to is visible before it is picked.
fn fit_trigger(s: &Presenter, k: &Kit, layer: &mut Vec<El>) -> El {
    let labels: Vec<String> = FitMode::ALL
        .iter()
        .map(|mode| mode.label().to_string())
        .collect();
    row().key("fit_mark").gap(6.0).center_y().child(
        Select::new("fit", &labels)
            .current(Some(s.cfg.fit.label()))
            .open(s.overlay == Overlay::FitPicker)
            .on_toggle(|s: &mut Presenter| s.toggle_fit_picker())
            .on_close(|s: &mut Presenter| s.dismiss(Overlay::FitPicker))
            .on_choose(|s: &mut Presenter, value: &str, _tx: &mut Tx| {
                s.choose_fit_label(value);
            })
            .build(k, layer),
    )
}

fn title(s: &Presenter, k: &Kit) -> El {
    let mut title = row().gap(4.0).center_y();
    if let Some(player) = s.clip.player() {
        title = title.child(text(player.info.title()).font(k.ty.ui).color(k.c.text_base));
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
    title
}

fn present_button(s: &Presenter, k: &Kit) -> El {
    if s.projection.is_presenting() {
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
    }
}

/// The picture, or what to do to get one. With a clip, clicking it plays or pauses. With none, the
/// whole area is a drop zone that also opens the file picker when clicked.
fn stage(s: &Presenter, k: &Kit) -> El {
    let stage = el()
        .key("stage")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .radius(k.radius.xl);
    match s.clip.player() {
        Some(player) if s.cfg.preview => stage
            .clip_rounded(k.radius.xl)
            .fill(Color::BLACK)
            .child(
                el().key("preview")
                    .w_full()
                    .h_full()
                    .native_surface(player.preview_surface(), s.cfg.fit.surface()),
            )
            .on_press(Presenter::toggle_playing),
        Some(_) => stage
            .fill(Color::BLACK)
            .center()
            .child(
                text("Preview is off")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_muted),
            )
            .on_press(Presenter::toggle_playing),
        None => stage
            .fill(k.c.bg_deep)
            .border(1.0, k.c.border_muted)
            .border_dash(7.0, 6.0)
            // Between the well's own shade and the window's, so aiming at the drop zone lifts it
            // without turning it into a different surface.
            .hover(k.c.bg_deep.mix(k.c.bg_base, 0.5))
            .press(k.c.pressed)
            .center()
            .child(empty_stage(k))
            .on_press_tx(Presenter::pick),
    }
}

/// What to do when there is nothing to show: a mark and a single hint, short enough that the
/// dashed frame reads as the thing to aim at.
fn empty_stage(k: &Kit) -> El {
    col()
        .key("empty-stage")
        .gap(10.0)
        .center()
        .child(
            el().size(56.0, 56.0)
                .radius(28.0)
                .fill(k.c.layer_02)
                .border(1.0, k.c.border_base)
                .center()
                .child(icon(&FILM, 26.0).color(k.c.icon_muted)),
        )
        .child(
            text("Drop a video file here")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        )
        .child(
            text("Click to choose a file")
                .font(k.ty.caption())
                .color(k.c.text_muted),
        )
}

fn scrub_row(s: &Presenter, k: &Kit) -> El {
    let (time, duration, progress) = match s.clip.player() {
        Some(player) => (player.time(), player.duration(), player.progress()),
        None => (0.0, 0.0, 0.0),
    };
    let shown_time = s.scrub.map(|fraction| fraction * duration).unwrap_or(time);
    row()
        .gap(10.0)
        .center_y()
        .w_full()
        .child(text(clock(shown_time)).font(k.ty.mono).color(k.c.text_base))
        .child(seek_bar(k, s, duration, progress))
        .child(text(clock(duration)).font(k.ty.mono).color(k.c.text_muted))
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
                    if let Some(player) = s.clip.player() {
                        player.seek_fraction(fraction);
                    }
                }
                Phase::Move => s.scrub = Some(fraction),
            }
        })
}

/// Play, and the toggle that only matters while it plays. Sound is not here: the volume section
/// owns it, with a fader and a MUTE, so the transport does not offer it twice.
fn transport(s: &Presenter, k: &Kit) -> El {
    let playing = s.clip.player().is_some_and(|player| player.is_playing());
    row()
        .gap(8.0)
        .center_y()
        .w_full()
        .child(
            Button::new(if playing { "Pause" } else { "Play" })
                .control_size(ControlSize::Large)
                .on_press(Presenter::toggle_playing)
                .build(k),
        )
        .child(spacer())
        .child(toggle_button(
            k,
            s.cfg.looping,
            (&LOOP, &LOOP_OFF),
            ("Loop on", "Loop off"),
            Presenter::toggle_loop,
        ))
}

/// A player-style toggle: the glyph, the brightness and the tooltip all follow one flag, so an
/// option that is off never looks like one that is on.
fn toggle_button(
    k: &Kit,
    on: bool,
    icons: (&'static Icon, &'static Icon),
    help: (&'static str, &'static str),
    press: fn(&mut Presenter),
) -> El {
    IconButton::new(if on { icons.0 } else { icons.1 })
        .variant(if on {
            IconButtonVariant::Ghost
        } else {
            IconButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Large)
        .help(if on { help.0 } else { help.1 })
        .on_press(press)
        .build(k)
}
