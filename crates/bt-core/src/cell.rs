//! Cells of tile layers.

use glam::IVec2;

use crate::flip::Flip;
use crate::ids::{TileId, TilesetSlot};

/// One cell of a tile layer: a tile of one of the map's tilesets, with its flips, or nothing.
/// Eight bytes and `Copy`. The empty cell has one form, [`Cell::EMPTY`], so `==` compares
/// content. Global tile ids exist only in the file format crate.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The slot plus one; 0 is the empty cell.
    slot_plus_one: u16,
    flip: Flip,
    tile: TileId,
}

impl Cell {
    /// The empty cell.
    pub const EMPTY: Cell = Cell {
        slot_plus_one: 0,
        flip: Flip::empty(),
        tile: TileId(0),
    };

    /// The tile `tile` of the tileset in `slot`, unflipped.
    pub fn new(slot: TilesetSlot, tile: TileId) -> Cell {
        // Slots stop below u16::MAX, so the sum fits.
        let slot_plus_one = u16::try_from(slot.index() + 1).unwrap_or(0);
        Cell {
            slot_plus_one,
            flip: Flip::empty(),
            tile,
        }
    }

    /// The same tile with `flip`; the empty cell stays empty.
    pub fn with_flip(self, flip: Flip) -> Cell {
        if self.is_empty() {
            return Cell::EMPTY;
        }
        Cell { flip, ..self }
    }

    /// Whether the cell holds no tile.
    pub fn is_empty(self) -> bool {
        self.slot_plus_one == 0
    }

    /// The tileset of the cell's tile, or `None` for the empty cell.
    pub fn slot(self) -> Option<TilesetSlot> {
        usize::from(self.slot_plus_one)
            .checked_sub(1)
            .and_then(TilesetSlot::new)
    }

    /// The tile's id in its tileset (0 for the empty cell).
    pub fn tile(self) -> TileId {
        self.tile
    }

    /// How the tile is turned and mirrored.
    pub fn flip(self) -> Flip {
        self.flip
    }

    /// Whether both cells hold the same tile, whatever their flips.
    pub fn same_tile(self, other: Cell) -> bool {
        self.slot_plus_one == other.slot_plus_one && self.tile == other.tile
    }
}

/// A cell at a position: what a layer or a preview holds there.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlacedCell {
    /// The position, in tiles.
    pub at: IVec2,
    /// The cell.
    pub cell: Cell,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(index: usize) -> TilesetSlot {
        TilesetSlot::new(index).unwrap()
    }

    #[test]
    fn a_cell_is_eight_bytes_and_empty_has_one_form() {
        assert_eq!(std::mem::size_of::<Cell>(), 8);
        assert_eq!(Cell::EMPTY.with_flip(Flip::H), Cell::EMPTY);
        assert_eq!(Cell::default(), Cell::EMPTY);
        assert_eq!((Cell::EMPTY.slot(), Cell::EMPTY.tile()), (None, TileId(0)));
    }

    #[test]
    fn cells_keep_their_slot_tile_and_flip() {
        let cell = Cell::new(slot(65534), TileId(7)).with_flip(Flip::H | Flip::D);
        assert_eq!(
            (cell.slot(), cell.tile(), cell.flip()),
            (Some(slot(65534)), TileId(7), Flip::H | Flip::D)
        );
        assert!(!cell.is_empty());
        assert!(cell.same_tile(Cell::new(slot(65534), TileId(7))));
        assert!(!cell.same_tile(Cell::new(slot(0), TileId(7))));
        assert_ne!(cell, Cell::new(slot(65534), TileId(7)), "flips are content");
    }
}
