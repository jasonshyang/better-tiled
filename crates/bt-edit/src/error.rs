//! Why an operation could not run.

use bt_core::{LayerId, ObjectId, TileId, TilesetSlot};

/// A request an operation cannot carry out: something it names does not exist or is of the
/// wrong kind. "Nothing to do" is not an error; it is an empty report.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// The map has no layer with this id.
    #[error("the map has no layer {0}")]
    NoSuchLayer(LayerId),
    /// The layer holds no cells.
    #[error("layer {0} is not a tile layer")]
    NotATileLayer(LayerId),
    /// The layer holds no objects.
    #[error("layer {0} is not an object layer")]
    NotAnObjectLayer(LayerId),
    /// No object layer holds an object with this id.
    #[error("the map has no object {0}")]
    NoSuchObject(ObjectId),
    /// The map has no tileset in this slot.
    #[error("the map has no tileset {0}")]
    NoSuchTileset(TilesetSlot),
    /// The tileset has no tile with this id.
    #[error("tileset {slot} has no tile {tile}")]
    NoSuchTile {
        /// The tileset.
        slot: TilesetSlot,
        /// The tile.
        tile: TileId,
    },
    /// A tile was asked for with the empty cell.
    #[error("an empty cell names no tile")]
    EmptyCell,
    /// The map holds as many tilesets as it can.
    #[error("the map holds as many tilesets as a map can")]
    TooManyTilesets,
    /// The map has given out every layer or object id.
    #[error("the map has used up its ids")]
    OutOfIds,
    /// The model refused a change (a cell outside a finite layer).
    #[error(transparent)]
    Model(#[from] bt_core::Error),
}
