//! The fader cap: the one object on the strip built as a lit solid rather than a tinted plate.
//!
//! A fillet catches the light, a body rolls off through a mid-tone, a skirt falls into shadow, and
//! a face is dished toward its middle the way a real cap is. Its material is a lighting profile
//! rather than a colour — every value is derived from the window's own background, so the cap
//! keeps its shape whatever the theme does to the tokens.

use super::strip::{light, shade};
use rune::{Color, El, Shadow, el};
use rune_kit::Kit;

/// The cap, which is the widest part of the fader, so the strip is built around it.
pub(super) const CAP_W: f32 = 18.0;
pub(super) const CAP_H: f32 = 28.0;
/// The cap's corners, the rim between it and the face sunk into it, and the skirt that shows below
/// that face. The face's radius is the cap's less the rim, so the corners nest, and its height is
/// the cap less the rim and the skirt.
const CAP_RADIUS: f32 = 3.5;
const CAP_RIM: f32 = 1.0;
const CAP_SKIRT: f32 = 2.0;
const FACE_RADIUS: f32 = CAP_RADIUS - CAP_RIM;
/// The scoop cut down into the face: how far its rectangle sits in from the face, and its corners.
/// It runs the full width of the face, so the only lip left round it is above and below — a lip
/// down each side is narrow enough to read as a ridge standing beside the scoop.
const SCOOP_IN_X: f32 = 0.0;
const SCOOP_IN_Y: f32 = 3.0;
const SCOOP_RADIUS: f32 = FACE_RADIUS;

/// The cap's material as a lighting profile rather than a tint. A moulded plastic or machined
/// metal bar has a top fillet that catches the light, a body that rolls off through a mid-tone,
/// and a skirt that falls to the darkest value under the face; the face sunk into it is lit from
/// the same side, its top edge inside the rim's shadow and its bottom edge taking the bounce. Each
/// of those is a number here and each is branched by mode, like the kit's own surfaces, so the cap
/// is an object in light in both themes.
struct CapMaterial {
    /// The body's profile, top to bottom.
    fillet: Color,
    body: Color,
    waist: Color,
    skirt: Color,
    /// The face's surface round the scoop: the lip along its top edge and its body.
    face_lip: Color,
    face_body: Color,
    /// The scoop, top to bottom: its upper wall in shadow, its floor, and the wall that climbs
    /// back into the light below.
    scoop_top: Color,
    scoop_mid: Color,
    scoop_floor: Color,
    scoop_bottom: Color,
    /// The seam round the silhouette: a dark hairline, with the light caught just inside its top
    /// edge and lost just inside its bottom one.
    rim_edge: Color,
    /// How hard the two sides roll away from the light, and the sheen the fillet throws into the
    /// upper third.
    roll: f32,
    sheen: f32,
    /// The shadow where the cap meets the rail, and the soft one it casts at rest.
    contact: f32,
    cast: f32,
}

impl CapMaterial {
    fn new(k: &Kit) -> Self {
        // One grey in both themes, because plastic is not a theme colour, and derived from the
        // window instead of a token: a little lighter than the window in dark, a good deal darker
        // than it in light. Every value below is that grey moved toward black or white, so the
        // cap keeps its shape whatever the theme does to the tokens.
        let base = if k.dark {
            k.c.bg_base.mix(k.c.white, 0.20)
        } else {
            k.c.bg_base.mix(k.c.black, 0.30)
        };
        Self {
            fillet: base.mix(k.c.white, 0.34),
            body: base.mix(k.c.white, 0.10),
            waist: base,
            skirt: base.mix(k.c.black, 0.50),
            face_lip: base.mix(k.c.white, 0.62),
            face_body: base.mix(k.c.white, 0.16),
            scoop_top: base.mix(k.c.black, 0.60),
            scoop_mid: base.mix(k.c.black, 0.46),
            scoop_floor: base.mix(k.c.black, 0.38),
            scoop_bottom: base.mix(k.c.white, 0.30),
            rim_edge: k.c.black.alpha(if k.dark { 0.5 } else { 0.28 }),
            roll: if k.dark { 0.38 } else { 0.22 },
            sheen: if k.dark { 0.22 } else { 0.34 },
            contact: if k.dark { 0.6 } else { 0.24 },
            cast: if k.dark { 0.3 } else { 0.12 },
        }
    }
}

