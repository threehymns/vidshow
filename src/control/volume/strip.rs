//! The strip itself: the key, the value field, the three columns below them, and the paint every
//! raised or cut-in part is made of.
//!
//! There is no panel under the strip: only the objects carry a surface. Raised parts (the cap,
//! MUTE) get a dark bottom, a lit top and a tight drop shadow; cut-in parts (the LCD value, the
//! rail, the meter tracks) are a shade darker than the window, with the shadow along their top
//! edge and a line of light along the bottom.

use super::fader::fader;
use super::meters::{Meters, stereo_meters};
use super::scale::{scale_marks, value_text};
use crate::control::Presenter;
use rune::{Color, El, Shadow, TextStyle, col, row, spacer, text};
use rune_kit::{Button, ControlSize, Kit};

/// The section's height. The width is not a number anyone picks: it is added up from the parts.
pub(super) const STRIP_H: f32 = 236.0;
const PAD: f32 = 8.0;
const GAP: f32 = 3.0;
/// Wide enough for `−60` plus its tick, and no wider, so the numbers sit against the meters.
const SCALE_W: f32 = 22.0;
/// The strip's width: the columns decide it, now that the key sits over the value instead of
/// beside it. The header narrows to the button and the row's spacer takes what is left.
const STRIP_W: f32 = 70.0;
/// How far the fader column sits in from the strip's edge, so the cap is not flush against the
/// window's border.
const INSET: f32 = 5.0;

/// The section's own slot. It has no surface of its own: everything visible inside it is an object
/// on the window's background, and its width is the width of the work inside it.
pub(super) fn strip(s: &Presenter, k: &Kit, m: Meters) -> El {
    col()
        .key("volume-strip")
        .w(STRIP_W)
        .h(STRIP_H)
        .min_h(0.0)
        // The key sits over the value. Stacked, the header is only as wide as the button, which
        // is what lets the columns below sit close together instead of being spread apart by a
        // wide header they have nothing to align with.
        .child(
            col()
                .w_full()
                .items_start()
                .gap(GAP)
                .child(mute_button(k, s.cfg.muted))
                .child(value_field(k, s)),
        )
        .child(channels(k, s, m))
}

/// The section's three channels, in the order they are read: the fader, the numbered scale, then
/// the stereo meters. The scale is read against the fader, so it sits straight beside it; the
/// slack the wide header leaves goes between the scale and the meters, which keeps the meters
/// under the value field.
fn channels(k: &Kit, s: &Presenter, m: Meters) -> El {
    row()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .py(PAD)
        .pl(INSET)
        .gap(GAP)
        .child(fader(s, k))
        .child(scale_marks(k, SCALE_W))
        .child(spacer())
        .child(stereo_meters(k, m))
}

/// A field: a sunken well holding the value, as wide as the strip so it lines up with the key
/// above it, with the reading centred in it.
fn value_field(k: &Kit, s: &Presenter) -> El {
    well(
        k,
        row()
            .w_full()
            .h(18.0)
            .px(2.5)
            .center_y()
            .justify_center()
            .border(1.0, k.c.border_muted),
        k.radius.xs,
    )
    .child(digits(k, s, 9.5))
    .child(label(k, "dB", 6.0, 0.4, k.c.text_faint).ml(1.0))
}

/// The value in the mono face, reading faint while the output is muted.
fn digits(k: &Kit, s: &Presenter, size: f32) -> El {
    text(value_text(s))
        .font(TextStyle { size, ..k.ty.mono })
        .color(if s.cfg.muted {
            k.c.text_faint
        } else {
            k.c.text_base
        })
}

/// The mute key: the kit's own button, one size down from the transport's Play, so the strip
/// carries the app's controls rather than a one-off moulding. Muting leaves the button as it is
/// and changes only what its surface is made of — the same neutral shape, ink and weight under a
/// danger wash, its border and shadow carrying the same tint.
fn mute_button(k: &Kit, on: bool) -> El {
    // Full width, so the key spans the strip like a console key fills its channel.
    let button = Button::new("MUTE")
        .control_size(ControlSize::Small)
        .on_press(Presenter::toggle_mute)
        .build(k)
        .w_full();
    if !on {
        return button;
    }
    button
        .fill(k.c.danger.bg)
        .border(1.0, k.c.danger.border)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.5,
            blur: 4.0,
            spread: -0.5,
            color: k.c.danger.fg.alpha(if k.dark { 0.35 } else { 0.22 }),
            inset: false,
        })
}

/// A small caption in the kit's own voice, for the scale's letters and the field's unit.
pub(super) fn label(k: &Kit, text_label: &str, size: f32, tracking: f32, color: Color) -> El {
    text(text_label)
        .font(TextStyle {
            size,
            tracking,
            ..k.ty.small
        })
        .color(color)
}

/// How anything cut into the window is painted: a shade darker than the window it is sunk into,
/// never a lighter panel. On a flat surface a lighter fill reads as a raised plate, not a hole;
/// what says "cut in" is the shadow along the top edge and the line of light along the bottom.
pub(super) fn recess(k: &Kit) -> Color {
    k.c.black.alpha(if k.dark { 0.42 } else { 0.06 })
}

/// A highlight on the strip, in the strength the mode needs: white reads on a dark surface with a
/// light touch and needs more over a light one, the way the kit's own surfaces are lit.
pub(super) fn light(k: &Kit, amount: f32) -> Color {
    k.c.white.alpha(if k.dark { amount } else { amount * 1.6 })
}

/// The shadow that goes with [`light`], and needs less of it on a light surface to stay a shadow.
pub(super) fn shade(k: &Kit, amount: f32) -> Color {
    k.c.black.alpha(if k.dark { amount } else { amount * 0.5 })
}

/// Something cut into the window: its fill darkens toward its top edge, the shadow falls inside
/// that edge, and a hairline of light sits on the bottom one — the same recipe as the cap's rim,
/// on the other side of the surface. Every recess on the strip is painted through this, so the
/// value field, the rail and the meter tracks are one material.
pub(super) fn well(k: &Kit, surface: El, radius: f32) -> El {
    surface
        .radius(radius)
        .continuous_corners()
        .fill(recess(k))
        .gradient(shade(k, 0.3), recess(k))
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: shade(k, 0.45),
            inset: true,
        })
        .box_shadow(Shadow {
            x: 0.0,
            y: -1.0,
            blur: 0.0,
            spread: 0.0,
            color: light(k, 0.05),
            inset: true,
        })
}
