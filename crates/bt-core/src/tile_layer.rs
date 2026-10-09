//! Tile layers: cells in a sparse map of 16×16 chunks, the same for finite and infinite maps.

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::cell::{Cell, PlacedCell};
use crate::chunk::{Chunk, ChunkPos};
use crate::error::Error;

/// Whether a layer's cells are confined to its size. Every tile layer of a map agrees with the
/// map's `infinite`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TileBounds {
    /// Cells exist only inside the layer's size.
    Finite,
    /// Cells exist anywhere; the size is what the file declares and bounds nothing.
    Infinite,
}

/// A layer of cells.
#[derive(Clone, Debug, PartialEq)]
pub struct TileLayer {
    size: UVec2,
    bounds: TileBounds,
    /// Ordered by row, then column, so every walk over a layer is deterministic.
    chunks: BTreeMap<ChunkPos, Arc<Chunk>>,
}

/// A chunk of a layer with its position.
#[derive(Copy, Clone, Debug)]
pub struct LayerChunk<'layer> {
    /// Where the chunk is.
    pub pos: ChunkPos,
    /// The chunk, shared with every snapshot that did not change it.
    pub chunk: &'layer Arc<Chunk>,
}

impl TileLayer {
    /// An empty layer of a finite map, holding cells in `0..size`.
    pub fn finite(size: UVec2) -> TileLayer {
        TileLayer {
            size,
            bounds: TileBounds::Finite,
            chunks: BTreeMap::new(),
        }
    }

    /// An empty layer of an infinite map; `declared_size` is the size its file states.
    pub fn infinite(declared_size: UVec2) -> TileLayer {
        TileLayer {
            size: declared_size,
            bounds: TileBounds::Infinite,
            chunks: BTreeMap::new(),
        }
    }

    /// The layer's size in cells: the bounds of a finite layer, what the file declares for an
    /// infinite one.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// Whether cells are confined to the size.
    pub fn bounds(&self) -> TileBounds {
        self.bounds
    }

    /// Whether the layer belongs to an infinite map.
    pub fn is_infinite(&self) -> bool {
        self.bounds == TileBounds::Infinite
    }

    /// Whether a cell can exist at `p`.
    pub fn contains(&self, p: IVec2) -> bool {
        match self.bounds {
            TileBounds::Infinite => true,
            TileBounds::Finite => {
                let inside = |v: i32, limit: u32| u32::try_from(v).is_ok_and(|v| v < limit);
                inside(p.x, self.size.x) && inside(p.y, self.size.y)
            }
        }
    }

    /// The cell at `p`; [`Cell::EMPTY`] where nothing was set.
    pub fn cell(&self, p: IVec2) -> Cell {
        self.chunks
            .get(&ChunkPos::of(p))
            .map_or(Cell::EMPTY, |chunk| chunk.cell(p))
    }

    /// Sets the cell at `p` and returns whether anything changed. A chunk shared with a snapshot
    /// is copied on its first change. A position outside a finite layer is an error, not ignored.
    pub fn set_cell(&mut self, p: IVec2, cell: Cell) -> Result<bool, Error> {
        if !self.contains(p) {
            return Err(Error::CellOutOfBounds {
                position: p,
                size: self.size,
            });
        }
        let pos = ChunkPos::of(p);
        match self.chunks.get_mut(&pos) {
            Some(chunk) => {
                if chunk.cell(p) == cell {
                    return Ok(false);
                }
                Ok(Arc::make_mut(chunk).set_cell(p, cell))
            }
            None if cell.is_empty() => Ok(false),
            None => {
                let mut chunk = Chunk::default();
                chunk.set_cell(p, cell);
                self.chunks.insert(pos, Arc::new(chunk));
                Ok(true)
            }
        }
    }

