//! better-tiled's editor without a window: the assets that are open, the documents that show
//! them, transactions that may span assets with undo and redo, the commands and their keys,
//! opening and saving files, the preferences, the session and recovery. No widget type appears
//! here; the window draws this state, feeds it intents and carries out the effects it returns.

mod actions;
mod document;
mod editor;
mod effect;
mod error;
mod files;
mod history;
mod intent;
mod notice;
mod recovery;
mod session;
mod settings;
mod store;

pub use actions::{
    ACTIONS, ActionId, ActionKind, ActionSpec, Chord, Context, Key, Keymap, KeymapError, Modifiers,
};
pub use document::{Document, DocumentId, Documents, Selection, ViewState, ViewToggles};
pub use editor::{Editor, EditorBuilder, Enabled};
pub use effect::{
    Choice, Decision, DecisionId, Effect, FileRequest, Purpose, Question, ViewRequest,
};
pub use error::EditError;
pub use files::{LoadJob, Loaded};
pub use history::{
    AssetChange, DEFAULT_HISTORY_LIMIT, History, HistoryEntry, MergeKey, SelectionChange,
    Transaction, Tx,
};
pub use intent::Intent;
pub use notice::{Notice, Severity};
pub use recovery::{
    Orphan, Snapshot, SnapshotMeta, delete_snapshot, orphans, remove_heartbeat, touch_heartbeat,
};
pub use session::{FileViewState, Session};
pub use settings::{
    Behaviour, General, Interface, MajorGrid, MigrateError, SETTINGS_VERSION, SettingColor,
    Settings, SettingsError, Theme, data_folder,
};
pub use store::{Asset, AssetId, AssetKind, AssetStore, NewAsset, OnDisk, Root};
