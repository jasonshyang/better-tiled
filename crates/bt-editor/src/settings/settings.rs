//! The preferences: typed, every field with a default, a version for migrations. A key that
//! names nothing is an error rather than ignored, so a typo in `settings.toml` is reported.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bt_core::Color;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The version of the settings this build writes.
pub const SETTINGS_VERSION: u32 = 1;

/// The most parts the fine grid may split a cell into (Tiled's limit).
pub const MAX_FINE_GRID_DIVISIONS: u16 = 128;

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

impl Settings {
    /// What is wrong with a value that is of the right kind but out of range; `None` when
    /// nothing is. Loading and changing settings refuse such values.
    pub fn problem(&self) -> Option<String> {
        if let Some(folder) = &self.general.stamps_folder
            && !folder.is_absolute()
        {
            return Some("general.stamps_folder must be an absolute folder path".into());
        }
        let divisions = self.interface.fine_grid_divisions;
        if !(1..=MAX_FINE_GRID_DIVISIONS).contains(&divisions) {
            return Some(format!(
                "interface.fine_grid_divisions must be from 1 to {MAX_FINE_GRID_DIVISIONS}, not {divisions}"
            ));
        }
        None
    }
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
    /// An existing Tiled stamp folder, or the data folder's `stamps` directory by default.
    pub stamps_folder: Option<PathBuf>,
    /// Reopen last session's files at start when none are given.
    pub restore_session: bool,
    /// Write files through a temporary file renamed over the target.
    pub safe_writing: bool,
    /// Keep snapshots of unsaved work to recover after a crash.
    pub recovery: bool,
    /// The most transactions a document's history keeps.
    pub history_limit: usize,
    /// Notice files changed by other programs: reload them, or ask when they hold unsaved work.
    pub watch_files: bool,
    /// Let `bt` work with the open maps: the editor listens for it (`11-live-link.md`). There
    /// is no live link on Windows.
    pub live_link: bool,
}

impl Default for General {
    fn default() -> General {
        General {
            stamps_folder: None,
            restore_session: true,
            safe_writing: true,
            recovery: true,
            history_limit: crate::history::DEFAULT_HISTORY_LIMIT,
            watch_files: true,
            live_link: true,
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
    /// How many parts the fine grid splits a cell into, for Snap to Fine Grid: from 1 to
    /// [`MAX_FINE_GRID_DIVISIONS`].
    pub fine_grid_divisions: u16,
}

impl Default for Interface {
    fn default() -> Interface {
        Interface {
            grid_color: SettingColor(Color::BLACK),
            major_grid: MajorGrid::default(),
            fade_color: SettingColor(Color::BLACK),
            object_line_width: 2.0,
            fine_grid_divisions: 4,
        }
    }
}

/// How input behaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Behaviour {
    /// The mouse wheel zooms (else it scrolls, and ⌘ or Ctrl with it zooms).
    pub wheel_zooms: bool,
    /// Where objects go when they are placed or moved.
    pub snapping: Snapping,
    /// Which layers a click selects objects on.
    pub object_selection: ObjectSelection,
    /// A rubber band selects only the objects it encloses (else those it touches).
    pub select_enclosed: bool,
    /// A tool's own key pressed again goes back to the tool before it.
    pub tool_key_returns: bool,
}

impl Default for Behaviour {
    fn default() -> Behaviour {
        Behaviour {
            wheel_zooms: true,
            snapping: Snapping::None,
            object_selection: ObjectSelection::PreferCurrentLayer,
            select_enclosed: false,
            tool_key_returns: false,
        }
    }
}

/// Where objects go (View ▸ Snapping).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Snapping {
    /// Where they are put.
    #[default]
    None,
    /// To the tile grid.
    Grid,
    /// To the fine grid.
    FineGrid,
    /// To whole pixels.
    Pixels,
}

/// Which layers a click selects objects on (Tiled's object selection behaviour).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectSelection {
    /// Any visible, unlocked object layer.
    AnyLayer,
    /// The current layer's objects first, then any layer's.
    #[default]
    PreferCurrentLayer,
    /// The current layer only.
    CurrentLayerOnly,
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
