//! better-tiled's editor without a window: the assets that are open, the documents that show
//! them, transactions that may span assets with undo and redo, the commands and their keys,
//! opening and saving files, the preferences, the session and recovery. No widget type appears
//! here; the window draws this state, feeds it intents and carries out the effects it returns.

mod actions;
mod automap;
mod clipboard;
mod document;
mod editor;
mod effect;
mod error;
mod external;
mod files;
mod godot;
mod history;
mod inspect;
mod intent;
mod layer_commands;
mod map_commands;
mod notice;
mod objects;
mod offset_map;
mod properties;
mod recovery;
mod session;
mod settings;
mod store;
mod terrain_helpers;
mod terrain_sets;
mod tileset_commands;
mod tools;
mod type_commands;
mod watch;

pub use actions::{
    ACTIONS, ActionId, ActionKind, ActionSpec, Chord, Context, Enabled, Key, Keymap, KeymapError,
    Modifiers, PaletteEntry, PaletteTarget, tool_action,
};
pub use automap::{
    AutoMapRules, CellExplanation, ExplainedRun, KnownTileset, LastRun, LiveEdit, RulesJob,
    RulesLoaded,
};
pub use document::{
    Document, DocumentId, Documents, Selection, TilesetEditing, TilesetMode, ViewState, ViewToggles,
};
pub use editor::{Editor, EditorBuilder};
pub use effect::{
    Choice, Decision, DecisionId, Dialog, Effect, FileRequest, GodotOperation, Purpose, Question,
    ViewRequest,
};
pub use error::EditError;
pub use external::{
    BaseRules, ExternalBase, ExternalEdit, ExternalEditError, ExternalUndo, ExternalUndoError,
    ExternalUndone, MARK_COLORS, Mark, MarkCells,
};
pub use files::{AutotilesJob, ExpandedAutotiles};
pub use files::{LoadJob, Loaded, NewMap, NewTileset, NewTilesetSource};
pub use history::{
    AssetChange, DEFAULT_HISTORY_LIMIT, History, HistoryEntry, MergeKey, SelectionChange,
    Transaction, Tx,
};
pub use inspect::Inspected;
pub use intent::Intent;
pub use notice::{Notice, Severity};
pub use objects::{
    HandleFrame, NodeHandle, ObjectFrame, ObjectRow, ObjectStroke, ObjectStrokeKind, SnapToggle,
};
pub use offset_map::{LayerChoice, MapOffset};
pub use properties::{
    Attribute, AttributeRow, AttributeValue, Change, ListEdit, Op, Origin, PropertyError,
    PropertySet, Shown, Step, Target, TargetId, TypeEdit, ValueType, conversions, convert,
    layer_attributes, map_attributes, object_attributes, set_layer_attribute, set_map_attribute,
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
pub use tools::{
    FillPreview, MAX_BRUSH_SIZE, NoTerrain, Stroke, StrokeKind, TerrainChoice, TerrainInUse,
    TerrainStroke, ToolFamily, ToolKind, ToolState,
};
pub use watch::{Found, Observation};

// The editing types the intents and queries speak in.
pub use bt_edit::{
    Brush, BrushMode, FillShape, LayerLocation, Node, ResizeHandle, SelectionMode, TileRegion,
    rect_select_rect, shape_fill_region,
};
pub use terrain_helpers::{
    FoundLayouts, LayoutSearch, LayoutTarget, TilesetImageJob, TilesetImageRead,
};
pub use terrain_sets::{LabelMark, LabelStroke, LabelTerrain, TerrainSetEdit};
