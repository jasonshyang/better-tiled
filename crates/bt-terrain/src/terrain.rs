//! A terrain set prepared for painting: what Tiled's `WangSet` computes for the filler
//! (`reference/tiled/src/libtiled/wangset.cpp`: `recalculateCells`, `recalculateColorDistances`,
//! `wangIdOfCell`, `wangIdProbability`, `transitionPenalty`, `isComplete`,
//! `effectiveTypeForColor`).

use std::collections::{BTreeMap, HashSet};

use bt_core::{Cell, Flip, Map, TileId, Tileset, TilesetSlot, WangId, WangSet, WangSetType};

use crate::color::TerrainColor;
use crate::error::TerrainError;
use crate::wangid::{LANE_COUNT, PackedWangId, kind_mask};

/// A tile the solver may place: a labelled tile, or a flipped or turned variant of one, with the
/// Wang id of what is placed, masked by the set's kind.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Candidate {
    pub(crate) cell: Cell,
    pub(crate) wang: PackedWangId,
    /// The tile's probability times the probabilities of the terrains on its lanes; at most 0
    /// means "never placed".
    pub(crate) weight: f64,
}

/// A terrain set of one of a map's tilesets, prepared once per tileset version: the candidate
/// tiles with the variants the tileset allows, their weights, the distances between terrains,
/// and the labels to read cells back with. It holds no reference to the map.
#[derive(Clone, Debug)]
pub struct Terrain {
    slot: TilesetSlot,
    set_index: usize,
    name: String,
    kind: WangSetType,
    kind_mask: PackedWangId,
    color_count: usize,
    /// Each terrain's probability, by colour (index 0 unused).
    color_probabilities: Vec<f64>,
    /// The labels that count, as the set's file has them (not masked).
    labels: BTreeMap<TileId, PackedWangId>,
    candidates: Vec<Candidate>,
    /// `distances[from * (color_count + 1) + to]`; -1 when no tiles lead from one to the other.
    distances: Vec<i32>,
    max_distance: i32,
    complete: bool,
}

impl Terrain {
    /// The terrain set `set_index` of the map's tileset in `slot`.
    pub fn new(map: &Map, slot: TilesetSlot, set_index: usize) -> Result<Terrain, TerrainError> {
        let tileset = map.tileset(slot).ok_or(TerrainError::NoSuchTileset(slot))?;
        Terrain::from_tileset(tileset, slot, set_index)
    }

    /// The terrain set `set_index` of `tileset`, whose tiles are placed from `slot`.
    pub fn from_tileset(
        tileset: &Tileset,
        slot: TilesetSlot,
        set_index: usize,
    ) -> Result<Terrain, TerrainError> {
        let set = tileset
            .wang_sets
            .get(set_index)
            .ok_or_else(|| TerrainError::NoSuchSet {
                tileset: tileset.name.clone(),
                index: set_index,
            })?;
        let color_count = set.colors.len();
        let labels = usable_labels(tileset, set);
        let kind_mask = kind_mask(set.kind);
        let mut color_probabilities = vec![1.0];
        color_probabilities.extend(set.colors.iter().map(|c| c.probability));
        let mut terrain = Terrain {
            slot,
            set_index,
            name: set.name.clone(),
            kind: set.kind,
            kind_mask,
            color_count,
            color_probabilities,
            labels,
            candidates: Vec::new(),
            distances: Vec::new(),
            max_distance: 1,
            complete: false,
        };
        terrain.recalculate_candidates(tileset);
        terrain.recalculate_distances();
        Ok(terrain)
    }

