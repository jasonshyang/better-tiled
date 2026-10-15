//! Which thing a target of the Properties panel is.

use std::fmt;

use bt_core::{CustomTypeId, LayerId, ObjectId, TileId};

/// Which thing a target is, and so where its own properties are kept: in a map (the map, a
/// layer, an object), in a tileset (the tileset, a tile) or in the project's custom types (a
/// class's members).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetId {
    /// A standalone property collection, such as a world or project's properties.
    Properties,
    /// The map.
    Map,
    /// A layer of the map.
    Layer(LayerId),
    /// An object of the map.
    Object(ObjectId),
    /// The tileset.
    Tileset,
    /// A tile of the tileset.
    Tile(TileId),
    /// A terrain set of the tileset.
    TerrainSet(usize),
    /// A terrain of a terrain set of the tileset.
    Terrain {
        /// The set.
        set: usize,
        /// The terrain (1-based).
        color: u8,
    },
    /// The members of a class of the project, with their defaults.
    Members(CustomTypeId),
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetId::Properties => f.write_str("the properties"),
            TargetId::Map => f.write_str("the map"),
            TargetId::Layer(id) => write!(f, "layer {id}"),
            TargetId::Object(id) => write!(f, "object {id}"),
            TargetId::Tileset => f.write_str("the tileset"),
            TargetId::Tile(id) => write!(f, "tile {id}"),
            TargetId::TerrainSet(set) => write!(f, "terrain set {set}"),
            TargetId::Terrain { set, color } => write!(f, "terrain {color} of set {set}"),
            TargetId::Members(id) => write!(f, "the members of type {id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_name_themselves() {
        assert_eq!(TargetId::Layer(LayerId(2)).to_string(), "layer 2");
        assert_eq!(TargetId::Tileset.to_string(), "the tileset");
    }
}
