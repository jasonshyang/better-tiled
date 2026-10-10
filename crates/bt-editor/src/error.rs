//! Why an edit, an undo or a redo did not happen.

use bt_core::{LayerId, TileId};

use crate::document::DocumentId;
use crate::store::{AssetId, AssetKind};

/// What stopped an edit.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EditError {
    /// No asset has this id (any more).
    #[error("{0} is not open")]
    NoSuchAsset(AssetId),
    /// The asset is of another kind.
    #[error("{asset} is not a {expected}")]
    WrongKind {
        /// The asset.
        asset: AssetId,
        /// What it should have been.
        expected: AssetKind,
    },
    /// No document has this id (any more).
    #[error("{0} is not open")]
    NoSuchDocument(DocumentId),
    /// The document's tileset editor is not open.
    #[error("{0} shows no tileset")]
    NoTilesetShown(DocumentId),
    /// The map has no layer with this id.
    #[error("the map has no layer {0}")]
    NoSuchLayer(LayerId),
    /// The tileset has no tile with this id.
    #[error("the tileset has no tile {}", .0.0)]
    NoSuchTile(TileId),
    /// A gesture is under way: another edit waits for it.
    #[error("a stroke is under way")]
    GestureUnderWay,
    /// No gesture is under way.
    #[error("no stroke is under way")]
    NoGesture,
    /// Nothing to undo.
    #[error("there is nothing to undo")]
    NothingToUndo,
    /// Nothing to redo.
    #[error("there is nothing to redo")]
    NothingToRedo,
    /// A later change to an asset must be undone first.
    #[error("“{label}” in {document} changed the same {asset} later: undo it first")]
    Blocked {
        /// The asset.
        asset: AssetId,
        /// Where the later change was made.
        document: DocumentId,
        /// What it was.
        label: String,
    },
    /// The operation could not be carried out on the map.
    #[error(transparent)]
    Operation(#[from] bt_edit::EditError),
    /// A change of properties, built-in attributes or custom types was refused.
    #[error(transparent)]
    Property(#[from] crate::properties::PropertyError),
    /// A terrain paint or fill was refused.
    #[error(transparent)]
    Terrain(#[from] bt_terrain::TerrainError),
    /// The tileset has no terrain set at this index.
    #[error("the tileset has no terrain set {0}")]
    NoSuchTerrainSet(usize),
    /// The terrain set has no terrain with this index.
    #[error("the terrain set has no terrain {0}")]
    NoSuchTerrain(u8),
    /// A terrain set holds at most 254 terrains.
    #[error("a terrain set holds at most 254 terrains")]
    TooManyTerrains,
    /// A terrain's weight must be a number of at least 0.
    #[error("a terrain's probability must be 0 or more, not {0}")]
    BadProbability(f64),
    /// A later change to an asset was made where it cannot be named (its history was dropped).
    #[error("{asset} changed since; undo its later changes first")]
    ChangedSince {
        /// The asset.
        asset: AssetId,
    },
}