    /// Every labelled tile in tile-id order, then the flipped and turned variants the tileset
    /// allows, enumerated as Tiled enumerates them; with "prefer untransformed" a variant whose
    /// Wang id is already placed is left out. Counts the distinct full Wang ids for
    /// [`Terrain::is_complete`].
    fn recalculate_candidates(&mut self, tileset: &Tileset) {
        let mut seen: HashSet<PackedWangId> = HashSet::new();
        let mut full_ids = 0_u64;
        let mut candidates = Vec::new();
        for (&tile, &label) in &self.labels {
            let wang = label.masked(self.kind_mask);
            full_ids += u64::from(!wang.has_zero_lane() && !seen.contains(&wang));
            seen.insert(wang);
            candidates.push(self.candidate(tileset, tile, Flip::empty(), wang));
        }
        let transformations = tileset.transformations;
        let allows = |t: bt_core::Transformations| transformations.contains(t);
        let any = allows(bt_core::Transformations::HFLIP)
            || allows(bt_core::Transformations::VFLIP)
            || allows(bt_core::Transformations::ROTATE);
        if any {
            let prefer = allows(bt_core::Transformations::PREFER_UNTRANSFORMED);
            for (&tile, &label) in &self.labels {
                let base = label.masked(self.kind_mask);
                let mut variants = vec![Variant {
                    flip: Flip::empty(),
                    wang: base,
                }];
                if allows(bt_core::Transformations::ROTATE) {
                    for i in 0..3 {
                        let Some(&previous) = variants.get(i) else {
                            break;
                        };
                        variants.push(Variant {
                            flip: previous.flip.rotated_cw(),
                            wang: previous.wang.rotated(1),
                        });
                    }
                }
                if allows(bt_core::Transformations::HFLIP) {
                    let mirrored: Vec<Variant> = variants
                        .iter()
                        .map(|v| Variant {
                            flip: v.flip.flipped_horizontally(),
                            wang: v.wang.flipped_horizontally(),
                        })
                        .collect();
                    variants.extend(mirrored);
                }
                if variants.len() <= 4 && allows(bt_core::Transformations::VFLIP) {
                    let mirrored: Vec<Variant> = variants
                        .iter()
                        .map(|v| Variant {
                            flip: v.flip.flipped_vertically(),
                            wang: v.wang.flipped_vertically(),
                        })
                        .collect();
                    variants.extend(mirrored);
                }
                let has_zero_lane = base.has_zero_lane();
                for variant in variants.into_iter().skip(1) {
                    let exists = seen.contains(&variant.wang);
                    if prefer && exists {
                        continue;
                    }
                    full_ids += u64::from(!has_zero_lane && !exists);
                    seen.insert(variant.wang);
                    candidates.push(self.candidate(tileset, tile, variant.flip, variant.wang));
                }
            }
        }
        self.candidates = candidates;
        self.complete = full_ids == self.complete_set_size();
    }

    fn candidate(
        &self,
        tileset: &Tileset,
        tile: TileId,
        flip: Flip,
        wang: PackedWangId,
    ) -> Candidate {
        let tile_probability = tileset.tile(tile).map_or(1.0, |data| data.probability);
        Candidate {
            cell: Cell::new(self.slot, tile).with_flip(flip),
            wang,
            weight: self.wang_probability(wang) * tile_probability,
        }
    }

    /// The tiles a complete set has (Tiled's `completeSetSize`): every combination of every
    /// terrain on the lanes, counted over four lanes for corner and edge sets and eight for mixed
    /// ones. Tiled counts a Wang id as full only when none of its eight lanes is 0, which a corner
    /// or edge set's ids never are, so those sets are never complete; the quirk is kept because
    /// completeness decides whether fills look ahead, which changes which cells stay unfilled.
    fn complete_set_size(&self) -> u64 {
        let c = self.color_count as u64;
        match self.kind {
            WangSetType::Corner | WangSetType::Edge => c.pow(4),
            WangSetType::Mixed => c.pow(8),
        }
    }