    /// The allocated chunks, row by row.
    pub fn chunks(&self) -> impl Iterator<Item = LayerChunk<'_>> {
        self.chunks
            .iter()
            .map(|(&pos, chunk)| LayerChunk { pos, chunk })
    }

    /// The chunk at `pos`, if allocated.
    pub fn chunk(&self, pos: ChunkPos) -> Option<&Arc<Chunk>> {
        self.chunks.get(&pos)
    }

    /// The non-empty cells, chunk by chunk in row order, row by row within a chunk.
    pub fn cells(&self) -> impl Iterator<Item = PlacedCell> + '_ {
        self.chunks
            .iter()
            .flat_map(|(&pos, chunk)| chunk.placed_cells(pos))
    }

    /// The smallest rectangle holding every non-empty cell, or `None` when there is none.
    pub fn used_bounds(&self) -> Option<IRect> {
        IRect::bounding(self.cells().map(|placed| placed.at))
    }

    /// The union of the allocated chunks (Tiled's `TileLayer::bounds`), or `None` when none is.
    pub fn chunk_bounds(&self) -> Option<IRect> {
        self.chunks
            .keys()
            .map(|pos| pos.rect())
            .reduce(|a, b| a.union(b))
    }

    /// Drops the chunks that hold no tile; operations that erase call it.
    pub fn prune_empty_chunks(&mut self) {
        self.chunks.retain(|_, chunk| !chunk.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{TileId, TilesetSlot};

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    #[test]
    fn finite_layers_refuse_cells_outside() {
        let mut layer = TileLayer::finite(UVec2::new(40, 30));
        assert_eq!(layer.set_cell(IVec2::new(17, 3), tile(7)), Ok(true));
        assert_eq!(layer.set_cell(IVec2::new(17, 3), tile(7)), Ok(false));
        assert_eq!(
            layer.set_cell(IVec2::new(40, 0), tile(7)),
            Err(Error::CellOutOfBounds {
                position: IVec2::new(40, 0),
                size: UVec2::new(40, 30)
            })
        );
        assert!(layer.set_cell(IVec2::new(-1, 0), tile(7)).is_err());
        assert_eq!(layer.cell(IVec2::new(17, 3)), tile(7));
        assert_eq!(
            layer.used_bounds(),
            Some(IRect {
                x: 17,
                y: 3,
                w: 1,
                h: 1
            })
        );
        assert_eq!(
            layer.chunk_bounds(),
            Some(IRect {
                x: 16,
                y: 0,
                w: 16,
                h: 16
            })
        );
    }

    #[test]
    fn erasing_where_nothing_is_allocates_nothing() {
        let mut layer = TileLayer::infinite(UVec2::new(30, 20));
        assert_eq!(
            layer.set_cell(IVec2::new(500, -500), Cell::EMPTY),
            Ok(false)
        );
        assert_eq!(layer.chunks().count(), 0);
        assert_eq!(layer.used_bounds(), None);
        layer.set_cell(IVec2::new(-17, 3), tile(1)).unwrap();
        layer.set_cell(IVec2::new(-17, 3), Cell::EMPTY).unwrap();
        assert_eq!(
            layer.chunks().count(),
            1,
            "an emptied chunk stays until pruned"
        );
        layer.prune_empty_chunks();
        assert_eq!(layer.chunks().count(), 0);
    }

    #[test]
    fn walks_go_row_by_row() {
        let mut layer = TileLayer::infinite(UVec2::ZERO);
        for p in [
            IVec2::new(20, 0),
            IVec2::new(-1, -1),
            IVec2::new(0, 0),
            IVec2::new(3, 0),
        ] {
            layer.set_cell(p, tile(1)).unwrap();
        }
        let order: Vec<IVec2> = layer.cells().map(|placed| placed.at).collect();
        assert_eq!(
            order,
            [
                IVec2::new(-1, -1),
                IVec2::new(0, 0),
                IVec2::new(3, 0),
                IVec2::new(20, 0)
            ]
        );
    }

    #[test]
    fn copies_share_the_chunks_they_did_not_change() {
        let mut a = TileLayer::infinite(UVec2::ZERO);
        a.set_cell(IVec2::new(0, 0), tile(1)).unwrap();
        a.set_cell(IVec2::new(-20, 40), tile(2)).unwrap();
        let mut b = a.clone();
        b.set_cell(IVec2::new(1, 0), tile(3)).unwrap();
        let far = ChunkPos::of(IVec2::new(-20, 40));
        let near = ChunkPos::of(IVec2::ZERO);
        assert!(Arc::ptr_eq(a.chunk(far).unwrap(), b.chunk(far).unwrap()));
        assert!(!Arc::ptr_eq(a.chunk(near).unwrap(), b.chunk(near).unwrap()));
        assert_eq!(a.cell(IVec2::new(1, 0)), Cell::EMPTY);
    }
}
