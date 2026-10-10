//! Which thing a target of the Properties panel is.

use std::fmt;

use bt_core::{LayerId, ObjectId, TileId};

/// Which thing a target is, and so where its own properties are kept: in a map (the map, a
/// layer, an object) or in a tileset (the tileset, a tile).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetId {
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
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TargetId::Map => f.write_str("the map"),
            TargetId::Layer(id) => write!(f, "layer {id}"),
            TargetId::Object(id) => write!(f, "object {id}"),
            TargetId::Tileset => f.write_str("the tileset"),
            TargetId::Tile(id) => write!(f, "tile {id}"),
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
