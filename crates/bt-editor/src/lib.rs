//! better-tiled's editor without a window: the assets that are open, the documents that show
//! them, transactions that may span assets with undo and redo, the commands and their keys,
//! opening and saving files, the preferences, the session and recovery. No widget type appears
//! here; the window draws this state, feeds it intents and carries out the effects it returns.

mod actions;
mod clipboard;
mod document;
mod editor;
mod effect;
mod error;
mod files;
mod history;
mod intent;
mod layer_commands;
mod map_commands;
mod notice;
mod objects;
mod properties;
mod recovery;
mod session;
mod settings;
mod store;
mod tileset_commands;
mod tools;
mod watch;

pub use actions::{
    ACTIONS, ActionId, ActionKind, ActionSpec, Chord, Context, Enabled, Key, Keymap, KeymapError,
    Modifiers, PaletteEntry, PaletteTarget, tool_action,
};
pub use document::{
    Document, DocumentId, Documents, Selection, TilesetEditing, TilesetMode, ViewState, ViewToggles,
};
pub use editor::{Editor, EditorBuilder};
pub use effect::{
    Choice, Decision, DecisionId, Dialog, Effect, FileRequest, Purpose, Question, ViewRequest,
};
pub use error::EditError;
pub use files::{LoadJob, Loaded, NewMap, NewTileset, NewTilesetSource};
pub use history::{
    AssetChange, DEFAULT_HISTORY_LIMIT, History, HistoryEntry, MergeKey, SelectionChange,
    Transaction, Tx,
};
pub use intent::Intent;
pub use notice::{Notice, Severity};
pub use objects::{HandleFrame, ObjectFrame, ObjectStroke, ObjectStrokeKind, SnapToggle};
pub use properties::{
    Attribute, AttributeRow, AttributeValue, Change, ListEdit, Op, Origin, PropertyError,
    PropertySet, Shown, Step, Target, TargetId, ValueType, conversions, convert, layer_attributes,
    map_attributes, object_attributes, set_layer_attribute, set_map_attribute,
    set_object_attribute, set_tile_attribute, set_tileset_attribute, tile_attributes,
    tileset_attributes, value_types,
};
pub use recovery::{
    Orphan, Snapshot, SnapshotMeta, delete_snapshot, orphans, remove_heartbeat, touch_heartbeat,
};
pub use session::{FileViewState, Session};
pub use settings::{
    Behaviour, General, Interface, MAX_FINE_GRID_DIVISIONS, MajorGrid, MigrateError,
    ObjectSelection, SETTINGS_VERSION, SettingColor, Settings, SettingsError, Snapping, Theme,
    data_folder,
};
pub use store::{Asset, AssetId, AssetKind, AssetStore, NewAsset, OnDisk, Root, TilesetHome};
pub use tools::{FillPreview, MAX_BRUSH_SIZE, Stroke, StrokeKind, ToolFamily, ToolKind, ToolState};
pub use watch::{Found, Observation};

// The editing types the intents and queries speak in.
pub use bt_edit::{
    Brush, BrushMode, FillShape, ResizeHandle, SelectionMode, TileRegion, rect_select_rect,
    shape_fill_region,
};
