//! The cells of a layer during a run. Rules read and write grids, never the map: a grid holds a
//! dense copy of the layer over the area of interest, so reads there are an index; cells read
//! outside it come from the layer as it was, and cells written outside it are kept in a sparse
//! overflow. The end of the run writes back only the cells written that differ, so the map's
//! copy-on-write chunks stay shared wherever nothing changed.

use bt_core::{Cell, ChunkPos, PlacedCell, TileLayer};
use bt_utils::IRect;
use bt_utils::hash::CellHashMap;
use glam::IVec2;

/// The most cells a dense copy holds; beyond it a grid is all overflow.
const DENSE_LIMIT: i64 = 1 << 24;

/// The most chunks a limit spans for its chunks to be looked up one by one.
const SMALL_LIMIT_CHUNKS: i64 = 1024;

/// A layer's cells during a run.
#[derive(Clone, Debug)]
pub(crate) struct Grid {
    /// The layer's cells when the run started; `None` for a layer the run creates.
    base: Option<TileLayer>,
    /// Where the dense copy is.
    area: IRect,
    /// The cells inside `area`, row by row.
    dense: Vec<Cell>,
    /// Which cells of `area` were written.
    dirty: Vec<bool>,
    /// The cells written outside `area`.
    overflow: CellHashMap<IVec2, Cell>,
}

impl Grid {
    /// A grid of `base` (or of an empty layer) with a dense copy over `area`; an area too large
    /// to copy is left sparse.
    pub(crate) fn new(base: Option<TileLayer>, area: IRect) -> Grid {
        let cells = i64::from(area.w.max(0)) * i64::from(area.h.max(0));
        let area = if area.is_empty() || cells > DENSE_LIMIT {
            IRect::default()
        } else {
            area
        };
        let size = usize::try_from(i64::from(area.w.max(0)) * i64::from(area.h.max(0)))
            .unwrap_or_default();
        let mut grid = Grid {
            base,
            area,
            dense: vec![Cell::EMPTY; size],
            dirty: vec![false; size],
            overflow: CellHashMap::default(),
        };
        if let Some(base) = &grid.base {
            for chunk in base.chunks() {
                if chunk.pos.rect().intersection(area).is_empty() {
                    continue;
                }
                for placed in chunk.chunk.placed_cells(chunk.pos) {
                    if let Some(i) = index_in(area, placed.at)
                        && let Some(slot) = grid.dense.get_mut(i)
                    {
                        *slot = placed.cell;
                    }
                }
            }
        }
        grid
    }

    /// The cell at `p` now.
    pub(crate) fn cell(&self, p: IVec2) -> Cell {
        if let Some(i) = index_in(self.area, p) {
            return self.dense.get(i).copied().unwrap_or_default();
        }
        if let Some(cell) = self.overflow.get(&p) {
            return *cell;
        }
        self.before(p)
    }

    /// The cell at `p` when the run started.
    pub(crate) fn before(&self, p: IVec2) -> Cell {
        self.base.as_ref().map_or(Cell::EMPTY, |base| base.cell(p))
    }

    /// Writes `cell` at `p` and returns what was there.
    pub(crate) fn set_cell(&mut self, p: IVec2, cell: Cell) -> Cell {
        if let Some(i) = index_in(self.area, p)
            && let (Some(slot), Some(dirty)) = (self.dense.get_mut(i), self.dirty.get_mut(i))
        {
            *dirty = true;
            return std::mem::replace(slot, cell);
        }
        let before = self.cell(p);
        self.overflow.insert(p, cell);
        before
    }

    /// Calls `visit` with every cell that holds a tile now.
    pub(crate) fn for_each_cell(&self, mut visit: impl FnMut(IVec2, Cell)) {
        for (i, &cell) in self.dense.iter().enumerate() {
            if !cell.is_empty() {
                visit(position_of(self.area, i), cell);
            }
        }
        for (&p, &cell) in &self.overflow {
            if !cell.is_empty() {
                visit(p, cell);
            }
        }
        if let Some(base) = &self.base {
            for chunk in base.chunks() {
                let rect = chunk.pos.rect();
                if rect.intersection(self.area) == rect {
                    continue;
                }
                for placed in chunk.chunk.placed_cells(chunk.pos) {
                    if !self.area.contains(placed.at) && !self.overflow.contains_key(&placed.at) {
                        visit(placed.at, placed.cell);
                    }
                }
            }
        }
    }

