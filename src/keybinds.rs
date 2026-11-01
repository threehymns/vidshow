//! Keybindings: every shortcut the app answers to, in one table.
//!
//! Adapted from sheath's `keybinds`: one [`Action`] per command, one default chord per action,
//! and a single [`Keybinds::matches`] that maps a key event back to the physical key it was
//! pressed on. The parts sheath needs for rebinding (a KDL store, recorders, conflict checks)
//! are left out, since vidshow has no rebinding UI yet.

use std::collections::BTreeMap;

use rune::{Key, KeyEvent, PhysicalKeyCode, Shortcut};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    PlayPause,
    Present,
    Mute,
    Loop,
    Preview,
    Cursor,
    Open,
    Restart,
    SeekBack,
    SeekBackFast,
    SeekForward,
    SeekForwardFast,
    VolumeUp,
    VolumeDown,
    Close,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::PlayPause,
        Action::Present,
        Action::Mute,
        Action::Loop,
        Action::Preview,
        Action::Cursor,
        Action::Open,
        Action::Restart,
        Action::SeekBack,
        Action::SeekBackFast,
        Action::SeekForward,
        Action::SeekForwardFast,
        Action::VolumeUp,
        Action::VolumeDown,
        Action::Close,
    ];

    /// Chords that must match with or without Shift held, because Shift is part of the keystroke
    /// rather than a modifier the action cares about.
    fn ignores_shift(self) -> bool {
        false
    }

    fn default_chord(self) -> &'static str {
        match self {
            Action::PlayPause => "space",
            Action::Present => "f",
            Action::Mute => "m",
            Action::Loop => "l",
            Action::Preview => "v",
            Action::Cursor => "c",
            Action::Open => {
                if cfg!(target_os = "macos") {
                    "super+o"
                } else {
                    "ctrl+o"
                }
            }
            Action::Restart => "home",
            Action::SeekBack => "left",
            Action::SeekBackFast => "shift+left",
            Action::SeekForward => "right",
            Action::SeekForwardFast => "shift+right",
            Action::VolumeUp => "up",
            Action::VolumeDown => "down",
            Action::Close => "escape",
        }
    }
}

pub struct Keybinds {
    chords: BTreeMap<Action, Shortcut>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Keybinds {
    pub fn defaults() -> Keybinds {
        Keybinds {
            chords: default_chords(),
        }
    }

    fn chord(&self, action: Action) -> Option<&Shortcut> {
        self.chords.get(&action)
    }

    pub fn matches(&self, action: Action, event: &KeyEvent) -> bool {
        self.chord(action)
            .is_some_and(|chord| matches(chord, event, action.ignores_shift()))
    }
}

fn default_chords() -> BTreeMap<Action, Shortcut> {
    let mut chords = BTreeMap::new();
    for action in Action::ALL {
        if let Some(chord) = default_shortcut(action) {
            chords.insert(action, chord);
        }
    }
    chords
}

fn default_shortcut(action: Action) -> Option<Shortcut> {
    let text = action.default_chord();
    match text.parse::<Shortcut>() {
        Ok(chord) => Some(chord),
        Err(_) => {
            debug_assert!(false, "{action:?} has an invalid default chord: {text}");
            None
        }
    }
}

fn matches(chord: &Shortcut, event: &KeyEvent, ignore_shift: bool) -> bool {
    let want = chord.modifiers();
    let held = event.mods;
    let logo = cfg!(target_os = "macos") || want.logo == held.logo;
    if (want.shift != held.shift && !ignore_shift)
        || want.alt != held.alt
        || want.cmd != held.cmd
        || want.ctrl != held.ctrl
        || !logo
    {
        return false;
    }
    physical(&event.key) == Some(chord.physical_key())
}

// Rune matches accelerators by physical position, so the logical key an event carries is mapped
// back to it.
fn physical(key: &Key) -> Option<PhysicalKeyCode> {
    use PhysicalKeyCode as Code;
    Some(match key {
        Key::Command(c) => char_key(*c)?,
        Key::Text(text) => {
            let mut chars = text.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            char_key(c)?
        }
        Key::Enter => Code::Enter,
        Key::Backspace => Code::Backspace,
        Key::Delete => Code::Delete,
        Key::Left => Code::ArrowLeft,
        Key::Right => Code::ArrowRight,
        Key::Up => Code::ArrowUp,
        Key::Down => Code::ArrowDown,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Escape => Code::Escape,
        Key::Tab => Code::Tab,
        Key::Function(n) => function_key(*n)?,
        _ => return None,
    })
}

fn char_key(c: char) -> Option<PhysicalKeyCode> {
    use PhysicalKeyCode as Code;
    Some(match c.to_ascii_lowercase() {
        'a' => Code::KeyA,
        'b' => Code::KeyB,
        'c' => Code::KeyC,
        'd' => Code::KeyD,
        'e' => Code::KeyE,
        'f' => Code::KeyF,
        'g' => Code::KeyG,
        'h' => Code::KeyH,
        'i' => Code::KeyI,
        'j' => Code::KeyJ,
        'k' => Code::KeyK,
        'l' => Code::KeyL,
        'm' => Code::KeyM,
        'n' => Code::KeyN,
        'o' => Code::KeyO,
        'p' => Code::KeyP,
        'q' => Code::KeyQ,
        'r' => Code::KeyR,
        's' => Code::KeyS,
        't' => Code::KeyT,
        'u' => Code::KeyU,
        'v' => Code::KeyV,
        'w' => Code::KeyW,
        'x' => Code::KeyX,
        'y' => Code::KeyY,
        'z' => Code::KeyZ,
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        '9' => Code::Digit9,
        '-' => Code::Minus,
        '=' => Code::Equal,
        ',' => Code::Comma,
        '.' => Code::Period,
        '/' => Code::Slash,
        ';' => Code::Semicolon,
        '\'' => Code::Quote,
        '[' => Code::BracketLeft,
        ']' => Code::BracketRight,
        '\\' => Code::Backslash,
        '`' => Code::Backquote,
        ' ' => Code::Space,
        _ => return None,
    })
}

fn function_key(n: u8) -> Option<PhysicalKeyCode> {
    use PhysicalKeyCode as Code;
    Some(match n {
        1 => Code::F1,
        2 => Code::F2,
        3 => Code::F3,
        4 => Code::F4,
        5 => Code::F5,
        6 => Code::F6,
        7 => Code::F7,
        8 => Code::F8,
        9 => Code::F9,
        10 => Code::F10,
        11 => Code::F11,
        12 => Code::F12,
        13 => Code::F13,
        14 => Code::F14,
        15 => Code::F15,
        16 => Code::F16,
        17 => Code::F17,
        18 => Code::F18,
        19 => Code::F19,
        20 => Code::F20,
        21 => Code::F21,
        22 => Code::F22,
        23 => Code::F23,
        24 => Code::F24,
        _ => return None,
    })
}