/// A fader cap with the profile of a real one: a face sunk into a cap whose skirt shows below it,
/// and a contact shadow tight against the rail under the soft one it casts, so it stands on the
/// rail rather than floating over it.
///
/// The body is one gradient down its height, sampled from a lit bar rather than from two ends: the
/// edge turning away, the fillet's bright band, the body, the waist where the section turns from
/// the light, and the skirt falling into the shadow the rail puts on it. The face is a dish: a
/// scoop cut down into its middle — a lip of face all round, then the recess with its upper wall
/// in shadow and its lower wall climbing back toward the light — with a bloom pooled in it. The
/// rim is a dark seam with the light caught just inside its top edge, not a bright stroke.
pub(super) fn cap_face(k: &Kit) -> El {
    let m = CapMaterial::new(k);
    let (face_w, face_h) = (CAP_W - 2.0 * CAP_RIM, CAP_H - CAP_RIM - CAP_SKIRT);
    let cap = el()
        .w(CAP_W)
        .h(CAP_H)
        .radius(CAP_RADIUS)
        .continuous_corners()
        .fill(m.waist)
        .linear_gradient(
            std::f32::consts::PI,
            [
                (0.00, m.fillet.mix(k.c.black, 0.30)),
                (0.09, m.fillet),
                (0.26, m.body),
                (0.52, m.waist),
                (0.78, m.waist.mix(m.skirt, 0.55)),
                (0.93, m.skirt),
                (1.00, m.skirt.mix(k.c.black, 0.30)),
            ],
        )
        // A seam, not a stroke: a dark hairline round the silhouette with the light caught just
        // inside its top edge and lost just inside its bottom one. A bright border all the way
        // round reads as a sticker at this size.
        .border(1.0, m.rim_edge)
        .box_shadow(Shadow {
            x: 0.0,
            y: 0.5,
            blur: 0.5,
            spread: 0.0,
            color: light(k, 0.14),
            inset: true,
        })
        .box_shadow(Shadow {
            x: 0.0,
            y: -0.5,
            blur: 0.5,
            spread: 0.0,
            color: shade(k, 0.30),
            inset: true,
        })
        // Tight against the skirt, then the soft shadow it lays on the rail, then the haze that
        // seats it in the strip: three scales of the same occlusion.
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 1.0,
            spread: 0.0,
            color: k.c.black.alpha(m.contact),
            inset: false,
        })
        .box_shadow(Shadow {
            x: 0.0,
            y: CAP_SKIRT + 1.5,
            blur: 5.0,
            spread: -0.5,
            color: k.c.black.alpha(m.cast),
            inset: false,
        })
        .box_shadow(Shadow {
            x: 0.0,
            y: 0.0,
            blur: 3.0,
            spread: 0.0,
            color: k.c.black.alpha(m.cast * 0.45),
            inset: false,
        });
    // The face: a lit lip all round, with the scoop cut down into its middle. The scoop's own
    // boundary is what reads as the centre curved down — a soft vertical gradient alone is a
    // pillow, and a band of knurling across the face is a barcode.
    let (scoop_w, scoop_h) = (face_w - 2.0 * SCOOP_IN_X, face_h - 2.0 * SCOOP_IN_Y);
    let face = el()
        .absolute(CAP_RIM, CAP_RIM)
        .w(face_w)
        .h(face_h)
        .radius(FACE_RADIUS)
        .continuous_corners()
        .fill(m.face_body)
        .linear_gradient(
            std::f32::consts::PI,
            [
                (0.00, m.face_lip),
                (0.08, m.face_body),
                (0.82, m.face_body),
                (1.00, m.face_body.mix(k.c.black, 0.18)),
            ],
        )
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 1.5,
            spread: 0.0,
            color: shade(k, 0.45),
            inset: true,
        })
        .box_shadow(Shadow {
            x: 0.0,
            y: -1.0,
            blur: 0.0,
            spread: 0.0,
            color: light(k, 0.10),
            inset: true,
        })
        // The scoop itself: a rounded recess, so it reads as moulded rather than routed. Its
        // upper wall is in shadow, its floor is the deepest value, and its lower wall climbs back
        // toward the light that falls into it.
        .child(
            el().absolute(SCOOP_IN_X, SCOOP_IN_Y)
                .w(scoop_w)
                .h(scoop_h)
                .radius(SCOOP_RADIUS)
                .continuous_corners()
                .fill(m.scoop_floor)
                .linear_gradient(
                    std::f32::consts::PI,
                    [
                        (0.00, m.scoop_top),
                        (0.28, m.scoop_mid),
                        (0.64, m.scoop_floor),
                        (1.00, m.scoop_bottom),
                    ],
                )
                // The crease under its top lip, and the light gathering along its lower one.
                .box_shadow(Shadow {
                    x: 0.0,
                    y: 1.0,
                    blur: 1.2,
                    spread: 0.0,
                    color: shade(k, 0.5),
                    inset: true,
                })
                .box_shadow(Shadow {
                    x: 0.0,
                    y: -0.5,
                    blur: 0.5,
                    spread: 0.0,
                    color: light(k, 0.12),
                    inset: true,
                })
                // The scoop's sides roll away too, so its edges are not a cut-out rectangle.
                .box_shadow(Shadow {
                    x: 1.0,
                    y: 0.0,
                    blur: 2.0,
                    spread: 0.0,
                    color: shade(k, 0.26),
                    inset: true,
                })
                .box_shadow(Shadow {
                    x: -1.0,
                    y: 0.0,
                    blur: 2.0,
                    spread: 0.0,
                    color: shade(k, 0.26),
                    inset: true,
                }),
        );
    let cap = cap.child(face);
    // Only the silhouette: both sides of the bar turn away from the light. The face's own curve
    // is the dish above, so nothing here draws a line down the middle of the cap.
    let cap = cap.child(
        el().absolute(0.0, 0.0)
            .w(CAP_W)
            .h(CAP_H)
            .radius(CAP_RADIUS)
            .continuous_corners()
            .linear_gradient(
                std::f32::consts::FRAC_PI_2,
                [
                    (0.00, k.c.black.alpha(m.roll)),
                    (0.07, k.c.black.alpha(m.roll * 0.25)),
                    (0.16, k.c.black.alpha(0.0)),
                    (0.84, k.c.black.alpha(0.0)),
                    (1.00, k.c.black.alpha(m.roll * 1.05)),
                ],
            ),
    );
    // The specular bloom, pooled on the lower wall of the bowl where it turns up into the light
    // rather than sitting on a ridge: a highlight in the bottom of the dish, off to the left.
    cap.child(
        el().absolute(0.0, 0.0)
            .w(CAP_W)
            .h(CAP_H)
            .radius(CAP_RADIUS)
            .continuous_corners()
            .radial_gradient(
                [0.50, 0.64],
                [CAP_W * 0.26, CAP_H * 0.11],
                [
                    (0.0, k.c.white.alpha(m.sheen)),
                    (0.45, k.c.white.alpha(m.sheen * 0.35)),
                    (1.0, k.c.white.alpha(0.0)),
                ],
            ),
    )
}
