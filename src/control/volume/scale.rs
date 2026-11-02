//! The dB numbers beside the fader: the marks, their ticks, the readout's text, and the level
//! arithmetic the meters are drawn from.
//!
//! The marks are the fader's own scale — its travel is linear in dB, so a mark is printed where
//! the cap that reads that number sits, and the readout above uses the same convention.

use super::cap::CAP_H;
use crate::control::Presenter;
use rune::{El, TextStyle, col, el, spacer, text};
use rune_kit::Kit;

/// The meters measure 0 dB at the top and this at the bottom.
pub(super) const DB_FLOOR: f32 = 60.0;

/// The gain at the top of the fader's travel, in dB: `20·log10(settings::MAX_VOLUME)`. The top of
/// the fader is therefore a short boost band above unity rather than a cliff, and 0 dB sits a
/// little way down from the stop the way it does on a console.
pub(super) const MAX_DB: f32 = 6.0206;

/// The labelled marks, loudest first.
const MARKS: [i32; 7] = [6, 0, -6, -12, -24, -40, -60];

/// Printed dB numbers with their ticks, which the fader is read against. The gaps between marks
/// are the spacers' weights, so the numbers land at their real dB height whatever the section's
/// height is.
pub(super) fn scale_marks(k: &Kit, width: f32) -> El {
    // The numbers are spread over the band the cap's centre travels, not the whole column: the cap
    // is held half its height off each end, so a mark has to sit where the cap can actually read
    // it, or the scale and the cap disagree by that much everywhere.
    let inset = CAP_H * 0.5 + 3.0;
    let mut column = col()
        .key("volume-scale")
        .w(width)
        .h_full()
        .child(el().w_full().h(inset));
    for (index, mark) in MARKS.iter().enumerate() {
        let next = MARKS.get(index + 1).copied().unwrap_or(-(DB_FLOOR as i32));
        // The number and its tick hang off a line of no height, centred on that line. Laid out
        // inside the flow their own height would push every mark below it out of place, and the
        // scale would read a little wrong at every mark but the one the cap is beside.
        column = column
            .child(
                el().w_full()
                    .h(0.0)
                    .child(
                        el().absolute(0.0, -4.5).child(
                            text(mark_text(*mark))
                                .font(TextStyle {
                                    size: 8.0,
                                    ..k.ty.mono
                                })
                                .color(k.c.text_faint),
                        ),
                    )
                    .child(
                        el().absolute((width - 4.0).max(0.0), -0.5)
                            .w(4.0)
                            .h(1.0)
                            .fill(k.c.border_strong),
                    ),
            )
            .child(spacer().grow((mark - next).abs() as f32));
    }
    column.child(el().w_full().h(inset))
}

/// A scale mark as it is printed, with the same sign convention as the readout above it.
fn mark_text(mark: i32) -> String {
    match mark {
        mark if mark > 0 => format!("+{mark}"),
        0 => "0".to_string(),
        mark => format!("−{}", -mark),
    }
}

/// The volume as it is printed in the value field.
pub(super) fn value_text(s: &Presenter) -> String {
    if s.cfg.muted || s.cfg.volume <= 0.0005 {
        return "−∞".to_string();
    }
    let db = 20.0 * s.cfg.volume.log10();
    format!("{}{:.1}", if db < 0.0 { "−" } else { "+" }, db.abs())
}

/// Where a level sits on the meter's −60 dB…0 dB scale, 0 at the bottom and 1 at the top.
pub(super) fn lit_fraction(level: f32) -> f32 {
    (1.0 + db_of(level) / DB_FLOOR).clamp(0.0, 1.0)
}

/// A level in dB, floored at the bottom of the scale: silence reads as an empty meter rather than
/// as the bottom of the scale wrapped round to the top.
pub(super) fn db_of(level: f32) -> f32 {
    (20.0 * level.max(1e-6).log10()).max(-DB_FLOOR)
}
