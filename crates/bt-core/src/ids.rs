//! The id newtypes of the model. Ids of different kinds never convert into each other.

use std::fmt;

/// A layer of a map. Unique in the map; ids are never reused (`Map::next_layer_id`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u32);

/// An object of a map, or a collision shape of a tile. Unique in the map, or in the tile.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u32);

/// A tile of a tileset: Tiled's local tile id. Unique in the tileset.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId(pub u32);

/// A class or enum of a project. Unique in the project.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomTypeId(pub u32);

/// A tileset's place in a map's list of tilesets. A map holds fewer than 65535 tilesets, which
/// is what lets a [`crate::Cell`] keep its tileset in two bytes with zero meaning "empty".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TilesetSlot(u16);

impl TilesetSlot {
    /// The most tilesets a map can hold.
    pub const LIMIT: usize = u16::MAX as usize;

    /// The slot at `index`, or `None` when a map cannot have that many tilesets.
    pub fn new(index: usize) -> Option<TilesetSlot> {
        if index < TilesetSlot::LIMIT {
            u16::try_from(index).ok().map(TilesetSlot)
        } else {
            None
        }
    }

    /// The index into the map's tilesets.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

macro_rules! display_number {
    ($($id:ty),*) => {$(
        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    )*};
}

display_number!(LayerId, ObjectId, TileId, CustomTypeId, TilesetSlot);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_stop_below_the_limit() {
        assert_eq!(TilesetSlot::new(0).map(TilesetSlot::index), Some(0));
        assert_eq!(TilesetSlot::new(65534).map(TilesetSlot::index), Some(65534));
        assert_eq!(TilesetSlot::new(65535), None);
        assert_eq!(LayerId(7).to_string(), "7");
    }
}
