//! Where a rule is tried. Tiled tries every rule at every position of the region; here each
//! input set has an anchor, its most selective "any of" condition, and a rule is tried only where
//! one of its sets' anchor tiles is. The engine indexes the positions of every anchor tile in
//! the grids (a bitset per tileset rejects the other cells at once), keeps the index up to date
//! as rules write when they match in order, and visits positions in Tiled's order: the region's
//! band rectangles one after the other, each row by row. A rule with a set that has no usable
//! anchor (only "none of" conditions, or ones accepting the empty cell) scans its lattice.

use std::collections::HashMap;

use bt_utils::IRect;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::engine::bind::TilesetBinding;
use crate::engine::matching::BoundSet;
use crate::engine::targets::Targets;
use crate::rulemap::RuleOptions;

/// Where a position comes in Tiled's scan: the band rectangle of the region it is in, its row
/// and its column.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ScanKey {
    /// The rectangle, in the region's band order.
    pub(crate) rect: usize,
    /// The row.
    pub(crate) y: i32,
    /// The column.
    pub(crate) x: i32,
}

impl ScanKey {
    /// The position.
    pub(crate) fn position(self) -> IVec2 {
        IVec2::new(self.x, self.y)
    }
}

/// The positions where a rule's input box overlaps the region by at least one cell (and fits
/// inside a finite map unless it may match outside): Tiled's `ruleMatchRegion`.
#[derive(Clone, Debug)]
pub(crate) struct MatchArea {
    /// The area's rectangles, in band order.
    rects: Vec<IRect>,
    /// Its cells, when it is more than one rectangle.
    cells: Option<CellSet>,
}

impl MatchArea {
    /// The area for a rule whose input box is `input_box` (relative, from the origin), on
    /// `region`, limited to `fit` when given.
    pub(crate) fn new(region: &CellSet, input_box: Bx, fit: Option<IRect>) -> MatchArea {
        // The box overlaps a cell of the region from that cell minus any of its offsets.
        let mut cells = region.dilated(input_box.negated());
        if let Some(fit) = fit {
            cells = cells.intersection_with_rect(fit);
        }
        let rects = cells.rects();
        let cells = (rects.len() > 1).then_some(cells);
        MatchArea { rects, cells }
    }

    /// The positions of one box, or none.
    pub(crate) fn of_box(area: Option<Bx>) -> MatchArea {
        MatchArea {
            rects: area.map(Bx::rect).into_iter().collect(),
            cells: None,
        }
    }

    /// Where `p` comes in the scan, if it is in the area.
    pub(crate) fn key(&self, p: IVec2) -> Option<ScanKey> {
        let rect = match &self.cells {
            None => self.rects.first().filter(|r| r.contains(p)).map(|_| 0)?,
            Some(cells) => {
                if !cells.contains(p) {
                    return None;
                }
                self.rects.iter().position(|r| r.contains(p))?
            }
        };
        Some(ScanKey {
            rect,
            y: p.y,
            x: p.x,
        })
    }

    /// The positions of the area outside `inside`.
    fn outside(&self, inside: IRect) -> Vec<IVec2> {
        match &self.cells {
            Some(cells) => cells.difference_with_rect(inside).cells().collect(),
            None => self
                .rects
                .iter()
                .flat_map(|&rect| rect_difference(rect, inside))
                .flat_map(|rect| rect.points())
                .collect(),
        }
    }

    /// Every position of the area on the lattice of `options`, in scan order.
    pub(crate) fn lattice(&self, options: &RuleOptions) -> impl Iterator<Item = ScanKey> + '_ {
        let (mod_x, mod_y) = (options.mod_x.max(1), options.mod_y.max(1));
        let (offset_x, offset_y) = (options.offset_x, options.offset_y);
        self.rects
            .iter()
            .enumerate()
            .flat_map(move |(index, rect)| {
                let first_x = rect.x + (offset_x - rect.x).rem_euclid(mod_x);
                let first_y = rect.y + (offset_y - rect.y).rem_euclid(mod_y);
                let rows = (first_y..rect.y + rect.h).step_by(usize::try_from(mod_y).unwrap_or(1));
                rows.flat_map(move |y| {
                    (first_x..rect.x + rect.w)
                        .step_by(usize::try_from(mod_x).unwrap_or(1))
                        .map(move |x| ScanKey { rect: index, y, x })
                })
            })
    }
}

/// The parts of `rect` outside `inside`: up to four rectangles, above, below, left and right.
fn rect_difference(rect: IRect, inside: IRect) -> Vec<IRect> {
    let both = rect.intersection(inside);
    if both.is_empty() {
        return vec![rect];
    }
    let parts = [
        IRect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: both.y - rect.y,
        },
        IRect {
            x: rect.x,
            y: both.y + both.h,
            w: rect.w,
            h: rect.y + rect.h - (both.y + both.h),
        },
        IRect {
            x: rect.x,
            y: both.y,
            w: both.x - rect.x,
            h: both.h,
        },
        IRect {
            x: both.x + both.w,
            y: both.y,
            w: rect.x + rect.w - (both.x + both.w),
            h: both.h,
        },
    ];
    parts.into_iter().filter(|r| !r.is_empty()).collect()
}