    /// Two terrains are a step apart when a tile has both on its corners or both on its edges
    /// (corners and edges do not connect); the steps are relaxed to shortest paths. Distances to
    /// "no terrain" stay direct, as in Tiled.
    fn recalculate_distances(&mut self) {
        let n = self.color_count;
        let width = n + 1;
        let mut distances = vec![-1_i32; width * width];
        for color in 1..=n {
            let row = color * width;
            for label in self.labels.values() {
                let wang = label.masked(self.kind_mask);
                let lanes = wang.lanes();
                let corners = [lanes[1], lanes[3], lanes[5], lanes[7]];
                let edges = [lanes[0], lanes[2], lanes[4], lanes[6]];
                for group in [corners, edges] {
                    if group.contains(&(color as u8)) {
                        for &other in &group {
                            if let Some(d) = distances.get_mut(row + usize::from(other)) {
                                *d = 1;
                            }
                        }
                    }
                }
            }
            if let Some(d) = distances.get_mut(row + color) {
                *d = 0;
            }
        }
        let at =
            |d: &Vec<i32>, from: usize, to: usize| d.get(from * width + to).copied().unwrap_or(-1);
        let mut max_distance = 1;
        loop {
            let mut progress = false;
            for i in 1..=n {
                for j in 1..=n {
                    if i == j {
                        continue;
                    }
                    for t in 0..=n {
                        let d0 = at(&distances, i, t);
                        let d1 = at(&distances, j, t);
                        if d0 == -1 || d1 == -1 {
                            continue;
                        }
                        let d = at(&distances, i, j);
                        if d == -1 || d0 + d1 < d {
                            let shorter = d0 + d1;
                            if let Some(slot) = distances.get_mut(i * width + j) {
                                *slot = shorter;
                            }
                            if let Some(slot) = distances.get_mut(j * width + i) {
                                *slot = shorter;
                            }
                            max_distance = max_distance.max(shorter);
                            progress = true;
                        }
                    }
                }
            }
            if !progress {
                break;
            }
        }
        self.distances = distances;
        self.max_distance = max_distance;
    }

    /// The product of the probabilities of the terrains on the lanes (Tiled's
    /// `wangIdProbability`).
    fn wang_probability(&self, wang: PackedWangId) -> f64 {
        wang.lanes()
            .iter()
            .filter(|&&c| c != 0)
            .map(|&c| {
                self.color_probabilities
                    .get(usize::from(c))
                    .copied()
                    .unwrap_or(1.0)
            })
            .product()
    }

    /// The tileset slot the set's tiles are placed from.
    pub fn slot(&self) -> TilesetSlot {
        self.slot
    }

    /// The set's index in its tileset.
    pub fn set_index(&self) -> usize {
        self.set_index
    }

    /// The set's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Which lanes the set uses.
    pub fn kind(&self) -> WangSetType {
        self.kind
    }

    /// How many terrains the set has.
    pub fn color_count(&self) -> usize {
        self.color_count
    }

    /// How many tiles carry a label that counts.
    pub fn labelled_tile_count(&self) -> usize {
        self.labels.len()
    }

    /// How many tiles the solver may place, variants included.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Whether `color` is "no terrain" or one of the set's terrains.
    pub fn has_color(&self, color: TerrainColor) -> bool {
        usize::from(color.0) <= self.color_count
    }

    /// How many tiles it takes to get from one terrain to the other, or `None` when no tiles
    /// lead there (Tiled's `transitionPenalty`).
    pub fn penalty(&self, from: TerrainColor, to: TerrainColor) -> Option<u32> {
        u32::try_from(self.raw_penalty(from.0, to.0)).ok()
    }

    pub(crate) fn raw_penalty(&self, from: u8, to: u8) -> i32 {
        let width = self.color_count + 1;
        let lookup = |row: u8, column: u8| {
            self.distances
                .get(usize::from(row) * width + usize::from(column))
                .copied()
                .unwrap_or(-1)
        };
        match (from, to) {
            (0, 0) => 0,
            (0, to) => lookup(to, 0),
            (from, to) => lookup(from, to),
        }
    }

    /// The longest finite distance between two terrains, at least 1: how far the brush re-solves
    /// cells around what it paints.
    pub fn max_distance(&self) -> u32 {
        u32::try_from(self.max_distance).unwrap_or(1)
    }

