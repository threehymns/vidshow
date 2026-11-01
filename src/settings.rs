//! What vidshow remembers between runs: which display presents, and how.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How the picture fills the presentation display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    /// Letterbox: the whole frame stays visible.
    #[default]
    Fit,
    /// Crop: the frame covers the display, cutting the overhanging edges.
    Fill,
}

impl FitMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fit => "Fit",
            Self::Fill => "Fill",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Fit => Self::Fill,
            Self::Fill => Self::Fit,
        }
    }

    pub fn surface(self) -> rune::SurfaceFit {
        match self {
            Self::Fit => rune::SurfaceFit::Fit,
            Self::Fill => rune::SurfaceFit::Fill,
        }
    }
}

/// Enough of a display's identity to find it again after a restart, redock or resolution change.
/// Names are not unique on Windows and the order of displays can change, so matching falls back
/// through name, then size, then the primary display.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayKey {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
}

impl DisplayKey {
    pub fn of(display: &rune::Display) -> Self {
        Self {
            name: display.name.clone(),
            width: display.size.0,
            height: display.size.1,
            x: display.position.0,
            y: display.position.1,
            primary: display.primary,
        }
    }

    /// "1920×1080", for the display picker.
    pub fn resolution(&self) -> String {
        format!("{}\u{d7}{}", self.width, self.height)
    }

    /// Whether two keys describe the same monitor in the same place at the same resolution.
    fn same_as(&self, other: &Self) -> bool {
        self.name == other.name
            && self.width == other.width
            && self.height == other.height
            && self.x == other.x
            && self.y == other.y
    }
}

/// Where the presentation goes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum TargetDisplay {
    /// The second display: the classic projector setup, and the default.
    #[default]
    Secondary,
    Primary,
    /// One particular display, by identity.
    Display(DisplayKey),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Settings {
    pub display: TargetDisplay,
    /// Keep the presentation window above other windows. It only applies to the window; a Wayland
    /// layer-shell overlay already sits above ordinary windows.
    pub always_on_top: bool,
    pub hide_cursor: bool,
    pub fit: FitMode,
    pub looping: bool,
    pub muted: bool,
    pub volume: f32,
    /// Draw a preview of the presentation output in the control window.
    pub preview: bool,
    pub last_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display: TargetDisplay::Secondary,
            always_on_top: true,
            hide_cursor: true,
            fit: FitMode::Fit,
            looping: false,
            muted: false,
            volume: 1.0,
            preview: true,
            last_dir: None,
        }
    }
}