    /// Calls `visit` with every cell inside `limit` (every cell when `None`) that holds a tile
    /// now; a region run visits its windows only.
    pub(crate) fn for_each_cell_within(
        &self,
        limit: Option<IRect>,
        mut visit: impl FnMut(IVec2, Cell),
    ) {
        let Some(limit) = limit else {
            self.for_each_cell(visit);
            return;
        };
        let dense = self.area.intersection(limit);
        for y in dense.y..dense.y + dense.h {
            let start = index_in(self.area, IVec2::new(dense.x, y));
            let row = start.and_then(|start| {
                let width = usize::try_from(dense.w).ok()?;
                self.dense.get(start..start + width)
            });
            for (x, &cell) in (dense.x..).zip(row.unwrap_or_default()) {
                if !cell.is_empty() {
                    visit(IVec2::new(x, y), cell);
                }
            }
        }
        for (&p, &cell) in &self.overflow {
            if limit.contains(p) && !cell.is_empty() {
                visit(p, cell);
            }
        }
        let Some(base) = &self.base else {
            return;
        };
        let outside_dense = |placed: &PlacedCell| {
            limit.contains(placed.at)
                && !self.area.contains(placed.at)
                && !self.overflow.contains_key(&placed.at)
        };
        let first = ChunkPos::of(limit.min());
        let last = ChunkPos::of(limit.max() - IVec2::ONE);
        let spanned = i64::from(last.x - first.x + 1) * i64::from(last.y - first.y + 1);
        // A small limit looks its chunks up; a large one walks the layer's chunks.
        if spanned <= SMALL_LIMIT_CHUNKS {
            for y in first.y..=last.y {
                for x in first.x..=last.x {
                    let pos = ChunkPos { x, y };
                    if pos.rect().intersection(self.area) == pos.rect() {
                        continue;
                    }
                    if let Some(chunk) = base.chunk(pos) {
                        for placed in chunk.placed_cells(pos).filter(outside_dense) {
                            visit(placed.at, placed.cell);
                        }
                    }
                }
            }
        } else {
            for chunk in base.chunks() {
                let rect = chunk.pos.rect();
                if rect.intersection(limit).is_empty() || rect.intersection(self.area) == rect {
                    continue;
                }
                for placed in chunk.chunk.placed_cells(chunk.pos).filter(outside_dense) {
                    visit(placed.at, placed.cell);
                }
            }
        }
    }

    /// Every cell written, with what it holds now, once each.
    pub(crate) fn written(&self) -> Vec<PlacedCell> {
        let mut cells: Vec<PlacedCell> = self
            .dirty
            .iter()
            .enumerate()
            .filter(|&(_, &dirty)| dirty)
            .map(|(i, _)| PlacedCell {
                at: position_of(self.area, i),
                cell: self.dense.get(i).copied().unwrap_or_default(),
            })
            .collect();
        cells.extend(
            self.overflow
                .iter()
                .map(|(&at, &cell)| PlacedCell { at, cell }),
        );
        cells
    }
}

/// The index of `p` in the dense copy of `area`, if it is inside.
fn index_in(area: IRect, p: IVec2) -> Option<usize> {
    if !area.contains(p) {
        return None;
    }
    let local = p - area.min();
    usize::try_from(i64::from(local.y) * i64::from(area.w) + i64::from(local.x)).ok()
}

/// The position of index `i` in the dense copy of `area`.
fn position_of(area: IRect, i: usize) -> IVec2 {
    let width = usize::try_from(area.w.max(1)).unwrap_or(1);
    let x = i32::try_from(i % width).unwrap_or_default();
    let y = i32::try_from(i / width).unwrap_or_default();
    area.min() + IVec2::new(x, y)
}

#[cfg(test)]
mod tests {
    use bt_core::{TileId, TilesetSlot};
    use glam::UVec2;

    use super::*;

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    fn layer() -> TileLayer {
        let mut layer = TileLayer::infinite(UVec2::ZERO);
        layer.set_cell(IVec2::new(1, 1), tile(1)).unwrap();
        layer.set_cell(IVec2::new(40, -3), tile(2)).unwrap();
        layer
    }