    pub(crate) fn raw_max_distance(&self) -> i32 {
        self.max_distance
    }

    /// Whether the set has every combination of its terrains (Tiled's `isComplete`; never true
    /// for corner and edge sets, see [`Terrain::complete_set_size`]).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// How the brush paints `color` (Tiled's `effectiveTypeForColor`): on a mixed set a terrain
    /// used only on corners paints as a corner set, only on edges as an edge set, else both.
    pub fn brush_kind(&self, color: TerrainColor) -> WangSetType {
        if self.kind != WangSetType::Mixed {
            return self.kind;
        }
        let mut on_corner = false;
        let mut on_edge = false;
        if !color.is_none() && usize::from(color.0) <= self.color_count {
            for label in self.labels.values() {
                for (lane, &c) in label.lanes().iter().enumerate() {
                    if c == color.0 {
                        on_corner |= lane % 2 == 1;
                        on_edge |= lane % 2 == 0;
                    }
                }
            }
        }
        match (on_corner, on_edge) {
            (true, false) => WangSetType::Corner,
            (false, true) => WangSetType::Edge,
            _ => WangSetType::Mixed,
        }
    }

    /// The terrains on a cell's lanes, its flips applied as Tiled applies them (the transpose is
    /// a quarter turn and a left-right mirror, then the left-right mirror, then the top-bottom
    /// one), masked by the set's kind; all 0 for a cell of another tileset or an unlabelled tile.
    pub fn wang_of_cell(&self, cell: Cell) -> WangId {
        self.packed_wang_of_cell(cell).to_wang_id()
    }

    pub(crate) fn packed_wang_of_cell(&self, cell: Cell) -> PackedWangId {
        if cell.slot() != Some(self.slot) {
            return PackedWangId::ZERO;
        }
        let Some(&label) = self.labels.get(&cell.tile()) else {
            return PackedWangId::ZERO;
        };
        let flip = cell.flip();
        let mut wang = label;
        if flip.contains(Flip::D) {
            wang = wang.rotated(1).flipped_horizontally();
        }
        if flip.contains(Flip::H) {
            wang = wang.flipped_horizontally();
        }
        if flip.contains(Flip::V) {
            wang = wang.flipped_vertically();
        }
        wang.masked(self.kind_mask)
    }

    pub(crate) fn kind_mask(&self) -> PackedWangId {
        self.kind_mask
    }

    pub(crate) fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// Whether some tile the solver may place has `pattern` (each `Some` lane that terrain, a
    /// `None` lane anything); with `placeable`, only tiles whose weight lets them be placed.
    pub fn has_tile_with(&self, pattern: &[Option<u8>; LANE_COUNT], placeable: bool) -> bool {
        let mut wanted = PackedWangId::ZERO;
        let mut mask = PackedWangId::ZERO;
        for (lane, value) in pattern.iter().enumerate() {
            if let Some(color) = value {
                wanted.set_lane(lane, *color);
                mask.set_lane(lane, crate::wangid::UNSET);
            }
        }
        let mask = mask.masked(self.kind_mask);
        let wanted = wanted.masked(mask);
        self.candidates
            .iter()
            .any(|c| c.wang.masked(mask) == wanted && (!placeable || c.weight > 0.0))
    }
}

/// A flipped or turned form of a tile while candidates are enumerated.
#[derive(Copy, Clone)]
struct Variant {
    flip: Flip,
    wang: PackedWangId,
}

