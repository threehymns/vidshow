//! The keyboard in the control window.
//!
//! The chords live in [`crate::keybinds`]; every action is carried out by [`Presenter`], so a
//! shortcut never has to be spelled out twice.

use super::{Overlay, Presenter};
use crate::keybinds::Action;
use rune::{KeyEvent, Tx};

pub fn presenter_key(s: &mut Presenter, event: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(action) = s.keybinds.action_for(event) else {
        return false;
    };
    // The settings dialog is modal: its own dismissal key, and nothing else, reaches the window.
    if s.overlay == Overlay::Settings {
        if action != Action::Close {
            return false;
        }
        s.overlay = Overlay::None;
        return true;
    }
    s.perform(action, tx)
}
