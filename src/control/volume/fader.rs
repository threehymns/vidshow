//! The fader: the law its travel follows, the cap riding it, and the rail under the cap.
//!
//! The law is what makes the strip readable: travel is linear in dB, so the numbered marks, the
//! cap and the readout above are all the same scale, and the top of the rail is a short boost
//! band above unity rather than a cliff.

use super::cap::{CAP_H, CAP_W, cap_face};
use super::scale::{DB_FLOOR, MAX_DB};
use super::strip::{light, recess, shade};
use crate::control::Presenter;
use crate::settings::MAX_VOLUME;
use rune::{Drag, El, Phase, Shadow, Tx, col, el, spacer};
use rune_kit::Kit;

/// The rail the cap rides: a real slot, wide enough for its round section to show — a dark wall,
/// a floor and a lit wall — instead of a hairline that can only read as a line.
const SLOT_W: f32 = 6.0;

/// The fader's law: travel, 0 at the top of the rail, to gain. It is linear in dB from the top of
/// the boost band down to the floor of the scale, with silence at the very bottom — so the printed
/// marks, the cap and the readout above are one scale, and an equal pull is an equal change in dB
/// the whole way down.
pub(super) fn gain_at(along: f32) -> f32 {
    let along = along.clamp(0.0, 1.0);
    gain_of_db(MAX_DB * (1.0 - along) - DB_FLOOR * along)
}

/// The same law read the other way: where the cap sits for a gain.
pub(super) fn along_of(gain: f32) -> f32 {
    if gain <= 0.0 {
        return 1.0;
    }
    ((MAX_DB - 20.0 * gain.log10()) / (MAX_DB + DB_FLOOR)).clamp(0.0, 1.0)
}

/// One keypress of `db`, from a gain. Silence is read as the floor of the scale, so stepping up
/// from nothing starts at the bottom of the fader instead of crawling out of the mud.
pub fn stepped(gain: f32, db: f32) -> f32 {
    let from = if gain > 0.0 {
        20.0 * gain.log10()
    } else {
        -DB_FLOOR
    };
    gain_of_db(from + db)
}

/// The gain a dB reading asks for, inside what the fader can reach: nothing below the floor of the
/// scale, and no more than the boost at the top.
fn gain_of_db(db: f32) -> f32 {
    if db <= -DB_FLOOR {
        return 0.0;
    }
    10f32.powf(db / 20.0).min(MAX_VOLUME)
}

/// The fader: a single rail with a real cap riding it. There is no channel around the rail — the
/// cap is the widest part, so the fader is as wide as the cap and the rail is centred under it.
/// The travel is split with two growing spacers, so the cap sits at the right fraction of whatever
/// height the section is given, and a drag anywhere in the fader moves it there. A double-click
/// puts it back on 0 dB, the way it does on a fader in a console or a DAW.
pub(super) fn fader(s: &Presenter, k: &Kit) -> El {
    // Where the cap sits, which is the same reading the field above shows, in the same units.
    let along = along_of(s.cfg.volume);
    el().key("volume-fader")
        .w(CAP_W)
        .h_full()
        .child(rail(k))
        .child(
            col()
                .w_full()
                .h_full()
                .py(3.0)
                .items_center()
                .child(spacer().grow(along.max(0.001)))
                .child(cap_face(k))
                .child(spacer().grow((1.0 - along).max(0.001))),
        )
        .on_drag(move |s: &mut Presenter, drag: &mut Drag, _tx: &mut Tx| {
            if drag.phase == Phase::Up {
                return;
            }
            // The cap travels the rail less its own height, so the pointer tracks the cap's centre
            // and not a point that runs ahead of it near the ends.
            let travel = (drag.h - 6.0 - CAP_H).max(1.0);
            let reach = (drag.y - 3.0 - CAP_H * 0.5).clamp(0.0, travel);
            s.set_volume(gain_at(reach / travel));
        })
        .on_double_press(Presenter::reset_volume)
}

/// The rail the cap rides on: a slot milled into the plate. Its section is round, so it is darkest
/// along the left wall and picks the light up along the right one, with the recess's shadow along
/// the top edge and a hairline of light on the bottom lip — a metal slot rather than a dark line.
fn rail(k: &Kit) -> El {
    el().key("volume-rail")
        .absolute((CAP_W - SLOT_W) * 0.5, 0.0)
        .w(SLOT_W)
        .h_full()
        .radius(SLOT_W * 0.25)
        .continuous_corners()
        .fill(recess(k))
        .linear_gradient(
            std::f32::consts::FRAC_PI_2,
            [
                (0.00, light(k, 0.03)),
                (0.10, shade(k, 0.38)),
                (0.34, shade(k, 0.20)),
                (0.58, recess(k)),
                (0.80, light(k, 0.10)),
                (0.92, light(k, 0.20)),
                (1.00, shade(k, 0.34)),
            ],
        )
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: shade(k, 0.5),
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
