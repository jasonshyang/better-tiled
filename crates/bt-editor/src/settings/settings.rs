//! The preferences: typed, every field with a default, a version for migrations. A key that
//! names nothing is an error rather than ignored, so a typo in `settings.toml` is reported.

use std::collections::BTreeMap;

use bt_core::Color;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of the settings this build writes.
pub const SETTINGS_VERSION: u32 = 1;

/// Every preference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// The version of the file's shape.
    pub version: u32,
    /// Saving, sessions and recovery.
    pub general: General,
    /// What the canvas draws.
    pub interface: Interface,
    /// How input behaves.
    pub behaviour: Behaviour,
    /// The look of the window.
    pub theme: Theme,
    /// Key bindings that differ from the defaults: command name to chords.
    pub keymap: BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: SETTINGS_VERSION,
            general: General::default(),
            interface: Interface::default(),
            behaviour: Behaviour::default(),
            theme: Theme::default(),
            keymap: BTreeMap::new(),
        }
    }
}

/// Saving, sessions and recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// Reopen last session's files at start when none are given.
    pub restore_session: bool,
    /// Write files through a temporary file renamed over the target.
    pub safe_writing: bool,
    /// Keep snapshots of unsaved work to recover after a crash.
    pub recovery: bool,
    /// The most transactions a document's history keeps.
    pub history_limit: usize,
}

impl Default for General {
    fn default() -> General {
        General {
            restore_session: true,
            safe_writing: true,
            recovery: true,
            history_limit: crate::history::DEFAULT_HISTORY_LIMIT,
        }
    }
}

/// Every how many cells grid lines are major.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MajorGrid {
    /// Every this many columns (0 for none).
    pub columns: u32,
    /// Every this many rows (0 for none).
    pub rows: u32,
}

/// What the canvas draws.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Interface {
    /// The grid's colour (its alpha is set by the zoom, as in Tiled).
    pub grid_color: SettingColor,
    /// The major grid.
    pub major_grid: MajorGrid,
    /// What layers below the highlighted one fade toward.
    pub fade_color: SettingColor,
    /// The width of object outlines in points.
    pub object_line_width: f32,
}

impl Default for Interface {
    fn default() -> Interface {
        Interface {
            grid_color: SettingColor(Color::BLACK),
            major_grid: MajorGrid::default(),
            fade_color: SettingColor(Color::BLACK),
            object_line_width: 2.0,
        }
    }
}

/// How input behaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behaviour {
    /// The mouse wheel zooms (else it scrolls, and ⌘ or Ctrl with it zooms).
    pub wheel_zooms: bool,
}

impl Default for Behaviour {
    fn default() -> Behaviour {
        Behaviour { wheel_zooms: true }
    }
}

/// The look of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    /// Dark or light.
    pub dark: bool,
    /// The colour of selections.
    pub selection_color: SettingColor,
    /// The size of interface text in points.
    pub font_size: f32,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            dark: true,
            selection_color: SettingColor(Color::rgb(48, 140, 198)),
            font_size: 13.0,
        }
    }
}

/// A colour written as Tiled writes colours: `#rrggbb`, or `#aarrggbb` when not opaque.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SettingColor(pub Color);

impl Serialize for SettingColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_tiled_text())
    }
}

impl<'de> Deserialize<'de> for SettingColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<SettingColor, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::from_tiled_text(&text)
            .map(SettingColor)
            .ok_or_else(|| {
                serde::de::Error::custom(format!("“{text}” is not a colour (#rrggbb or #aarrggbb)"))
            })
    }
}
