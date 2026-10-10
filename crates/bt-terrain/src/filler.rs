//! The solver: Tiled's `WangFiller` (`reference/tiled/src/tiled/wangfiller.cpp`), greedy, one
//! pass over the region in band order, hard lanes propagated to neighbours, a correction queue
//! for the brush and a one-step lookahead for fills, with no backtracking. Every random choice is
//! drawn from a generator derived from the seed and the cell, so a preview is the paint; and a
//! cell no tile fits comes back with what it needed.

use std::collections::{HashMap, HashSet};

use bt_core::{Cell, PlacedCell, TileLayer, WangSetType};
use bt_utils::rng::{RandomPicker, cell_rng};
use bt_utils::{IRect, bands::banded_rects};
use glam::{IVec2, UVec2};

use crate::diagnose::{Cause, Needed, pattern_of};
use crate::request::BrushTarget;
use crate::terrain::{Candidate, Terrain};
use crate::topology::Topology;
use crate::wangid::{
    CORNER_LANES, EDGE_LANES, LANE_COUNT, PackedWangId, UNSET, is_corner, kind_mask, lane_of, next,
    opposite, previous, side_mask,
};

/// What the solver wants of one cell: the terrain of each lane (`UNSET` for no wish), and which
/// of those lanes it must have.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellInfo {
    pub(crate) wanted: PackedWangId,
    pub(crate) mask: PackedWangId,
}

impl Default for CellInfo {
    fn default() -> CellInfo {
        CellInfo {
            wanted: PackedWangId::FULL,
            mask: PackedWangId::ZERO,
        }
    }
}

/// What a run decided, in row-major order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Outcome {
    pub(crate) placed: Vec<PlacedCell>,
    pub(crate) invalid: Vec<IVec2>,
    pub(crate) needed: Vec<Needed>,
}

/// A run of the solver over one tile layer.
pub(crate) struct Filler<'a> {
    terrain: &'a Terrain,
    back: &'a TileLayer,
    topology: &'a Topology,
    corrections: bool,
    erasing: bool,
    seed: u64,
    grid: HashMap<IVec2, CellInfo>,
    region: HashSet<IVec2>,
}

/// The state a run builds while it resolves cells.
#[derive(Default)]
struct Solve {
    /// Cells that are settled: placed, or outside a finite layer.
    checked: HashSet<IVec2>,
    placed: HashMap<IVec2, Cell>,
    invalid: HashMap<IVec2, Needed>,
    queue: Vec<IVec2>,
    /// Whether some tile has a pattern, by the pattern's lanes and mask.
    used: HashMap<(PackedWangId, PackedWangId), bool>,
}

/// A candidate the lookahead turned down, and why.
struct Rejection {
    lane: usize,
    pattern: [Option<u8>; LANE_COUNT],
}

