//! What the control window draws: the header, the stage, the seek bar and the transport.

use super::settings_popup;
use super::{Overlay, Presenter};
use rune::{Color, Cursor, Cx, Drag, El, Icon, Phase, Tx, col, el, icon, row, spacer, text};
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

pub fn presenter_view(s: &Presenter, cx: &mut Cx<Presenter>) -> El {
    let k = rune_theme_opencode::kit(cx.system_dark());
    // Panels that float above the window are collected while the rest is built.
    let mut layer = Vec::new();
    let root = col()
        .w_full()
        .h_full()
        .fill(k.c.bg_deep)
        .padding(14.0)
        .gap(10.0)
        .child(header(s, k, &mut layer))
        .child(stage(s, k))
        .child(scrub_row(s, k))
        .child(transport(s, k))
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

/// The clip's name and details, the display picker, and the window's own buttons.
fn header(s: &Presenter, k: &Kit, layer: &mut Vec<El>) -> El {
    let selected = s.selected_display();
    let labels = s.display_labels();
    // With no display there is nothing to pick from, so the listbox never opens.
    let open = s.overlay == Overlay::DisplayPicker && !labels.is_empty();
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
                .on_toggle(|s: &mut Presenter| s.toggle_display_picker())
                .on_close(|s: &mut Presenter| s.dismiss(Overlay::DisplayPicker))
                .on_choose(|s: &mut Presenter, value: &str, _tx: &mut Tx| {
                    s.choose_display_label(value);
                })
                .build(k, layer),
        );

    row()
        .h(36.0)
        .gap(10.0)
        .center_y()
        .w_full()
        .child(title(s, k))
        .child(trigger)
        .child(spacer())
        .child(
            IconButton::new(&oc::SETTINGS_GEAR)
                .control_size(ControlSize::Large)
                .help("Settings")
                .on_press(|s: &mut Presenter| s.overlay = Overlay::Settings)
                .build(k),
        )
        .child(
            Button::new("Open…")
                .control_size(ControlSize::Large)
                .on_press_tx(Presenter::pick)
                .build(k),
        )
        .child(present_button(s, k))
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
            .fill(k.c.bg_base)
            .border(1.0, k.c.border_muted)
            .border_dash(7.0, 6.0)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .cursor(Cursor::Pointer)
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

/// Play, the toggles that only matter while it plays, and the fit.
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
        .child(toggle_button(
            k,
            !s.cfg.muted,
            (&SPEAKER, &SPEAKER_MUTED),
            ("Sound on", "Sound off"),
            Presenter::toggle_mute,
        ))
        .child(
            Button::new(s.cfg.fit.label())
                .control_size(ControlSize::Large)
                .on_press(Presenter::toggle_fit)
                .build(k),
        )
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
