//! Files: opening, saving, closing and quitting, and what the editor knows about them.

mod add_tileset;
mod close;
mod load;
mod new;
mod open;
mod save;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bt_core::CustomTypes;
use bt_tmx::Loader;

use crate::document::DocumentId;
use crate::effect::DecisionId;
use crate::store::AssetId;

pub use load::{LoadJob, Loaded};
pub(crate) use load::{
    LoadKind, LoadPurpose, LoadSource, LoadedContent, LoadedFiles, ProjectChoice,
};
pub use new::{NewMap, NewTileset, NewTilesetSource};

/// What the editor knows about files beyond the assets: the loaders the maps were read with
/// (holding their templates, for writing), the open project, questions awaiting answers, and
/// whether a quit is under way.
#[derive(Debug, Default)]
pub(crate) struct Files {
    pub(crate) loaders: HashMap<AssetId, Loader>,
    pub(crate) project: Option<AssetId>,
    pub(crate) custom_types: Arc<CustomTypes>,
    pub(crate) pending: HashMap<DecisionId, Pending>,
    pub(crate) next_decision: u64,
    /// The user chose to save before quitting: quit once nothing is unsaved.
    pub(crate) quitting: bool,
    /// Where to look for projects no higher than (the user's home folder).
    pub(crate) home: Option<PathBuf>,
}

/// What an awaited answer is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    /// Closing a document with unsaved work.
    Close(DocumentId),
    /// Quitting with unsaved work.
    Quit,
    /// Opening snapshots of an earlier run again.
    Recover(Vec<crate::recovery::Orphan>),
    /// A file changed on disk under unsaved work: the content hash on disk when asked.
    Conflict {
        /// The asset.
        asset: AssetId,
        /// What the file held when the question was asked.
        content_hash: u64,
    },
}
