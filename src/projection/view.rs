//! What the projector draws — the picture and nothing else — and how its window hooks up.

use super::{Event, Projection};
use rune::{Color, Cursor, Cx, El, Tx, el};

pub fn projection_view(p: &Projection, _cx: &mut Cx<Projection>) -> El {
    let mut root = el().key("projector").w_full().h_full().fill(Color::BLACK);
    if let Some(player) = &p.player {
        root = root.child(
            el().key("frame")
                .w_full()
                .h_full()
                .native_surface(player.surface(), p.fit.surface())
                .cursor(if p.hide_cursor {
                    Cursor::Hidden
                } else {
                    Cursor::Default
                }),
        );
    }
    root
}

pub fn projection_closed(p: &mut Projection, _tx: &mut Tx) {
    (p.notify)(Event::Closed);
}

pub fn projection_setup(p: &mut Projection, waker: rune::Waker) {
    p.wakers.add(waker);
}
