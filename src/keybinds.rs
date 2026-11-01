//! Keybindings: every shortcut the app answers to, in one table.
//!
//! One [`Action`] per command, one default chord per action, and one [`Keybinds::action_for`] that
//! maps a key event back to the physical key it was pressed on.

use std::collections::BTreeMap;

use rune::{Key, KeyEvent, PhysicalKeyCode, Shortcut};

/// Open is the one chord that differs by platform; the rest are the same everywhere.
#[cfg(target_os = "macos")]
const OPEN_CHORD: &str = "super+o";
#[cfg(not(target_os = "macos"))]
const OPEN_CHORD: &str = "ctrl+o";

/// Declares the actions once: the enum, [`Action::ALL`] and every default chord.
macro_rules! actions {
    ($($variant:ident => $chord:expr),+ $(,)?) => {
        /// One command, whichever window asks for it.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Action {
            $($variant),+
        }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$variant),+];

            fn default_chord(self) -> &'static str {
                match self {
                    $(Action::$variant => $chord),+
                }
            }
        }
    };
}
actions! {
    PlayPause => "space",
    Present => "f",
    Mute => "m",
    Loop => "l",
    Preview => "v",
    Cursor => "c",
    Open => OPEN_CHORD,
    Restart => "home",
    SeekBack => "left",
    SeekBackFast => "shift+left",
    SeekForward => "right",
    SeekForwardFast => "shift+right",
    VolumeUp => "up",
    VolumeDown => "down",
    Close => "escape",
}

/// The actions that only move the clip. Every one of them is carried out by the control window
/// whatever asked for it, and none of them needs the control window itself.
impl Action {
    pub fn is_transport(self) -> bool {
        matches!(
            self,
            Self::PlayPause
                | Self::Mute
                | Self::Loop
                | Self::Restart
                | Self::SeekBack
                | Self::SeekBackFast
                | Self::SeekForward
                | Self::SeekForwardFast
                | Self::VolumeUp
                | Self::VolumeDown
        )
    }

    /// Transport a window that only shows the picture offers. It has no volume control of its own,
    /// so the volume keys stay with the control window.
    pub fn in_projector(self) -> bool {
        self.is_transport() && !matches!(self, Self::VolumeUp | Self::VolumeDown)
    }
}

/// The chord each action answers to.
#[derive(Clone)]
pub struct Keybinds {
    chords: BTreeMap<Action, Shortcut>,
}

impl Keybinds {
    pub fn defaults() -> Self {
        let mut chords = BTreeMap::new();
        for action in Action::ALL.iter().copied() {
            match action.default_chord().parse::<Shortcut>() {
                Ok(chord) => {
                    chords.insert(action, chord);
                }
                // A chord that no longer parses would silently unbind its action.
                Err(_) => debug_assert!(
                    false,
                    "{action:?} has an invalid default chord: {}",
                    action.default_chord()
                ),
            }
        }
        Self { chords }
    }

    /// The action `event` asks for, if this table binds it.
    pub fn action_for(&self, event: &KeyEvent) -> Option<Action> {
        Action::ALL.iter().copied().find(|action| {
            self.chords
                .get(action)
                .is_some_and(|chord| matches(chord, event))
        })
    }
}

fn matches(chord: &Shortcut, event: &KeyEvent) -> bool {
    let want = chord.modifiers();
    let held = event.mods;
    let logo = cfg!(target_os = "macos") || want.logo == held.logo;
    if want.shift != held.shift
        || want.alt != held.alt
        || want.cmd != held.cmd
        || want.ctrl != held.ctrl
        || !logo
    {
        return false;
    }
    physical(&event.key) == Some(chord.physical_key())
}

/// Rune matches accelerators by physical position, so the logical key an event carries is mapped
/// back to it.
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
