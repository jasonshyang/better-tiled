//! What a tileset keeps about a tile beyond its place in the image.

use bt_utils::IRect;

use crate::ids::{LayerId, TileId};
use crate::image_ref::ImageRef;
use crate::object_layer::ObjectLayer;
use crate::property::Properties;

/// A tile's data. An atlas keeps entries only for tiles that have some; a collection has one
/// for every tile, with its image.
#[derive(Clone, Debug, PartialEq)]
pub struct TileData {
    /// The class; empty for none.
    pub class: String,
    /// The relative weight for random painting and terrain filling (1).
    pub probability: f64,
    /// A collection tile's image.
    pub image: Option<ImageRef>,
    /// The part of that image the tile is (Tiled 1.9); `None` for the whole image.
    pub rect: Option<IRect>,
    /// The frames of an animated tile; empty for a still one.
    pub animation: Vec<Frame>,
    /// The tile's collision shapes; `None` when the tile has no group for them.
    pub collision: Option<CollisionGroup>,
    /// Custom properties.
    pub properties: Properties,
}

impl Default for TileData {
    fn default() -> TileData {
        TileData {
            class: String::new(),
            probability: 1.0,
            image: None,
            rect: None,
            animation: Vec::new(),
            collision: None,
            properties: Properties::new(),
        }
    }
}

/// One frame of an animated tile.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    /// The tile shown, in the same tileset.
    pub tile: TileId,
    /// How long it is shown.
    pub duration_ms: u32,
}

/// A tile's collision shapes, in the object group Tiled keeps them in: the shapes are in the
/// tile's own pixel space, with ids unique in the tile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollisionGroup {
    /// The group's id as the file gives it; `LayerId(0)` when it gives none.
    pub id: LayerId,
    /// The group's colour, draw order and shapes.
    pub layer: ObjectLayer,
}
