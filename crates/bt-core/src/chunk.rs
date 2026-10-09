//! Chunks: the 16×16 blocks a tile layer stores its cells in. A chunk is the unit of
//! copy-on-write sharing (an undo snapshot shares every chunk an edit did not touch), of GPU
//! caching and of dirty tracking.

use std::cmp::Ordering;
use std::fmt;

use bt_utils::IRect;
use glam::IVec2;

use crate::cell::{Cell, PlacedCell};

/// The side of a chunk, in cells.
pub const CHUNK_SIZE: i32 = 16;

/// The cells of a chunk.
const CHUNK_AREA: usize = 256;

/// A chunk's position, in chunks: the chunk at `(x, y)` holds cells `16x..16x + 16` by
/// `16y..16y + 16`. Ordered by row, then column, so walks over a layer go row by row.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    /// The column of chunks.
    pub x: i32,
    /// The row of chunks.
    pub y: i32,
}

impl ChunkPos {
    /// The chunk holding the cell at `tile` (floor division, so `-1` is in chunk `-1`).
    pub fn of(tile: IVec2) -> ChunkPos {
        ChunkPos {
            x: tile.x.div_euclid(CHUNK_SIZE),
            y: tile.y.div_euclid(CHUNK_SIZE),
        }
    }

    /// The position of the chunk's top-left cell.
    pub fn origin(self) -> IVec2 {
        IVec2::new(self.x * CHUNK_SIZE, self.y * CHUNK_SIZE)
    }

    /// The cells the chunk covers.
    pub fn rect(self) -> IRect {
        IRect::from_min_size(self.origin(), IVec2::splat(CHUNK_SIZE))
    }
}

impl Ord for ChunkPos {
    fn cmp(&self, other: &ChunkPos) -> Ordering {
        (self.y, self.x).cmp(&(other.y, other.x))
    }
}

impl PartialOrd for ChunkPos {
    fn partial_cmp(&self, other: &ChunkPos) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 16×16 cells, row by row. A chunk carries no revision number: a cache that wants to know
/// whether a chunk changed keeps its `Arc` and compares pointers.
#[derive(Clone, PartialEq, Eq)]
pub struct Chunk {
    cells: [Cell; CHUNK_AREA],
}

impl Default for Chunk {
    fn default() -> Chunk {
        Chunk {
            cells: [Cell::EMPTY; CHUNK_AREA],
        }
    }
}

impl fmt::Debug for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chunk")
            .field(
                "tiles",
                &self.cells.iter().filter(|c| !c.is_empty()).count(),
            )
            .finish()
    }
}

/// The index of a cell in its chunk.
fn index_in_chunk(tile: IVec2) -> usize {
    let local = IVec2::new(tile.x.rem_euclid(CHUNK_SIZE), tile.y.rem_euclid(CHUNK_SIZE));
    // Both coordinates are in 0..16.
    usize::try_from(local.y * CHUNK_SIZE + local.x).unwrap_or(0)
}

impl Chunk {
    /// The cell at `tile`, a position anywhere in the layer; only its place within a chunk
    /// counts.
    pub fn cell(&self, tile: IVec2) -> Cell {
        self.cells
            .get(index_in_chunk(tile))
            .copied()
            .unwrap_or(Cell::EMPTY)
    }

    /// Sets the cell at `tile` (its place within a chunk); returns whether it changed.
    pub(crate) fn set_cell(&mut self, tile: IVec2, cell: Cell) -> bool {
        match self.cells.get_mut(index_in_chunk(tile)) {
            Some(slot) if *slot != cell => {
                *slot = cell;
                true
            }
            _ => false,
        }
    }

    /// The cells, row by row.
    pub fn cells(&self) -> &[Cell; CHUNK_AREA] {
        &self.cells
    }

    /// Whether every cell is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| c.is_empty())
    }

    /// The chunk's non-empty cells at `pos`, row by row, with their positions in the layer.
    pub fn placed_cells(&self, pos: ChunkPos) -> impl Iterator<Item = PlacedCell> + '_ {
        let origin = pos.origin();
        (0..CHUNK_SIZE)
            .flat_map(move |y| (0..CHUNK_SIZE).map(move |x| origin + IVec2::new(x, y)))
            .zip(self.cells.iter())
            .filter(|(_, cell)| !cell.is_empty())
            .map(|(at, &cell)| PlacedCell { at, cell })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{TileId, TilesetSlot};

    #[test]
    fn positions_floor_into_chunks_and_order_by_row() {
        assert_eq!(ChunkPos::of(IVec2::new(-1, 16)), ChunkPos { x: -1, y: 1 });
        assert_eq!(ChunkPos::of(IVec2::new(15, 0)), ChunkPos { x: 0, y: 0 });
        assert_eq!(ChunkPos { x: -1, y: 1 }.origin(), IVec2::new(-16, 16));
        let mut positions = vec![
            ChunkPos { x: 1, y: 0 },
            ChunkPos { x: 0, y: 1 },
            ChunkPos { x: 0, y: 0 },
        ];
        positions.sort();
        assert_eq!(
            positions,
            [
                ChunkPos { x: 0, y: 0 },
                ChunkPos { x: 1, y: 0 },
                ChunkPos { x: 0, y: 1 }
            ]
        );
    }

    #[test]
    fn cells_are_set_in_place() {
        let mut chunk = Chunk::default();
        let cell = Cell::new(TilesetSlot::new(0).unwrap(), TileId(3));
        assert!(chunk.set_cell(IVec2::new(-1, 17), cell));
        assert!(
            !chunk.set_cell(IVec2::new(15, 1), cell),
            "the same place, already set"
        );
        assert_eq!(chunk.cell(IVec2::new(15, 1)), cell);
        let placed: Vec<PlacedCell> = chunk.placed_cells(ChunkPos { x: -1, y: 1 }).collect();
        assert_eq!(
            placed,
            [PlacedCell {
                at: IVec2::new(-1, 17),
                cell
            }]
        );
        assert!(!chunk.is_empty());
    }
}