/// The labels that count: of tiles the tileset has, not all zero, naming only terrains the set
/// has (Tiled refuses or never stores the others).
fn usable_labels(tileset: &Tileset, set: &WangSet) -> BTreeMap<TileId, PackedWangId> {
    let count = set.colors.len();
    set.tiles
        .iter()
        .filter(|(tile, label)| {
            tileset.has_tile(**tile)
                && !label.is_empty()
                && label.lanes().iter().all(|&c| usize::from(c) <= count)
        })
        .map(|(&tile, &label)| (tile, PackedWangId::from_wang_id(label)))
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::{Transformations, WangColor};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn color(name: &str, probability: f64) -> WangColor {
        WangColor {
            name: name.to_owned(),
            class: String::new(),
            color: bt_core::Color::rgb(0, 0, 0),
            tile: None,
            probability,
            properties: Default::default(),
        }
    }

    /// A corner set labelled on tiles 0.. with the given corner terrains (top-right,
    /// bottom-right, bottom-left, top-left).
    fn corner_tileset(
        colors: usize,
        corners: &[[u8; 4]],
        transformations: Transformations,
    ) -> Tileset {
        let mut tileset = (*Atlas::builder()
            .grid(8, 8)
            .build_embedded()
            .unwrap()
            .tileset)
            .clone();
        let mut set = WangSet {
            name: "set".to_owned(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: None,
            colors: (1..=colors).map(|i| color(&format!("c{i}"), 1.0)).collect(),
            tiles: Default::default(),
            properties: Default::default(),
        };
        for (i, c) in corners.iter().enumerate() {
            set.tiles.insert(
                TileId(i as u32),
                WangId::from_lanes([0, c[0], 0, c[1], 0, c[2], 0, c[3]]),
            );
        }
        tileset.wang_sets.push(set);
        tileset.transformations = transformations;
        tileset
    }

    fn slot() -> TilesetSlot {
        TilesetSlot::new(0).unwrap()
    }

    #[test]
    fn variants_follow_tileds_order_and_preference() {
        let tileset = corner_tileset(2, &[[1, 1, 2, 2]], Transformations::ROTATE);
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        let wangs: Vec<[u8; 8]> = terrain
            .candidates()
            .iter()
            .map(|c| c.wang.lanes())
            .collect();
        assert_eq!(
            wangs,
            [
                [0, 1, 0, 1, 0, 2, 0, 2],
                [0, 2, 0, 1, 0, 1, 0, 2],
                [0, 2, 0, 2, 0, 1, 0, 1],
                [0, 1, 0, 2, 0, 2, 0, 1],
            ],
            "the tile, then three quarter turns"
        );
        assert_eq!(
            terrain.candidates()[1].cell.flip(),
            Flip::empty().rotated_cw()
        );

        // A tile whose turns repeat it: with the preference, the repeats are left out.
        let all_one = corner_tileset(
            1,
            &[[1, 1, 1, 1]],
            Transformations::ROTATE
                | Transformations::HFLIP
                | Transformations::PREFER_UNTRANSFORMED,
        );
        let terrain = Terrain::from_tileset(&all_one, slot(), 0).unwrap();
        assert_eq!(terrain.candidate_count(), 1);
        let without = corner_tileset(
            1,
            &[[1, 1, 1, 1]],
            Transformations::ROTATE | Transformations::HFLIP,
        );
        assert_eq!(
            Terrain::from_tileset(&without, slot(), 0)
                .unwrap()
                .candidate_count(),
            8
        );
        let flips = corner_tileset(
            1,
            &[[1, 1, 1, 1]],
            Transformations::HFLIP | Transformations::VFLIP,
        );
        assert_eq!(
            Terrain::from_tileset(&flips, slot(), 0)
                .unwrap()
                .candidate_count(),
            4,
            "mirrored both ways when there are no turns"
        );
    }

    #[test]
    fn distances_relax_through_the_chain() {
        // A–B and B–C transitions only: A to C takes two steps.
        let tileset = corner_tileset(
            3,
            &[
                [1, 1, 1, 1],
                [2, 2, 2, 2],
                [3, 3, 3, 3],
                [1, 2, 2, 1],
                [2, 3, 3, 2],
            ],
            Transformations::empty(),
        );
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        let c = TerrainColor;
        assert_eq!(terrain.penalty(c(1), c(2)), Some(1));
        assert_eq!(terrain.penalty(c(1), c(3)), Some(2));
        assert_eq!(terrain.penalty(c(3), c(1)), Some(2));
        assert_eq!(terrain.penalty(c(2), c(2)), Some(0));
        assert_eq!(
            terrain.penalty(c(1), TerrainColor::NONE),
            None,
            "nothing borders nothing"
        );
        assert_eq!(
            terrain.penalty(TerrainColor::NONE, TerrainColor::NONE),
            Some(0)
        );
        assert_eq!(terrain.max_distance(), 2);
        assert!(!terrain.is_complete(), "a corner set never is, as in Tiled");
    }

    #[test]
    fn weights_multiply_terrain_and_tile_probabilities() {
        let mut tileset = corner_tileset(2, &[[1, 1, 2, 2]], Transformations::empty());
        tileset.wang_sets[0].colors[0].probability = 0.5;
        tileset.wang_sets[0].colors[1].probability = 3.0;
        tileset.tiles.insert(
            TileId(0),
            bt_core::TileData {
                probability: 2.0,
                ..Default::default()
            },
        );
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        assert_eq!(terrain.candidates()[0].weight, 0.5 * 0.5 * 3.0 * 3.0 * 2.0);
    }

    #[test]
    fn cells_read_back_with_their_flips() {
        let tileset = corner_tileset(4, &[[1, 2, 3, 4]], Transformations::empty());
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        let cell = Cell::new(slot(), TileId(0));
        let lanes = |flip: Flip| terrain.wang_of_cell(cell.with_flip(flip)).lanes();
        assert_eq!(lanes(Flip::empty()), [0, 1, 0, 2, 0, 3, 0, 4]);
        assert_eq!(lanes(Flip::H), [0, 4, 0, 3, 0, 2, 0, 1]);
        assert_eq!(lanes(Flip::V), [0, 2, 0, 1, 0, 4, 0, 3]);
        // The transpose (a quarter turn, then a left-right mirror) keeps the top-left and
        // bottom-right corners and swaps the other two.
        assert_eq!(lanes(Flip::D), [0, 3, 0, 2, 0, 1, 0, 4]);
        assert_eq!(terrain.wang_of_cell(Cell::EMPTY), WangId::default());
        let other = Cell::new(TilesetSlot::new(1).unwrap(), TileId(0));
        assert_eq!(terrain.wang_of_cell(other), WangId::default());
    }

    #[test]
    fn labels_that_cannot_count_are_left_out() {
        let mut tileset = corner_tileset(1, &[[1, 1, 1, 1]], Transformations::empty());
        let set = &mut tileset.wang_sets[0];
        set.tiles
            .insert(TileId(1), WangId::from_lanes([0, 2, 0, 1, 0, 1, 0, 1]));
        set.tiles
            .insert(TileId(500), WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1]));
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        assert_eq!(
            terrain.labelled_tile_count(),
            1,
            "a missing terrain, a missing tile"
        );
        assert!(Terrain::from_tileset(&tileset, slot(), 3).is_err());
    }

    #[test]
    fn a_mixed_set_paints_a_terrain_by_where_it_is_used() {
        let mut tileset = corner_tileset(2, &[], Transformations::empty());
        let set = &mut tileset.wang_sets[0];
        set.kind = WangSetType::Mixed;
        set.tiles
            .insert(TileId(0), WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1]));
        set.tiles
            .insert(TileId(1), WangId::from_lanes([2, 1, 2, 1, 2, 1, 2, 1]));
        let terrain = Terrain::from_tileset(&tileset, slot(), 0).unwrap();
        assert_eq!(terrain.brush_kind(TerrainColor(1)), WangSetType::Corner);
        assert_eq!(terrain.brush_kind(TerrainColor(2)), WangSetType::Edge);
        assert_eq!(terrain.brush_kind(TerrainColor::NONE), WangSetType::Mixed);
    }
}
