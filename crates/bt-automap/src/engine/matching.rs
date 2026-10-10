//! Whether a rule holds at a position, as Tiled's `matchRuleAtOffset` decides: any of its input
//! sets holds, and a set holds when at each of its positions the cell is one of the "any of"
//! tiles (if it names any) and none of the "none of" tiles. Cells outside a finite map read as
//! empty, or as the nearest cell inside (`OverflowBorder`), or as the cell the map wraps to
//! (`WrapBorder`); a layer the map lacks reads as empty everywhere.

use bt_core::Cell;
use glam::{IVec2, UVec2};

use crate::engine::bind::{BoundCell, TilesetBinding};
use crate::engine::targets::{TargetOrigin, Targets};
use crate::observer::{Check, Condition, Observer};
use crate::rule_ref::RuleRef;
use crate::rulemap::InputSet;

/// How cells outside a finite map are read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Border {
    /// As empty cells.
    Empty,
    /// As the nearest cell inside (`OverflowBorder`).
    Overflow,
    /// As the cell the map wraps to (`WrapBorder`).
    Wrap,
}

/// A cell read for a condition: where it was read and what is there.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReadCell {
    /// The cell read: the condition's, moved inside the map by the border.
    pub(crate) at: IVec2,
    /// What it holds.
    pub(crate) cell: Cell,
}

/// The size of a finite layer as positions, or `None` when it holds nothing.
fn extent(size: UVec2) -> Option<IVec2> {
    let w = i32::try_from(size.x).ok().filter(|&w| w > 0)?;
    let h = i32::try_from(size.y).ok().filter(|&h| h > 0)?;
    Some(IVec2::new(w, h))
}

/// Reads the cell of `grid` (a layer of the run, or none) at `p`.
pub(crate) fn read(targets: &Targets, grid: Option<usize>, p: IVec2, border: Border) -> ReadCell {
    let empty = ReadCell {
        at: p,
        cell: Cell::EMPTY,
    };
    let Some(target) = grid.and_then(|g| targets.tiles.get(g)) else {
        return empty;
    };
    let Some(size) = target.size else {
        return ReadCell {
            at: p,
            cell: target.grid.cell(p),
        };
    };
    let Some(extent) = extent(size) else {
        return empty;
    };
    let at = match border {
        Border::Wrap => IVec2::new(p.x.rem_euclid(extent.x), p.y.rem_euclid(extent.y)),
        Border::Overflow => p.clamp(IVec2::ZERO, extent - IVec2::ONE),
        Border::Empty => {
            if p.cmplt(IVec2::ZERO).any() || p.cmpge(extent).any() {
                return empty;
            }
            p
        }
    };
    ReadCell {
        at,
        cell: target.grid.cell(at),
    }
}

/// An input set bound for a run: its layers' grids and its cells in the run's numbering.
#[derive(Clone, Debug)]
pub(crate) struct BoundSet<'r> {
    /// The set's index in its rule.
    pub(crate) index: usize,
    /// The compiled set.
    pub(crate) set: &'r InputSet,
    /// For each of its layers, the grid read; `None` when the map has no such layer.
    pub(crate) grids: Vec<Option<usize>>,
    /// Its cells, bound, in the order of the set's cells.
    pub(crate) cells: Vec<BoundCell>,
    /// Whether it can hold: `false` for a set needing a tile on a layer the map lacks, which an
    /// observer still sees checked.
    pub(crate) possible: bool,
}

/// What reading conditions needs.
pub(crate) struct Reader<'a> {
    pub(crate) targets: &'a Targets,
    pub(crate) tilesets: &'a TilesetBinding,
    pub(crate) border: Border,
}

