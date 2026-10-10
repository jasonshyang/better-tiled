//! Files: opening, saving, closing and quitting, and what the editor knows about them.

mod add_tileset;
mod close;
mod load;
mod new;
mod open;
mod project;
mod save;

use bt_tmx::Loader;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::document::DocumentId;
use crate::effect::DecisionId;
use crate::store::AssetId;

pub use load::{LoadJob, Loaded};
pub(crate) use load::{
    LoadKind, LoadPurpose, LoadSource, LoadedContent, LoadedFiles, ProjectChoice, hash_file,
};
pub use new::{NewMap, NewTileset, NewTilesetSource};

/// What the editor knows about files beyond the assets: the loaders the maps were read with
/// (holding their templates, for writing), the open project, questions awaiting answers, and
/// whether a quit is under way.
#[derive(Debug, Default)]
pub(crate) struct Files {
    pub(crate) loaders: HashMap<AssetId, Loader>,
    pub(crate) project: Option<AssetId>,
    pub(crate) pending: HashMap<DecisionId, Pending>,
    pub(crate) next_decision: u64,
    /// The user chose to save before quitting: quit once nothing is unsaved.
    pub(crate) quitting: bool,
    /// Where to look for projects no higher than (the user's home folder).
    pub(crate) home: Option<PathBuf>,
    /// Where project files are locked while they are written (the user's runtime folder).
    pub(crate) lock_folder: Option<PathBuf>,
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