impl<'a> Filler<'a> {
    /// A solver over `back` with the set `terrain`. With `corrections` it is the brush (cells
    /// around what was painted are re-solved when their tiles no longer fit); without, a fill
    /// (border cells prefer their surroundings and choices look one step ahead). `erasing` lets a
    /// cell be left empty where that fits best.
    pub(crate) fn new(
        terrain: &'a Terrain,
        back: &'a TileLayer,
        topology: &'a Topology,
        corrections: bool,
        erasing: bool,
        seed: u64,
    ) -> Filler<'a> {
        Filler {
            terrain,
            back,
            topology,
            corrections,
            erasing,
            seed,
            grid: HashMap::new(),
            region: HashSet::new(),
        }
    }

    /// The cell's wishes, made from its current tile when it has none yet; the cell joins the
    /// region (Tiled's `changePosition`).
    fn change_position(&mut self, p: IVec2) -> &mut CellInfo {
        let terrain = self.terrain;
        let back = self.back;
        let region = &mut self.region;
        let info = self.grid.entry(p).or_default();
        if *info == CellInfo::default() {
            info.wanted = terrain.packed_wang_of_cell(back.cell(p));
            region.insert(p);
        }
        info
    }

    /// Lane `lane` of cell `p` must be `color` (Tiled's `setWangIndex`).
    fn set_lane(&mut self, p: IVec2, lane: usize, color: u8) {
        let info = self.change_position(p);
        info.wanted.set_lane(lane, color);
        info.mask.set_lane(lane, UNSET);
    }

    /// The four corners meeting at the vertex at the top-left of cell `vertex` must be `color`.
    fn set_corner(&mut self, vertex: IVec2, color: u8) {
        for at in self.topology.around_vertex(vertex) {
            self.set_lane(at.cell, at.lane, color);
        }
    }

    /// Edge `lane` of cell `p`, and the matching edge of the cell across it, must be `color`.
    fn set_edge(&mut self, p: IVec2, lane: usize, color: u8) {
        self.set_lane(p, lane, color);
        if let Some(across) = self.topology.across_edge(p, lane) {
            self.set_lane(across, opposite(lane), color);
        }
    }

    /// The whole tile at `p` must be `color` on the lanes `kind` paints, and its neighbours must
    /// match it where they touch (Tiled's "fill full tiles" in `WangBrush::updateBrushAt`).
    fn set_tile(&mut self, p: IVec2, color: u8, kind: WangSetType) {
        let around = self.topology.around(p);
        let lanes: &[usize] = match kind {
            WangSetType::Corner => &CORNER_LANES,
            WangSetType::Edge => &EDGE_LANES,
            WangSetType::Mixed => &[0, 1, 2, 3, 4, 5, 6, 7],
        };
        {
            let center = self.change_position(p);
            for &lane in lanes {
                center.wanted.set_lane(lane, color);
                center.mask.set_lane(lane, UNSET);
            }
        }
        for (lane, &q) in around.iter().enumerate() {
            let corner = is_corner(lane);
            if kind == WangSetType::Edge && corner {
                continue;
            }
            let adjacent = self.change_position(q);
            if corner || kind != WangSetType::Corner {
                adjacent.wanted.set_lane(opposite(lane), color);
                adjacent.mask.set_lane(opposite(lane), UNSET);
            }
            if !corner && kind != WangSetType::Edge {
                for touching in [(lane + 3) % LANE_COUNT, (lane + 5) % LANE_COUNT] {
                    adjacent.wanted.set_lane(touching, color);
                    adjacent.mask.set_lane(touching, UNSET);
                }
            }
        }
    }

    /// Paints one brush target with `color`; `kind` is how the brush paints that terrain.
    pub(crate) fn set_target(&mut self, target: BrushTarget, color: u8, kind: WangSetType) {
        match target {
            BrushTarget::Corner(vertex) => self.set_corner(vertex, color),
            BrushTarget::Edge { cell, side } => self.set_edge(cell, side.index(), color),
            BrushTarget::Tile(cell) => self.set_tile(cell, color, kind),
        }
    }

    /// Adds each region cell's 180° turn around the centre of a map of `size`, with its wishes
    /// turned too (Tiled's rotational symmetry in `WangBrush::updateBrush`).
    pub(crate) fn mirror_region(&mut self, size: UVec2) {
        let (w, h) = (
            i32::try_from(size.x).unwrap_or(i32::MAX),
            i32::try_from(size.y).unwrap_or(i32::MAX),
        );
        for rect in banded_rects(self.region.iter().copied()) {
            for p in rect.points() {
                let source = self.grid.get(&p).copied().unwrap_or_default();
                let wanted = source.wanted.rotated(2);
                let mask = source.mask.rotated(2);
                let target = self.change_position(IVec2::new(w - p.x - 1, h - p.y - 1));
                target.wanted = target.wanted.merged_with(wanted, mask);
                target.mask = target.mask.merged_with(mask, mask);
            }
        }
    }

    /// The cells a fill covers (Tiled's `setRegion`: no wishes of their own).
    pub(crate) fn set_region(&mut self, cells: &[IVec2]) {
        self.region.extend(cells.iter().copied());
    }

    /// Every region cell softly prefers `color` on the lanes `kind` paints: a fill with a chosen
    /// terrain, which Tiled's fill does not have.
    pub(crate) fn prefer_color(&mut self, color: u8, kind: WangSetType) {
        let lanes = kind_mask(kind).masked(self.terrain.kind_mask()).lanes();
        let cells: Vec<IVec2> = self.region.iter().copied().collect();
        for p in cells {
            let info = self.grid.entry(p).or_default();
            for (lane, &m) in lanes.iter().enumerate() {
                if m == UNSET && info.mask.lane(lane) != UNSET {
                    info.wanted.set_lane(lane, color);
                }
            }
        }
    }

    /// Resolves the region and, for the brush, the corrections around it.
    pub(crate) fn run(mut self) -> Outcome {
        let mut solve = Solve::default();
        // Cells outside a finite layer are settled as they are and take no further part.
        if !self.back.is_infinite() {
            let outside: Vec<IVec2> = self
                .region
                .iter()
                .copied()
                .filter(|p| !self.back.contains(*p))
                .collect();
            for p in outside {
                self.region.remove(&p);
                solve.checked.insert(p);
            }
        }
        let bands = banded_rects(self.region.iter().copied());
        if !self.corrections {
            for rect in &bands {
                for x in rect.x..rect.x + rect.w {
                    self.prefer_surroundings(IVec2::new(x, rect.y), 0);
                    self.prefer_surroundings(IVec2::new(x, rect.y + rect.h - 1), 4);
                }
                for y in rect.y..rect.y + rect.h {
                    self.prefer_surroundings(IVec2::new(rect.x, y), 6);
                    self.prefer_surroundings(IVec2::new(rect.x + rect.w - 1, y), 2);
                }
            }
        }
        let bounds = self.correction_bounds();
        for rect in &bands {
            for p in rect.points() {
                self.resolve(&mut solve, p, bounds);
            }
        }
        while !solve.queue.is_empty() {
            let batch = std::mem::take(&mut solve.queue);
            for p in batch {
                self.resolve(&mut solve, p, bounds);
            }
        }
        outcome_of(solve)
    }

    /// The area the brush may re-solve: the region's bounds grown by the set's maximum terrain
    /// distance (one more on staggered and hexagonal grids), within a finite layer.
    fn correction_bounds(&self) -> IRect {
        let Some(region) = IRect::bounding(self.region.iter().copied()) else {
            return IRect::default();
        };
        let margin = self.terrain.raw_max_distance() + i32::from(self.topology.is_hex());
        let grown = IRect::from_min_size(
            region.min() - IVec2::splat(margin),
            region.size() + IVec2::splat(2 * margin),
        );
        if self.back.is_infinite() {
            return grown;
        }
        let size = self.back.size();
        let layer = IRect::from_min_size(
            IVec2::ZERO,
            IVec2::new(
                i32::try_from(size.x).unwrap_or(i32::MAX),
                i32::try_from(size.y).unwrap_or(i32::MAX),
            ),
        );
        grown.intersection(layer)
    }

    /// A border cell of a fill prefers, on the lanes of its `side` it has no rule for, what the
    /// tiles around the region show it.
    fn prefer_surroundings(&mut self, p: IVec2, side: usize) {
        let surroundings = self.wang_from_surroundings(p);
        let info = self.grid.entry(p).or_default();
        let excluded = info.mask.union(surroundings.mask_of_value(UNSET));
        let mask = side_mask(side).masked(excluded.inverted());
        info.wanted = info.wanted.merged_with(surroundings, mask);
    }

    /// What the tiles around `p` outside the region show it (Tiled's `wangIdFromSurroundings`):
    /// edges from the neighbour's opposite edge, corners from the diagonal neighbour or, failing
    /// that, from the side neighbours' touching corners.
    fn wang_from_surroundings(&self, p: IVec2) -> PackedWangId {
        let around = self.topology.around(p);
        let ids: [PackedWangId; LANE_COUNT] = std::array::from_fn(|lane| {
            let q = around.get(lane).copied().unwrap_or(p);
            let cell = self.back.cell(q);
            if cell.is_empty() || self.region.contains(&q) {
                PackedWangId::FULL
            } else {
                self.terrain.packed_wang_of_cell(cell)
            }
        });
        let id_at = |lane: usize| ids.get(lane).copied().unwrap_or(PackedWangId::FULL);
        let mut id = PackedWangId::FULL;
        for lane in EDGE_LANES {
            id.set_lane(lane, id_at(lane).lane(opposite(lane)));
        }
        for lane in CORNER_LANES {
            let mut color = id_at(lane).lane(opposite(lane));
            if color == UNSET || color == 0 {
                let left = id_at(previous(lane)).lane((lane + 2) % LANE_COUNT);
                if left != UNSET {
                    color = left;
                }
            }
            if color == UNSET || color == 0 {
                let right = id_at(next(lane)).lane((lane + 6) % LANE_COUNT);
                if right != UNSET {
                    color = right;
                }
            }
            id.set_lane(lane, color);
        }
        id
    }

    /// Places the best tile at `p`, or records what it needed; then hands its lanes to the
    /// unsettled neighbours and queues the brush's corrections.
    fn resolve(&mut self, solve: &mut Solve, p: IVec2, bounds: IRect) {
        if solve.checked.contains(&p) {
            return;
        }
        let cell = match self.find_best(solve, p) {
            Ok(cell) => cell,
            Err(needed) => {
                solve.invalid.entry(p).or_insert(needed);
                return;
            }
        };
        solve.checked.insert(p);
        solve.placed.insert(p, cell);
        solve.invalid.remove(&p);
        let wang = self.terrain.packed_wang_of_cell(cell);
        for (lane, &q) in self.topology.around(p).iter().enumerate() {
            if solve.checked.contains(&q) {
                continue;
            }
            let info = self.grid.entry(q).or_default();
            update_adjacent(info, wang, lane);
            if !is_corner(lane)
                && self.corrections
                && bounds.contains(q)
                && !self.region.contains(&q)
            {
                let current = self.terrain.packed_wang_of_cell(self.back.cell(q));
                if current.masked(info.mask) != info.wanted.masked(info.mask) {
                    solve.queue.push(q);
                    for l in 0..LANE_COUNT {
                        if info.mask.lane(l) == 0 {
                            info.wanted.set_lane(l, current.lane(l));
                        }
                    }
                }
            }
        }
    }

    /// The best tile for `p` (Tiled's `findBestMatch`): among the tiles that agree with the
    /// lanes the cell must have, those with the lowest total distance from its wishes, drawn by
    /// weight; for fills, the first drawn that leaves every unsettled neighbour some tile.
    fn find_best(&self, solve: &mut Solve, p: IVec2) -> Result<Cell, Needed> {
        let terrain = self.terrain;
        let info = self.grid.get(&p).copied().unwrap_or_default();
        let required = info.wanted.masked(info.mask);
        let unreachable = terrain.raw_max_distance() + 1;
        let erase = Candidate {
            cell: Cell::EMPTY,
            wang: PackedWangId::ZERO,
            weight: 1.0,
        };
        let mut picker: RandomPicker<Candidate> = RandomPicker::new();
        let mut lowest: Option<i32> = None;
        let mut agrees = false;
        let mut stuck = PackedWangId::ZERO;
        let mut zero_weight: Option<PackedWangId> = None;
        let erasing = self.erasing.then_some(erase);
        for candidate in terrain.candidates().iter().copied().chain(erasing) {
            if candidate.wang.masked(info.mask) != required {
                continue;
            }
            agrees = true;
            let mut total = 0;
            let mut rejected = false;
            for lane in 0..LANE_COUNT {
                let wanted = info.wanted.lane(lane);
                let has = candidate.wang.lane(lane);
                if wanted == UNSET || wanted == has {
                    continue;
                }
                let mut penalty = terrain.raw_penalty(wanted, has);
                if penalty < 0 {
                    if self.corrections {
                        // No tiles lead from what this one has to what the cell wants.
                        stuck.set_lane(lane, UNSET);
                        rejected = true;
                        break;
                    }
                    penalty = unreachable;
                }
                total += penalty;
            }
            if rejected {
                continue;
            }
            if lowest.is_none_or(|l| total <= l) {
                if lowest.is_none_or(|l| total < l) {
                    picker.clear();
                    lowest = Some(total);
                    zero_weight = None;
                }
                if candidate.weight > 0.0 {
                    picker.add(candidate, candidate.weight);
                } else if zero_weight.is_none() {
                    zero_weight = Some(candidate.wang);
                }
            }
        }
        let kind_mask = terrain.kind_mask();
        if picker.is_empty() {
            let (lanes, cause) = match (agrees, lowest, zero_weight) {
                (true, Some(_), Some(wang)) => (
                    pattern_of(wang, PackedWangId::FULL, kind_mask),
                    Cause::ZeroProbability,
                ),
                // Every tile that agrees had a lane no tile could take where the cell wants.
                (true, _, _) => (
                    pattern_of(info.wanted, info.mask.union(stuck), kind_mask),
                    Cause::NoTile,
                ),
                (false, _, _) => (pattern_of(info.wanted, info.mask, kind_mask), Cause::NoTile),
            };
            return Err(Needed {
                cell: p,
                lanes,
                cause,
            });
        }
        let look_ahead = !self.corrections && !terrain.is_complete();
        let mut rng = cell_rng(self.seed, p);
        let mut rejections = [0_u32; LANE_COUNT];
        let mut patterns: [Option<[Option<u8>; LANE_COUNT]>; LANE_COUNT] = [None; LANE_COUNT];
        while let Some(candidate) = picker.take(&mut rng) {
            if look_ahead && let Some(rejection) = self.look_ahead(solve, p, candidate.wang) {
                if let Some(count) = rejections.get_mut(rejection.lane) {
                    *count += 1;
                }
                if let Some(pattern) = patterns.get_mut(rejection.lane) {
                    pattern.get_or_insert(rejection.pattern);
                }
                continue;
            }
            return Ok(candidate.cell);
        }
        // The lookahead turned the whole tier down: name the neighbour it did so most for.
        let mut most = 0;
        for (lane, &count) in rejections.iter().enumerate() {
            if count > rejections.get(most).copied().unwrap_or(0) {
                most = lane;
            }
        }
        let lanes = patterns
            .get(most)
            .copied()
            .flatten()
            .unwrap_or([None; LANE_COUNT]);
        Err(Needed {
            cell: p,
            lanes,
            cause: Cause::Neighbour(lane_of(most)),
        })
    }

    /// Whether placing a tile with `wang` at `p` leaves every unsettled neighbour some tile;
    /// the first neighbour it does not, with what that neighbour would have needed.
    fn look_ahead(&self, solve: &mut Solve, p: IVec2, wang: PackedWangId) -> Option<Rejection> {
        for (lane, &q) in self.topology.around(p).iter().enumerate() {
            if solve.checked.contains(&q) {
                continue;
            }
            let mut adjacent = self.grid.get(&q).copied().unwrap_or_default();
            update_adjacent(&mut adjacent, wang, lane);
            if adjacent.wanted != PackedWangId::ZERO
                && !self.is_used(solve, adjacent.wanted, adjacent.mask)
            {
                return Some(Rejection {
                    lane,
                    pattern: pattern_of(adjacent.wanted, adjacent.mask, self.terrain.kind_mask()),
                });
            }
        }
        None
    }

    /// Whether some candidate has `wanted` on the lanes of `mask` (Tiled's `wangIdIsUsed`),
    /// remembered per pattern for the run.
    fn is_used(&self, solve: &mut Solve, wanted: PackedWangId, mask: PackedWangId) -> bool {
        let mask = mask.masked(self.terrain.kind_mask());
        let wanted = wanted.masked(mask);
        *solve.used.entry((wanted, mask)).or_insert_with(|| {
            self.terrain
                .candidates()
                .iter()
                .any(|c| c.wang.masked(mask) == wanted)
        })
    }
}

