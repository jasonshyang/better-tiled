//! Global tile ids: how files refer to tiles. They exist nowhere outside this crate.
//!
//! The high four bits are flags (`docs/reference/global-tile-ids.rst` in Tiled); the other 28
//! index a map-wide space in which each tileset owns the range starting at its first gid.

use std::collections::BTreeMap;

use bt_core::{Cell, Flip, TileId, TilesetSlot};

/// Mirrored left-right.
const FLIPPED_HORIZONTALLY: u32 = 0x8000_0000;
/// Mirrored top-bottom.
const FLIPPED_VERTICALLY: u32 = 0x4000_0000;
/// Mirrored along the anti-diagonal; a 60° turn on hexagonal maps.
const FLIPPED_ANTI_DIAGONALLY: u32 = 0x2000_0000;
/// A 120° turn on hexagonal maps; masked on every orientation.
const ROTATED_HEXAGONAL_120: u32 = 0x1000_0000;
/// Every flag bit.
const FLAGS: u32 =
    FLIPPED_HORIZONTALLY | FLIPPED_VERTICALLY | FLIPPED_ANTI_DIAGONALLY | ROTATED_HEXAGONAL_120;

/// A global id split into its index and its flags.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SplitGid {
    /// The gid without flags; 0 is no tile.
    pub(crate) gid: u32,
    /// The flags.
    pub(crate) flip: Flip,
}

/// Splits a global id as written into its index and flags.
pub(crate) fn split(raw: u32) -> SplitGid {
    let mut flip = Flip::empty();
    for (bit, flag) in [
        (FLIPPED_HORIZONTALLY, Flip::H),
        (FLIPPED_VERTICALLY, Flip::V),
        (FLIPPED_ANTI_DIAGONALLY, Flip::D),
        (ROTATED_HEXAGONAL_120, Flip::HEX120),
    ] {
        if raw & bit != 0 {
            flip |= flag;
        }
    }
    SplitGid {
        gid: raw & !FLAGS,
        flip,
    }
}

/// Joins an index and flags into a global id as written.
pub(crate) fn join(gid: u32, flip: Flip) -> u32 {
    let mut raw = gid & !FLAGS;
    for (bit, flag) in [
        (FLIPPED_HORIZONTALLY, Flip::H),
        (FLIPPED_VERTICALLY, Flip::V),
        (FLIPPED_ANTI_DIAGONALLY, Flip::D),
        (ROTATED_HEXAGONAL_120, Flip::HEX120),
    ] {
        if flip.contains(flag) {
            raw |= bit;
        }
    }
    raw
}

/// Why a global id names no tile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum GidError {
    /// A tile is used, but the file declares no tilesets.
    NoTilesets,
    /// The gid is below the first tileset's first gid.
    BelowFirstTileset,
}

/// Maps global ids to cells through the first gid of each tileset (Tiled's `GidMapper`,
/// `libtiled/gidmapper.cpp`).
#[derive(Clone, Debug, Default)]
pub(crate) struct GidMapper {
    first_gids: BTreeMap<u32, TilesetSlot>,
}

impl GidMapper {
    /// Gives the range starting at `first_gid` to the tileset in `slot`; a later tileset with
    /// the same first gid takes it over.
    pub(crate) fn insert_tileset(&mut self, first_gid: u32, slot: TilesetSlot) {
        self.first_gids.insert(first_gid, slot);
    }

    /// The cell a global id names: the tileset with the largest first gid at or below it owns
    /// it, even past its last tile (Tiled keeps such references). Gid 0 is the empty cell,
    /// whatever its flags.
    pub(crate) fn cell(&self, raw: u32) -> Result<Cell, GidError> {
        let SplitGid { gid, flip } = split(raw);
        if gid == 0 {
            return Ok(Cell::EMPTY);
        }
        if self.first_gids.is_empty() {
            return Err(GidError::NoTilesets);
        }
        let (first_gid, slot) = self
            .first_gids
            .range(..=gid)
            .next_back()
            .ok_or(GidError::BelowFirstTileset)?;
        Ok(Cell::new(*slot, TileId(gid - first_gid)).with_flip(flip))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(index: usize) -> TilesetSlot {
        TilesetSlot::new(index).unwrap()
    }

    #[test]
    fn flags_split_and_join() {
        let split_gid = split(0x8000_0000 | 0x2000_0000 | 42);
        assert_eq!(split_gid.gid, 42);
        assert_eq!(split_gid.flip, Flip::H | Flip::D);
        assert_eq!(
            split(0x4000_0000 | 0x1000_0000 | 7).flip,
            Flip::V | Flip::HEX120
        );
        for raw in [0, 1, 0xF000_0001, 0x9000_0123, 0x0FFF_FFFF] {
            let SplitGid { gid, flip } = split(raw);
            assert_eq!(join(gid, flip), raw);
        }
    }

    #[test]
    fn the_largest_first_gid_below_owns_a_gid() {
        let mut mapper = GidMapper::default();
        assert_eq!(mapper.cell(0), Ok(Cell::EMPTY));
        assert_eq!(mapper.cell(5), Err(GidError::NoTilesets));
        mapper.insert_tileset(1, slot(0));
        mapper.insert_tileset(305, slot(1));
        mapper.insert_tileset(100, slot(2));
        assert_eq!(mapper.cell(1), Ok(Cell::new(slot(0), TileId(0))));
        assert_eq!(mapper.cell(99), Ok(Cell::new(slot(0), TileId(98))));
        assert_eq!(mapper.cell(100), Ok(Cell::new(slot(2), TileId(0))));
        assert_eq!(mapper.cell(305), Ok(Cell::new(slot(1), TileId(0))));
        assert_eq!(
            mapper.cell(100_000),
            Ok(Cell::new(slot(1), TileId(100_000 - 305)))
        );
        assert_eq!(
            mapper.cell(0x8000_0000 | 0x4000_0000 | 306),
            Ok(Cell::new(slot(1), TileId(1)).with_flip(Flip::H | Flip::V))
        );
        assert_eq!(mapper.cell(0x8000_0000), Ok(Cell::EMPTY));
        let mut sparse = GidMapper::default();
        sparse.insert_tileset(10, slot(0));
        assert_eq!(sparse.cell(9), Err(GidError::BelowFirstTileset));
    }
}
