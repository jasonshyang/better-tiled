//! What a terrain request can be refused for.

use bt_core::{Lane, LayerId, TilesetSlot};
use thiserror::Error;

/// Why a terrain set could not be prepared or a request could not run. Nothing is changed when
/// a request is refused.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum TerrainError {
    /// The map has no tileset in the slot.
    #[error("the map has no tileset in slot {0}")]
    NoSuchTileset(TilesetSlot),
    /// The tileset has no terrain set at the index.
    #[error("the tileset “{tileset}” has no terrain set {index}")]
    NoSuchSet {
        /// The tileset's name.
        tileset: String,
        /// The index asked for.
        index: usize,
    },
    /// The map has no layer with the id.
    #[error("the map has no layer {0}")]
    NoSuchLayer(LayerId),
    /// The layer is not a tile layer.
    #[error("“{0}” is not a tile layer")]
    NotTileLayer(String),
    /// The set has no terrain with the index.
    #[error("the terrain set has no terrain {color} (it has {count})")]
    NoSuchColor {
        /// The terrain asked for.
        color: u8,
        /// How many the set has.
        count: usize,
    },
    /// An edge target names a corner.
    #[error("an edge target needs an edge, not the {0:?} corner")]
    NotAnEdge(Lane),
    /// Symmetry was asked for on an infinite map, which has no centre.
    #[error("painting with symmetry needs a finite map")]
    MirrorOnInfiniteMap,
}