impl Reader<'_> {
    /// Whether `set` holds at `p`, stopping at the first condition that fails.
    pub(crate) fn holds(&self, set: &BoundSet<'_>, p: IVec2) -> bool {
        for (layer, &grid) in set.set.layers.iter().zip(&set.grids) {
            for position in &layer.positions {
                let read = read(self.targets, grid, p + position.offset, self.border);
                let key = self.tilesets.key_of(read.cell);
                let start = position.start;
                let any = set
                    .cells
                    .get(start..start + position.any)
                    .unwrap_or_default();
                if !any.is_empty() && !any.iter().any(|c| c.matches(key, read.cell)) {
                    return false;
                }
                let none_start = start + position.any;
                let none = set
                    .cells
                    .get(none_start..none_start + position.none)
                    .unwrap_or_default();
                if none.iter().any(|c| c.matches(key, read.cell)) {
                    return false;
                }
            }
        }
        true
    }

    /// Whether `set` holds at `p`, checking every condition and reporting each to `observer`.
    pub(crate) fn holds_observed<O: Observer>(
        &self,
        set: &BoundSet<'_>,
        rule: RuleRef,
        p: IVec2,
        input_names: &[String],
        observer: &mut O,
    ) -> bool {
        let mut holds = true;
        for (layer, &grid) in set.set.layers.iter().zip(&set.grids) {
            let name = input_names
                .get(layer.target)
                .map(String::as_str)
                .unwrap_or_default();
            let present = grid
                .and_then(|g| self.targets.tiles.get(g))
                .is_some_and(|t| matches!(t.origin, TargetOrigin::Existing(_)));
            for position in &layer.positions {
                let read = read(self.targets, grid, p + position.offset, self.border);
                let key = self.tilesets.key_of(read.cell);
                let before = grid
                    .and_then(|g| self.targets.tiles.get(g))
                    .map_or(read.cell, |t| t.grid.before(read.at));
                let start = position.start;
                let none_start = start + position.any;
                let mut report = |condition, cells: &[BoundCell], from: usize, count: usize| {
                    let found = cells.iter().any(|c| c.matches(key, read.cell));
                    let ok = match condition {
                        Condition::AnyOf => found,
                        Condition::NoneOf => !found,
                    };
                    observer.check(&Check {
                        rule,
                        position: p,
                        input_set: set.index,
                        layer: name,
                        layer_present: present,
                        at: read.at,
                        condition,
                        tiles: set.set.cells.get(from..from + count).unwrap_or_default(),
                        found: read.cell,
                        before,
                        holds: ok,
                    });
                    ok
                };
                let any = set.cells.get(start..none_start).unwrap_or_default();
                if !any.is_empty() && !report(Condition::AnyOf, any, start, position.any) {
                    holds = false;
                }
                let none = set
                    .cells
                    .get(none_start..none_start + position.none)
                    .unwrap_or_default();
                if !none.is_empty() && !report(Condition::NoneOf, none, none_start, position.none) {
                    holds = false;
                }
            }
        }
        holds
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Map, TileId, TileLayer, TilesetSlot};
    use bt_utils::IRect;

    use super::*;
    use crate::engine::request::LayerNames;

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    fn targets() -> (Targets, Option<usize>) {
        let mut layer = TileLayer::finite(UVec2::new(3, 2));
        layer.set_cell(IVec2::new(0, 0), tile(1)).unwrap();
        layer.set_cell(IVec2::new(2, 1), tile(2)).unwrap();
        let map = Map {
            width: 3,
            height: 2,
            layers: vec![bt_core::Layer::new(
                bt_core::LayerId(1),
                "set",
                bt_core::LayerKind::Tile(layer),
            )],
            ..Map::default()
        };
        let mut targets = Targets::new(IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
        });
        let grid = targets.tile_input(&map, &LayerNames::by_name(), "set");
        (targets, grid)
    }

    #[test]
    fn cells_outside_the_map_read_as_the_border_says() {
        let (targets, grid) = targets();
        let at = |p: IVec2, border| {
            let r = read(&targets, grid, p, border);
            (r.at, r.cell)
        };
        assert_eq!(
            at(IVec2::new(-1, 0), Border::Empty),
            (IVec2::new(-1, 0), Cell::EMPTY)
        );
        assert_eq!(
            at(IVec2::new(-1, 0), Border::Overflow),
            (IVec2::new(0, 0), tile(1))
        );
        assert_eq!(
            at(IVec2::new(-1, -1), Border::Wrap),
            (IVec2::new(2, 1), tile(2))
        );
        assert_eq!(
            at(IVec2::new(5, 1), Border::Wrap),
            (IVec2::new(2, 1), tile(2))
        );
        assert_eq!(
            read(&targets, None, IVec2::ZERO, Border::Wrap).cell,
            Cell::EMPTY
        );
    }
}
