//! Why a change of custom properties was refused.

use bt_core::{LayerId, ObjectId, TileId};

use super::target_id::TargetId;

/// What stopped a change of custom properties. Whatever was given to change is left as it was.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PropertyError {
    /// The map has no layer with this id.
    #[error("the map has no layer {0}")]
    NoSuchLayer(LayerId),
    /// The map has no object with this id.
    #[error("the map has no object {0}")]
    NoSuchObject(ObjectId),
    /// The tileset has no tile with this id.
    #[error("the tileset has no tile {0}")]
    NoSuchTile(TileId),
    /// A target of a tileset was given a map to change.
    #[error("{0} is not part of a map")]
    NotInMap(TargetId),
    /// A target of a map was given a tileset to change.
    #[error("{0} is not part of a tileset")]
    NotInTileset(TargetId),
    /// A property needs a name.
    #[error("a property needs a name")]
    EmptyName,
    /// A rename would replace a property a target has.
    #[error("there is a property “{0}” already")]
    NameTaken(String),
    /// The project has no class or enum of this name that property values can have.
    #[error("the project has no type “{0}” for property values")]
    UnknownType(String),
    /// The path does not suit the operation: a whole-property operation given a member, a reset
    /// given a list's item, a paste given a path, a path that does not start with a property.
    #[error("{operation} does not apply at “{path}”")]
    PathMismatch {
        /// The operation, as its command says it.
        operation: &'static str,
        /// The path, as Tiled writes it.
        path: String,
    },
}
