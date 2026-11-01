//! The keyboard in the projector's window: the transport, and closing it.
//!
//! The control window owns the clip, the settings and the saved state, so a transport action asked
//! for here is carried out there. Otherwise a loop or mute toggled on the projector would not show
//! up on its controls and would not be remembered.

use super::{Event, Projection};
use crate::keybinds::Action;
use rune::{KeyEvent, Tx};

pub fn projection_key(p: &mut Projection, event: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(action) = p.keybinds.action_for(event) else {
        return false;
    };
    if action == Action::Close {
        tx.close_window();
        return true;
    }
    if !moves_playback(action) {
        return false;
    }
    (p.notify)(Event::Action(action));
    true
}

/// The actions a window that only shows the picture offers.
fn moves_playback(action: Action) -> bool {
    matches!(
        action,
        Action::PlayPause
            | Action::Mute
            | Action::Loop
            | Action::Restart
            | Action::SeekBack
            | Action::SeekBackFast
            | Action::SeekForward
            | Action::SeekForwardFast
    )
}
