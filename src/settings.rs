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
}

/// Where the presentation window goes.
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
    /// Keep the presentation window above other windows (it is always a borderless full-screen
    /// window; this only decides whether it floats over taskbar-topping windows too).
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
        {
            if dir.join("vidshow.portable").is_file() {
                return Some(dir.join("settings.json"));
            }
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

    pub fn load() -> Self {
        let Some(path) = Self::file() else {
            return Self::default();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let _ = std::fs::write(path, json);
    }

    pub fn volume(&self) -> f32 {
        self.volume.clamp(0.0, 1.0)
    }

    /// The display to present on, as an index into `displays`.
    pub fn resolve(&self, displays: &[rune::Display]) -> Option<usize> {
        if displays.is_empty() {
            return None;
        }
        match &self.display {
            TargetDisplay::Primary => Some(displays.iter().position(|d| d.primary).unwrap_or(0)),
            TargetDisplay::Secondary => displays
                .iter()
                .position(|d| !d.primary)
                .or_else(|| displays.iter().position(|d| d.primary))
                .or(Some(0)),
            TargetDisplay::Display(key) => Some(
                displays
                    .iter()
                    .position(|d| {
                        let k = DisplayKey::of(d);
                        k.name == key.name
                            && k.width == key.width
                            && k.height == key.height
                            && k.x == key.x
                            && k.y == key.y
                    })
                    // An unplugged or rearranged display: keep the same screen if it is still here.
                    .or_else(|| {
                        displays.iter().position(|d| {
                            let k = DisplayKey::of(d);
                            key.name.is_some() && k.name == key.name
                        })
                    })
                    .or_else(|| {
                        displays.iter().position(|d| {
                            let k = DisplayKey::of(d);
                            k.width == key.width
                                && k.height == key.height
                                && k.primary == key.primary
                        })
                    })
                    .or_else(|| displays.iter().position(|d| d.primary))
                    .unwrap_or(0),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_display_falls_back_to_itself() {
        let settings = Settings::default();
        let only = rune::Display {
            id: None,
            name: Some("Display 1".into()),
            position: (0, 0),
            size: (1920, 1080),
            scale: 1.0,
            primary: true,
        };
        assert_eq!(settings.resolve(&[only]), Some(0));
    }

    #[test]
    fn secondary_prefers_a_non_primary_display() {
        let settings = Settings::default();
        let displays = vec![
            rune::Display {
                id: None,
                name: Some("A".into()),
                position: (0, 0),
                size: (2560, 1440),
                scale: 1.0,
                primary: true,
            },
            rune::Display {
                id: None,
                name: Some("B".into()),
                position: (2560, 0),
                size: (1920, 1080),
                scale: 1.0,
                primary: false,
            },
        ];
        assert_eq!(settings.resolve(&displays), Some(1));
    }

    #[test]
    fn a_saved_display_follows_its_identity() {
        let displays = vec![
            rune::Display {
                id: None,
                name: Some("A".into()),
                position: (0, 0),
                size: (2560, 1440),
                scale: 1.0,
                primary: true,
            },
            rune::Display {
                id: None,
                name: Some("B".into()),
                position: (2560, 0),
                size: (1920, 1080),
                scale: 1.0,
                primary: false,
            },
        ];
        let mut settings = Settings::default();
        settings.display = TargetDisplay::Display(DisplayKey::of(&displays[0]));
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
    fn round_trips_through_json() {
        let mut settings = Settings::default();
        settings.fit = FitMode::Fill;
        settings.display = TargetDisplay::Display(DisplayKey {
            name: Some("DISPLAY2".into()),
            width: 1920,
            height: 1080,
            x: 1920,
            y: 0,
            primary: false,
        });
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.fit, FitMode::Fill);
        assert_eq!(back.display, settings.display);
    }
}
