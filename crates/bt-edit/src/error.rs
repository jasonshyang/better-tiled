//! Why an operation could not run.

use bt_core::{LayerId, ObjectId, TileId, TilesetSlot};

use crate::objects::node::Node;

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
    /// The layer holds no layers.
    #[error("layer {0} is not a group")]
    NotAGroup(LayerId),
    /// A group was asked to go inside itself or one of its own groups.
    #[error("group {0} cannot go inside itself")]
    IntoItself(LayerId),
    /// A place among a group's layers (or the map's top-level layers) past the last one.
    #[error("{} has no place {index}", owner_of_places(.parent))]
    NoSuchPlace {
        /// The group; `None` for the map's top level.
        parent: Option<LayerId>,
        /// The place asked for, counted from the bottom.
        index: usize,
    },
    /// No object layer holds an object with this id.
    #[error("the map has no object {0}")]
    NoSuchObject(ObjectId),
    /// An object brought in with an id of its own collides with one already there.
    #[error("there is already an object {0}")]
    ObjectIdInUse(ObjectId),
    /// The object has no nodes to edit: it is not a polygon or a polyline.
    #[error("object {0} is neither a polygon nor a polyline")]
    NotAPolygon(ObjectId),
    /// The edit is one of a polyline's, and the object is not a polyline.
    #[error("object {0} is not a polyline")]
    NotAPolyline(ObjectId),
    /// A polygon or polyline has no node at this place.
    #[error("object {} has no node {}", .0.object, .0.index)]
    NoSuchNode(Node),
    /// An edit at the end of a polyline was given a node between its ends, or, to close a
    /// polyline, the same end twice.
    #[error(
        "node {} of object {} is not an end this edit can use: the first or last node of a \
         polyline, and the other one to close it",
        .0.index,
        .0.object
    )]
    NotAnEnd(Node),
    /// A polyline has too few nodes to close into a polygon.
    #[error("polyline {0} has too few nodes to close into a polygon")]
    TooFewNodes(ObjectId),
    /// The layer draws its objects by where they are on screen, so their order in its list
    /// cannot raise or lower them.
    #[error("layer {0} draws its objects top-down, not in their list order")]
    NotIndexOrdered(LayerId),
    /// A place in a list past its end.
    #[error("there is no place {index} in a list of {len}")]
    IndexOutOfRange {
        /// The place asked for.
        index: usize,
        /// How long the list is.
        len: usize,
    },
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
    /// A paste was given a different number of targets than its clip has layers.
    #[error("the clip has {layers} layers but {targets} targets were given")]
    TargetCount {
        /// The clip's layers.
        layers: usize,
        /// The targets given.
        targets: usize,
    },
    /// The model refused a change (a cell outside a finite layer).
    #[error(transparent)]
    Model(#[from] bt_core::Error),
}

/// What holds the places of [`EditError::NoSuchPlace`], in words.
fn owner_of_places(parent: &Option<LayerId>) -> String {
    match parent {
        Some(group) => format!("group {group}"),
        None => "the map's top level".to_owned(),
    }
}