impl Settings {
    /// The settings file: `%APPDATA%\vidshow\settings.json` on Windows, unless a
    /// `vidshow.portable` marker sits beside the exe, in which case `settings.json` is kept
    /// there so the whole folder can move between machines.
    pub fn file() -> Option<PathBuf> {
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            && dir.join("vidshow.portable").is_file()
        {
            return Some(dir.join("settings.json"));
        }
        #[cfg(windows)]
        {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                return Some(PathBuf::from(appdata).join("vidshow").join("settings.json"));
            }
        }
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(config).join("vidshow").join("settings.json"));
        }
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("vidshow")
                .join("settings.json")
        })
    }

    /// Reads the saved settings. A missing file is normal; one that cannot be read or parsed is
    /// reported, because quietly going back to defaults looks like the app forgot everything.
    pub fn load() -> (Self, Option<String>) {
        let Some(path) = Self::file() else {
            return (Self::default(), None);
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(format!("can't read {}: {error}", path.display())),
                );
            }
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(settings) => (settings.validated(), None),
            Err(error) => (
                Self::default(),
                Some(format!(
                    "{} was not usable, so defaults are in place: {error}",
                    path.display()
                )),
            ),
        }
    }

    /// Saves the settings, reporting where it failed.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = Self::file() else {
            return Err("there is nowhere to save settings on this system".into());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|error| format!("can't create {}: {error}", dir.display()))?;
        }
        let json = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("can't write the settings: {error}"))?;
        std::fs::write(&path, json)
            .map_err(|error| format!("can't write {}: {error}", path.display()))
    }

    /// Keeps what a hand-edited file can get wrong inside the range the rest of the app assumes.
    fn validated(mut self) -> Self {
        if !self.volume.is_finite() {
            self.volume = 1.0;
        }
        self.volume = self.volume.clamp(0.0, 1.0);
        self
    }

    /// The display to present on, as an index into `displays`.
    pub fn resolve(&self, displays: &[rune::Display]) -> Option<usize> {
        if displays.is_empty() {
            return None;
        }
        let keys: Vec<DisplayKey> = displays.iter().map(DisplayKey::of).collect();
        match &self.display {
            TargetDisplay::Primary => {
                Some(keys.iter().position(|key| key.primary).unwrap_or_default())
            }
            TargetDisplay::Secondary => keys
                .iter()
                .position(|key| !key.primary)
                .or_else(|| keys.iter().position(|key| key.primary))
                .or(Some(0)),
            TargetDisplay::Display(want) => keys
                .iter()
                .position(|key| key.same_as(want))
                // An unplugged or rearranged display: keep the same screen if it is still here.
                .or_else(|| {
                    want.name.as_ref().and_then(|name| {
                        keys.iter().position(|key| key.name.as_ref() == Some(name))
                    })
                })
                .or_else(|| {
                    keys.iter().position(|key| {
                        key.width == want.width
                            && key.height == want.height
                            && key.primary == want.primary
                    })
                })
                .or_else(|| keys.iter().position(|key| key.primary))
                .or(Some(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(name: &str, size: (u32, u32), position: (i32, i32), primary: bool) -> rune::Display {
        rune::Display {
            id: None,
            name: Some(name.into()),
            position,
            size,
            scale: 1.0,
            primary,
        }
    }

    #[test]
    fn a_single_display_falls_back_to_itself() {
        let settings = Settings::default();
        let only = display("Display 1", (1920, 1080), (0, 0), true);
        assert_eq!(settings.resolve(&[only]), Some(0));
    }

    #[test]
    fn secondary_prefers_a_non_primary_display() {
        let settings = Settings::default();
        let displays = vec![
            display("A", (2560, 1440), (0, 0), true),
            display("B", (1920, 1080), (2560, 0), false),
        ];
        assert_eq!(settings.resolve(&displays), Some(1));
    }

    #[test]
    fn a_saved_display_follows_its_identity() {
        let displays = vec![
            display("A", (2560, 1440), (0, 0), true),
            display("B", (1920, 1080), (2560, 0), false),
        ];
        let settings = Settings {
            display: TargetDisplay::Display(DisplayKey::of(&displays[0])),
            ..Settings::default()
        };
        assert_eq!(settings.resolve(&displays), Some(0));
        // The same monitor at a new resolution is still the same monitor.
        let mut moved = displays.clone();
        moved[0].size = (3840, 2160);
        assert_eq!(settings.resolve(&moved), Some(0));
        // Unplugged: fall back to the primary rather than nothing.
        let only_secondary = vec![displays[1].clone()];
        assert_eq!(settings.resolve(&only_secondary), Some(0));
    }

    #[test]
    fn a_hand_edited_volume_comes_back_in_range() {
        let read_back = |volume: f32| {
            Settings {
                volume,
                ..Settings::default()
            }
            .validated()
            .volume
        };
        assert_eq!(read_back(4.0), 1.0);
        assert_eq!(read_back(f32::NAN), 1.0);
        assert_eq!(read_back(-2.0), 0.0);
    }

    #[test]
    fn round_trips_through_json() {
        let settings = Settings {
            fit: FitMode::Fill,
            display: TargetDisplay::Display(DisplayKey {
                name: Some("DISPLAY2".into()),
                width: 1920,
                height: 1080,
                x: 1920,
                y: 0,
                primary: false,
            }),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fit, FitMode::Fill);
        assert_eq!(back.display, settings.display);
    }
}
