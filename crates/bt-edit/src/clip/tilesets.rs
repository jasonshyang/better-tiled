//! The tileset list of a clip being made: cells from a map or from other clips come in with
//! slots of their own list and leave with slots of this one, each tileset listed once.

use std::sync::Arc;

use bt_core::{Cell, TilesetRef, TilesetSlot};

use crate::error::EditError;

/// The tilesets a clip's cells use, in the order they were first met.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClipTilesets {
    list: Vec<TilesetRef>,
}

impl ClipTilesets {
    /// An empty list.
    pub(crate) fn new() -> ClipTilesets {
        ClipTilesets::default()
    }

    /// `cell`, whose slot indexes `source`, with its slot in this list; the tileset is added on
    /// first use. The empty cell stays empty. A slot `source` lacks is an error, as is a list
    /// longer than a map can hold.
    pub(crate) fn adopt(&mut self, source: &[TilesetRef], cell: Cell) -> Result<Cell, EditError> {
        let Some(slot) = cell.slot() else {
            return Ok(Cell::EMPTY);
        };
        let tileset = source
            .get(slot.index())
            .ok_or(EditError::NoSuchTileset(slot))?;
        let index = match self
            .list
            .iter()
            .position(|known| Arc::ptr_eq(&known.tileset, &tileset.tileset))
        {
            Some(index) => index,
            None => {
                self.list.push(tileset.clone());
                self.list.len() - 1
            }
        };
        let adopted = TilesetSlot::new(index).ok_or(EditError::TooManyTilesets)?;
        Ok(Cell::new(adopted, cell.tile()).with_flip(cell.flip()))
    }

    /// The list.
    pub(crate) fn into_list(self) -> Vec<TilesetRef> {
        self.list
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Flip, TileId};

    use super::*;
    use crate::testutil::{atlas, tile_of};

    #[test]
    fn tilesets_are_listed_once_in_the_order_met() {
        let (a, b) = (
            TilesetRef::embedded(atlas("a")),
            TilesetRef::embedded(atlas("b")),
        );
        let source = vec![a.clone(), b.clone()];
        let mut tilesets = ClipTilesets::new();
        let first = tilesets
            .adopt(&source, tile_of(1, 3).with_flip(Flip::V))
            .unwrap();
        assert_eq!(first, tile_of(0, 3).with_flip(Flip::V), "b comes first");
        assert_eq!(
            tilesets.adopt(&source, tile_of(0, 2)).unwrap(),
            tile_of(1, 2)
        );
        assert_eq!(
            tilesets.adopt(&source, tile_of(1, 1)).unwrap(),
            tile_of(0, 1)
        );
        assert_eq!(tilesets.adopt(&source, Cell::EMPTY).unwrap(), Cell::EMPTY);
        let missing = Cell::new(TilesetSlot::new(5).unwrap(), TileId(0));
        assert_eq!(
            tilesets.adopt(&source, missing),
            Err(EditError::NoSuchTileset(TilesetSlot::new(5).unwrap()))
        );
        let list = tilesets.into_list();
        assert_eq!(list, vec![b, a]);
    }
}