/// Whether `p` is on the lattice of `options`: `x ≡ OffsetX (mod ModX)` and the same for `y`, in
/// map coordinates.
pub(crate) fn on_lattice(p: IVec2, options: &RuleOptions) -> bool {
    (p.x - options.offset_x).rem_euclid(options.mod_x.max(1)) == 0
        && (p.y - options.offset_y).rem_euclid(options.mod_y.max(1)) == 0
}

/// A tile, as the run compares it: its key (the slot plus one) and its id.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AnchorTile {
    pub(crate) key: u32,
    pub(crate) tile: u32,
}

/// An input set's anchor: a position of one of its layers whose cell must be one of these
/// tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    /// The grid read.
    pub(crate) grid: usize,
    /// The position, relative to the rule's origin.
    pub(crate) offset: IVec2,
    /// The tiles, any of which may be there.
    pub(crate) tiles: Vec<AnchorTile>,
}

/// The "any of" conditions of `set` that can anchor it: on a layer the map has, naming tiles
/// and not the empty cell.
pub(crate) fn anchor_candidates(set: &BoundSet<'_>) -> Vec<Anchor> {
    let mut found = Vec::new();
    for (layer, &grid) in set.set.layers.iter().zip(&set.grids) {
        let Some(grid) = grid else {
            continue;
        };
        for position in &layer.positions {
            let any = set
                .cells
                .get(position.start..position.start + position.any)
                .unwrap_or_default();
            if any.is_empty() || any.iter().any(|c| c.is_empty()) {
                continue;
            }
            found.push(Anchor {
                grid,
                offset: position.offset,
                tiles: any
                    .iter()
                    .map(|c| AnchorTile {
                        key: c.key,
                        tile: c.tile,
                    })
                    .collect(),
            });
        }
    }
    found
}

/// An anchor tile on a grid.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct GridTile {
    grid: usize,
    tile: AnchorTile,
}

/// An anchor tile found at a cell.
#[derive(Copy, Clone, Debug)]
struct AnchorHit {
    tile: AnchorTile,
    at: IVec2,
}

/// The positions of the anchor tiles in the grids.
#[derive(Clone, Debug, Default)]
pub(crate) struct AnchorIndex {
    /// Per grid and key, a bitset of the tile ids anchors name.
    wanted: Vec<Vec<Vec<u64>>>,
    /// The positions of each anchor tile, per grid.
    positions: HashMap<GridTile, Vec<IVec2>>,
}

impl AnchorIndex {
    /// An index of the tiles `anchors` name, over the grids of `targets` as they are now,
    /// within `limit` when given.
    pub(crate) fn new<'a>(
        targets: &Targets,
        tilesets: &TilesetBinding,
        anchors: impl Iterator<Item = &'a Anchor>,
        limit: Option<IRect>,
    ) -> AnchorIndex {
        let mut index = AnchorIndex::default();
        let mut grids: Vec<usize> = Vec::new();
        for anchor in anchors {
            for tile in &anchor.tiles {
                index.mark_wanted(anchor.grid, *tile);
            }
            if !grids.contains(&anchor.grid) {
                grids.push(anchor.grid);
            }
        }
        for grid in grids {
            let Some(target) = targets.tiles.get(grid) else {
                continue;
            };
            let mut hits: Vec<AnchorHit> = Vec::new();
            target.grid.for_each_cell_within(limit, |at, cell| {
                let tile = AnchorTile {
                    key: tilesets.key_of(cell),
                    tile: cell.tile().0,
                };
                if index.is_wanted(grid, tile) {
                    hits.push(AnchorHit { tile, at });
                }
            });
            for hit in hits {
                index
                    .positions
                    .entry(GridTile {
                        grid,
                        tile: hit.tile,
                    })
                    .or_default()
                    .push(hit.at);
            }
        }
        index
    }

    fn mark_wanted(&mut self, grid: usize, tile: AnchorTile) {
        let (Ok(key), Ok(bit)) = (usize::try_from(tile.key), usize::try_from(tile.tile)) else {
            return;
        };
        if self.wanted.len() <= grid {
            self.wanted.resize(grid + 1, Vec::new());
        }
        let Some(keys) = self.wanted.get_mut(grid) else {
            return;
        };
        // An unmatchable key is never in a grid; it needs no bits.
        if tile.key == crate::engine::bind::UNMATCHABLE {
            return;
        }
        if keys.len() <= key {
            keys.resize(key + 1, Vec::new());
        }
        if let Some(bits) = keys.get_mut(key) {
            if bits.len() <= bit / 64 {
                bits.resize(bit / 64 + 1, 0);
            }
            if let Some(word) = bits.get_mut(bit / 64) {
                *word |= 1 << (bit % 64);
            }
        }
    }

    /// Whether some anchor names `tile` on `grid`.
    pub(crate) fn is_wanted(&self, grid: usize, tile: AnchorTile) -> bool {
        let (Ok(key), Ok(bit)) = (usize::try_from(tile.key), usize::try_from(tile.tile)) else {
            return false;
        };
        self.wanted
            .get(grid)
            .and_then(|keys| keys.get(key))
            .and_then(|bits| bits.get(bit / 64))
            .is_some_and(|word| word & (1 << (bit % 64)) != 0)
    }

    /// Where `tile` is on `grid` (positions may repeat after writes).
    pub(crate) fn hits(&self, grid: usize, tile: AnchorTile) -> &[IVec2] {
        self.positions
            .get(&GridTile { grid, tile })
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// How many cells an anchor's tiles are in.
    pub(crate) fn count(&self, anchor: &Anchor) -> usize {
        anchor
            .tiles
            .iter()
            .map(|t| self.hits(anchor.grid, *t).len())
            .sum()
    }

    /// Records that `tile` was written at `p` on `grid`, if an anchor names it.
    pub(crate) fn record_write(&mut self, grid: usize, tile: AnchorTile, p: IVec2) {
        if self.is_wanted(grid, tile) {
            self.positions
                .entry(GridTile { grid, tile })
                .or_default()
                .push(p);
        }
    }
}

