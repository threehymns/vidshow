//! The keyboard: what each bound chord does in each window.
//!
//! Every binding lives in [`crate::keybinds`]; these handlers only decide what an action means
//! here, so a shortcut never has to be spelled out twice.

use crate::keybinds::Action;
use crate::projection::Projection;
use crate::ui::{Presenter, pick_file};
use rune::{KeyEvent, Tx};

pub fn presenter_key(s: &mut Presenter, event: &KeyEvent, tx: &mut Tx) -> bool {
    // The settings dialog is modal: its own dismissal key, and nothing else, reaches the window.
    if s.settings_open {
        if s.keybinds.matches(Action::Close, event) {
            s.settings_open = false;
            return true;
        }
        return false;
    }
    for action in Action::ALL {
        if !s.keybinds.matches(action, event) {
            continue;
        }
        return match action {
            Action::Close => close(s),
            Action::Open => {
                pick_file(s, tx);
                true
            }
            other => {
                perform(s, other);
                true
            }
        };
    }
    false
}

fn perform(s: &mut Presenter, action: Action) {
    match action {
        Action::PlayPause => s.play_pause(),
        Action::Present => s.toggle_present(),
        Action::Mute => s.toggle_mute(),
        Action::Loop => s.toggle_loop(),
        Action::Preview => s.toggle_preview(),
        Action::Cursor => s.toggle_cursor(),
        Action::Restart => s.restart(),
        Action::SeekBack => s.seek_by(-5.0),
        Action::SeekBackFast => s.seek_by(-30.0),
        Action::SeekForward => s.seek_by(5.0),
        Action::SeekForwardFast => s.seek_by(30.0),
        Action::VolumeUp => s.change_volume(0.05),
        Action::VolumeDown => s.change_volume(-0.05),
        Action::Open | Action::Close => {}
    }
}

/// Escape closes what is on top first; only then does it stop presenting.
fn close(s: &mut Presenter) -> bool {
    if s.picker_open {
        s.picker_open = false;
        true
    } else if s.presenting {
        s.stop_presenting();
        true
    } else {
        false
    }
}

pub fn projection_key(p: &mut Projection, event: &KeyEvent, tx: &mut Tx) -> bool {
    for action in Action::ALL {
        if !p.keybinds.matches(action, event) {
            continue;
        }
        if action == Action::Close {
            tx.close_window();
            return true;
        }
        let Some(player) = p.player.clone() else {
            return false;
        };
        match action {
            Action::PlayPause => player.toggle(),
            Action::Mute => player.set_muted(!player.muted()),
            Action::Loop => player.set_looping(!player.looping()),
            Action::Restart => player.seek(0.0),
            Action::SeekBack => player.seek_by(-5.0),
            Action::SeekBackFast => player.seek_by(-30.0),
            Action::SeekForward => player.seek_by(5.0),
            Action::SeekForwardFast => player.seek_by(30.0),
            // The projector has no controls of its own beyond playback.
            _ => return false,
        }
        return true;
    }
    false
}
