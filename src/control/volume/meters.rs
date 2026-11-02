//! The strip's output meters: two tracks whose fill is coloured by level, and the clip lamp over
//! them that latches on any peak that came back past full scale.

use super::scale::{DB_FLOOR, db_of, lit_fraction};
use super::strip::{label, recess, shade, well};
use crate::control::Presenter;
use rune::{El, Shadow, col, el, row, spacer};
use rune_kit::Kit;

/// Wide enough for the two tracks, and no wider, so the meters sit under the value field.
pub(super) const METERS_W: f32 = 16.0;

/// The output's two channels, as the meters need them.
#[derive(Clone, Copy)]
pub(super) struct Meters {
    pub(super) levels: [f32; 2],
    pub(super) peaks: [f32; 2],
    pub(super) dim: f32,
    pub(super) clipped: bool,
}

/// The pair of skinny meters, with L and R under them so the two bars are told apart, and the
/// clip lamp over them.
pub(super) fn stereo_meters(k: &Kit, m: Meters) -> El {
    col()
        .key("volume-stereo")
        .w(METERS_W)
        .h_full()
        .gap(2.0)
        .child(clip_lamp(k, m.clipped))
        .child(
            row()
                .w(METERS_W)
                .grow(1.0)
                .min_h(0.0)
                .gap(2.0)
                .child(meter(k, m.levels[0], m.peaks[0], 0, 7.0, m.dim))
                .child(meter(k, m.levels[1], m.peaks[1], 1, 7.0, m.dim)),
        )
        .child(
            row()
                .w(METERS_W)
                .h(7.0)
                .justify_between()
                .child(label(k, "L", 7.0, 0.5, k.c.text_faint))
                .child(label(k, "R", 7.0, 0.5, k.c.text_faint)),
        )
}

/// The clip lamp: one window over the pair of meters, lit and latched by any peak that came back
/// over full scale. It sits above the tracks so it is seen before the bars are read, and clicking
/// it clears the latch, the way the clip lamp on a console meter bridge resets.
fn clip_lamp(k: &Kit, lit: bool) -> El {
    let lamp = el()
        .key("volume-clip")
        .role("button")
        .label("Clip")
        .w(METERS_W)
        .h(3.0)
        .radius(1.0)
        .continuous_corners()
        .fill(if lit { k.c.danger.fg } else { recess(k) });
    let lamp = if lit {
        lamp.box_shadow(Shadow {
            x: 0.0,
            y: 0.0,
            blur: 4.0,
            spread: 0.0,
            color: k.c.danger.fg.alpha(0.55),
            inset: false,
        })
    } else {
        // Unlit, it is the same cut-in window as the meter tracks, with its top edge in shadow.
        lamp.box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 1.0,
            spread: 0.0,
            color: shade(k, 0.5),
            inset: true,
        })
    };
    lamp.on_press(|s: &mut Presenter| s.volume.clear_clip())
}

/// A continuous level meter: a dark track with a fill whose colour is tied to the dB scale — green
/// to −12, amber to −3, red above — so what is lit says how loud it is. A hairline marks the peak
/// the level has reached. Every layer is pinned and laid out with growing spacers, so the meter is
/// exact at any height.
fn meter(k: &Kit, level: f32, peak: f32, slot: usize, width: f32, dim: f32) -> El {
    let db = db_of(level);
    // A muted signal is still there, just quieter on the eye.
    let shown = 1.0 - 0.7 * dim.clamp(0.0, 1.0);
    let mut bar = well(
        k,
        el().key(format!("volume-meter-{slot}")).w(width).h_full(),
        1.0,
    )
    .child(
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .opacity(shown)
            .child(colours(k, db)),
    );
    // With no peak to mark, the marker would sit on the floor of the track as a white line and
    // read as a signal that is not there.
    if peak > 0.0005 {
        let peak_at = lit_fraction(peak).clamp(0.0005, 1.0);
        bar = bar.child(
            el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(
                col()
                    .w_full()
                    .h_full()
                    .child(spacer().grow(1.0 - peak_at))
                    .child(
                        el().w_full()
                            .h(2.0)
                            .opacity(shown)
                            .fill(k.c.text_base)
                            .box_shadow(Shadow {
                                x: 0.0,
                                y: 0.0,
                                blur: 3.0,
                                spread: 0.0,
                                color: k.c.warning.fg.alpha(0.4 * (1.0 - dim)),
                                inset: false,
                            }),
                    )
                    .child(spacer().grow(peak_at)),
            ),
        );
    }
    bar
}

/// The fill in the three zones a meter is read in. Each band keeps its colour whatever the level
/// is and only the part of it under the level is painted, so the bar stays continuous.
fn colours(k: &Kit, db: f32) -> El {
    let bands = [
        (-3.0, 0.0, k.c.danger.fg),
        (-12.0, -3.0, k.c.warning.fg),
        (-DB_FLOOR, -12.0, k.c.success.fg),
    ];
    let mut fill = col().w_full().h_full();
    for (floor, ceiling, colour) in bands {
        let part = ((db - floor) / (ceiling - floor)).clamp(0.0, 1.0);
        let mut band = col().w_full().grow(ceiling - floor);
        // A band above the level has nothing to paint. Building no block at all keeps a
        // zero-height one from being drawn as a hairline on the band's edge.
        if part > 0.0 {
            let mut block = el()
                .w_full()
                .grow(part.max(0.0001))
                .fill(colour)
                .gradient(colour.mix(k.c.black, 0.2), colour.mix(k.c.white, 0.06));
            // The edge the level is at, so the top of the bar reads as a point and not a fade.
            if db > floor && db < ceiling {
                let edge = if k.dark {
                    k.c.white.alpha(0.45)
                } else {
                    k.c.black.alpha(0.2)
                };
                block = block.child(el().w_full().h(1.0).fill(edge));
            }
            band = band.child(spacer().grow(1.0 - part)).child(block);
        } else {
            band = band.child(spacer());
        }
        fill = fill.child(band);
    }
    fill
}
