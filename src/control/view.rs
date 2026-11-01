//! What the control window draws: the header, the stage, the seek bar and the transport.

use super::{Overlay, Presenter};
use rune::{Color, Cx, Drag, El, Icon, Phase, Tx, col, el, icon, row, spacer, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Progress, Select,
};
use rune_theme_opencode::icons as oc;

/// The marks OpenCode's icon catalog has no glyph for — the display picker's monitor, and loop and
/// sound in both their states — drawn on the catalog's own grid: 16pt, 1pt stroke.
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

pub fn presenter_view(s: &Presenter, cx: &mut Cx<Presenter>) -> El {
    let k = rune_theme_opencode::kit(cx.system_dark());
    let (time, duration, progress, playing) = match s.clip.player() {
        Some(player) => (
            player.time(),
            player.duration(),
            player.progress(),
            player.is_playing(),
        ),
        None => (0.0, 0.0, 0.0, false),
    };
    let shown_time = s.scrub.map(|fraction| fraction * duration).unwrap_or(time);

    // The clip's own details (size, frame rate, audio) hang off an info icon beside the title.
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

    let selected = s.selected_display();
    let labels = s.display_labels();
    // With no display there is nothing to pick from, so the listbox never opens.
    let open = s.overlay == Overlay::DisplayPicker && !labels.is_empty();
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
                        s.overlay = if s.overlay == Overlay::DisplayPicker {
                            Overlay::None
                        } else {
                            Overlay::DisplayPicker
                        };
                    }
                })
                .on_close(|s: &mut Presenter| {
                    if s.overlay == Overlay::DisplayPicker {
                        s.overlay = Overlay::None;
                    }
                })
                .on_choose(|s: &mut Presenter, value: &str, _tx: &mut Tx| {
                    s.choose_display_label(value);
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
                .on_press(|s: &mut Presenter| s.overlay = Overlay::Settings)
                .build(k),
        )
        .child(
            Button::new("Open…")
                .control_size(ControlSize::Large)
                .on_press_tx(Presenter::pick)
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

    let stage = match s.clip.player() {
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
            .on_press(Presenter::toggle_playing),
        _ => el()
            .key("stage")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .radius(k.radius.xl)
            .fill(Color::BLACK)
            .center()
            .child(
                text(if s.clip.player().is_some() {
                    "Preview is off"
                } else {
                    "Drop a video file here"
                })
                .font(k.ty.ui_medium)
                .color(k.c.text_muted),
            )
            .on_press(Presenter::toggle_playing),
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
        );

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
    // Errors are the only standing text: everything else is a control or the picture.
    if let Some(problem) = s.problem() {
        root = root.child(
            text(problem)
                .font(k.ty.text_xsmall_medium)
                .color(k.c.danger.fg),
        );
    }
    super::settings_popup::settings_window(s, k, &mut layer);
    rune_kit::root(root, layer)
}