/// The anchor of each input set: its condition whose tiles are in the fewest cells; `None` when
/// some set has no usable anchor, so the rule must scan its lattice.
pub(crate) fn choose_anchors(
    candidates: &[Vec<Anchor>],
    index: &AnchorIndex,
) -> Option<Vec<Anchor>> {
    candidates
        .iter()
        .map(|set| set.iter().min_by_key(|a| index.count(a)).cloned())
        .collect()
}

/// The positions where a rule with `anchors` can match, in scan order, once each.
///
/// With `OverflowBorder` or `WrapBorder`, a condition outside a finite map reads another cell
/// than the one at its offset, so anchors say nothing there: every position whose input box
/// leaves `inside` (where it fits in the map) is tried as well.
pub(crate) fn anchored_positions(
    anchors: &[Anchor],
    index: &AnchorIndex,
    area: &MatchArea,
    options: &RuleOptions,
    inside: Option<IRect>,
) -> Vec<ScanKey> {
    let mut keys: Vec<ScanKey> = Vec::new();
    for anchor in anchors {
        for tile in &anchor.tiles {
            for &q in index.hits(anchor.grid, *tile) {
                let p = q - anchor.offset;
                if on_lattice(p, options)
                    && let Some(key) = area.key(p)
                {
                    keys.push(key);
                }
            }
        }
    }
    if let Some(inside) = inside {
        for p in area.outside(inside) {
            if on_lattice(p, options)
                && let Some(key) = area.key(p)
            {
                keys.push(key);
            }
        }
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn the_area_is_where_the_input_box_overlaps_the_region() {
        // A 2×2 rule on a 5×5 map without MatchOutsideMap: positions 0..=3 on both axes.
        let region = CellSet::from_rect(rect(0, 0, 5, 5));
        let rule = Bx {
            min: IVec2::ZERO,
            max: IVec2::new(1, 1),
        };
        let area = MatchArea::new(&region, rule, Some(rect(0, 0, 4, 4)));
        assert_eq!(area.rects, [rect(0, 0, 4, 4)]);
        // Outside the map it reaches one cell further up and left.
        let outside = MatchArea::new(&region, rule, None);
        assert_eq!(outside.rects, [rect(-1, -1, 6, 6)]);
        assert_eq!(
            area.key(IVec2::new(3, 2)),
            Some(ScanKey {
                rect: 0,
                y: 2,
                x: 3
            })
        );
        assert_eq!(area.key(IVec2::new(4, 2)), None);
    }

    #[test]
    fn lattices_are_in_map_coordinates() {
        let region = CellSet::from_rect(rect(1, 0, 6, 1));
        let area = MatchArea::new(&region, Bx::point(IVec2::ZERO), None);
        let options = RuleOptions {
            mod_x: 3,
            offset_x: 1,
            ..RuleOptions::default()
        };
        let xs: Vec<i32> = area.lattice(&options).map(|k| k.x).collect();
        assert_eq!(xs, [1, 4]);
        assert!(on_lattice(IVec2::new(-2, 0), &options));
        assert!(!on_lattice(IVec2::new(2, 0), &options));
    }

    #[test]
    fn a_selection_is_scanned_band_by_band() {
        let mut region = CellSet::from_rect(rect(0, 0, 2, 3));
        region.insert_rect(rect(5, 1, 2, 2));
        let area = MatchArea::new(&region, Bx::point(IVec2::ZERO), None);
        let order: Vec<IVec2> = area
            .lattice(&RuleOptions::default())
            .map(ScanKey::position)
            .collect();
        assert_eq!(
            order[2..],
            [
                IVec2::new(0, 1),
                IVec2::new(1, 1),
                IVec2::new(0, 2),
                IVec2::new(1, 2),
                IVec2::new(5, 1),
                IVec2::new(6, 1),
                IVec2::new(5, 2),
                IVec2::new(6, 2)
            ]
        );
    }
}