    #[test]
    fn reads_come_from_the_copy_the_overflow_or_the_layer() {
        let mut grid = Grid::new(
            Some(layer()),
            IRect {
                x: 0,
                y: 0,
                w: 4,
                h: 4,
            },
        );
        assert_eq!(grid.cell(IVec2::new(1, 1)), tile(1));
        assert_eq!(grid.cell(IVec2::new(40, -3)), tile(2), "outside the copy");
        assert_eq!(grid.set_cell(IVec2::new(40, -3), tile(5)), tile(2));
        assert_eq!(grid.cell(IVec2::new(40, -3)), tile(5));
        assert_eq!(grid.before(IVec2::new(40, -3)), tile(2));
        assert_eq!(grid.set_cell(IVec2::new(2, 2), tile(3)), Cell::EMPTY);
        let mut written = grid.written();
        written.sort_by_key(|placed| (placed.at.y, placed.at.x));
        let written: Vec<(IVec2, Cell)> = written.iter().map(|p| (p.at, p.cell)).collect();
        assert_eq!(
            written,
            [(IVec2::new(40, -3), tile(5)), (IVec2::new(2, 2), tile(3))]
        );
        let mut all = Vec::new();
        grid.for_each_cell(|p, cell| all.push((p, cell)));
        all.sort_by_key(|(p, _)| (p.y, p.x));
        assert_eq!(
            all,
            [
                (IVec2::new(40, -3), tile(5)),
                (IVec2::new(1, 1), tile(1)),
                (IVec2::new(2, 2), tile(3))
            ]
        );
    }

    #[test]
    fn a_window_visits_the_copy_the_overflow_and_the_layer_inside_it_only() {
        let mut layer = layer();
        layer.set_cell(IVec2::new(-20, 2), tile(4)).unwrap();
        let mut grid = Grid::new(
            Some(layer),
            IRect {
                x: 0,
                y: 0,
                w: 4,
                h: 4,
            },
        );
        grid.set_cell(IVec2::new(3, 3), tile(3));
        grid.set_cell(IVec2::new(-19, 2), tile(5));
        let within = |limit: IRect| {
            let mut seen = Vec::new();
            grid.for_each_cell_within(Some(limit), |p, cell| seen.push((p, cell)));
            seen.sort_by_key(|(p, _)| (p.y, p.x));
            seen
        };
        assert_eq!(
            within(IRect {
                x: 1,
                y: 1,
                w: 3,
                h: 3
            }),
            [(IVec2::new(1, 1), tile(1)), (IVec2::new(3, 3), tile(3))]
        );
        // Outside the copy: the layer's cells and the overflow, which covers them.
        assert_eq!(
            within(IRect {
                x: -21,
                y: 0,
                w: 4,
                h: 4
            }),
            [(IVec2::new(-20, 2), tile(4)), (IVec2::new(-19, 2), tile(5))]
        );
        // A huge limit walks the layer's chunks instead of looking them up.
        let mut all = Vec::new();
        grid.for_each_cell_within(
            Some(IRect {
                x: -100_000,
                y: -100_000,
                w: 200_000,
                h: 200_000,
            }),
            |p, cell| all.push((p, cell)),
        );
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn a_new_layer_starts_empty_and_a_huge_area_stays_sparse() {
        let mut grid = Grid::new(
            None,
            IRect {
                x: 0,
                y: 0,
                w: 1 << 13,
                h: 1 << 13,
            },
        );
        let count = |grid: &Grid| {
            let mut count = 0;
            grid.for_each_cell(|_, _| count += 1);
            count
        };
        assert_eq!(count(&grid), 0);
        grid.set_cell(IVec2::new(5, 5), tile(1));
        assert_eq!(count(&grid), 1);
        assert_eq!(grid.cell(IVec2::new(5, 5)), tile(1));
        assert_eq!(
            index_in(
                IRect {
                    x: -2,
                    y: -2,
                    w: 4,
                    h: 4
                },
                IVec2::new(1, 1)
            ),
            Some(15)
        );
        assert_eq!(
            position_of(
                IRect {
                    x: -2,
                    y: -2,
                    w: 4,
                    h: 4
                },
                15
            ),
            IVec2::new(1, 1)
        );
    }
}