/// The neighbour across `adjacent_lane` of a placed tile with `wang` must match it where they
/// touch: the facing lane, and for a side neighbour the two corners beside it (Tiled's
/// `updateAdjacent`).
pub(crate) fn update_adjacent(info: &mut CellInfo, wang: PackedWangId, adjacent_lane: usize) {
    let lane = opposite(adjacent_lane);
    info.wanted.set_lane(lane, wang.lane(adjacent_lane));
    info.mask.set_lane(lane, UNSET);
    if !is_corner(lane) {
        for (corner, from) in [
            (next(lane), previous(adjacent_lane)),
            (previous(lane), next(adjacent_lane)),
        ] {
            info.wanted.set_lane(corner, wang.lane(from));
            info.mask.set_lane(corner, UNSET);
        }
    }
}

/// The run's decisions in row-major order.
fn outcome_of(solve: Solve) -> Outcome {
    let row_major = |p: &IVec2| (p.y, p.x);
    let mut placed: Vec<PlacedCell> = solve
        .placed
        .into_iter()
        .map(|(at, cell)| PlacedCell { at, cell })
        .collect();
    placed.sort_by_key(|placed| row_major(&placed.at));
    let mut invalid: Vec<(IVec2, Needed)> = solve.invalid.into_iter().collect();
    invalid.sort_by_key(|(p, _)| row_major(p));
    let (invalid, needed) = invalid.into_iter().unzip();
    Outcome {
        placed,
        invalid,
        needed,
    }
}
