//! A transaction: one undoable change, possibly to several assets at once, with the selection
//! before and after.

use bt_automap::layers::Setup;

use crate::document::{DocumentId, Selection};
use crate::store::{AssetId, Root};

/// Consecutive transactions with the same key fold into one (typing in a field, dragging a
/// slider) until the run is broken.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MergeKey(pub String);

/// One asset's change: its root and version before and after.
#[derive(Clone, Debug)]
pub struct AssetChange {
    pub(crate) asset: AssetId,
    pub(crate) before: Root,
    pub(crate) after: Root,
    pub(crate) before_version: u64,
    pub(crate) after_version: u64,
}

impl AssetChange {
    /// The asset changed.
    pub fn asset(&self) -> AssetId {
        self.asset
    }
}

/// The selection before and after a transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionChange {
    pub(crate) before: Selection,
    pub(crate) after: Selection,
}

/// The document's AutoMap layers before and after a transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoMapChange {
    pub(crate) before: Option<Setup>,
    pub(crate) after: Option<Setup>,
}

/// One undoable change.
#[derive(Clone, Debug)]
pub struct Transaction {
    pub(crate) label: String,
    pub(crate) merge_key: Option<MergeKey>,
    pub(crate) document: DocumentId,
    pub(crate) selection: SelectionChange,
    pub(crate) automap: AutoMapChange,
    pub(crate) changes: Vec<AssetChange>,
}

impl Transaction {
    /// What it is called in menus and the History panel.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The document it was made in.
    pub fn document(&self) -> DocumentId {
        self.document
    }

    /// The assets it changed.
    pub fn changes(&self) -> &[AssetChange] {
        &self.changes
    }
}
