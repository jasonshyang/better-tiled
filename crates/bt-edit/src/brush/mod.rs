//! The brush: what the Stamp tool, Bucket Fill, Shape Fill, the quick stamps and the saved
//! stamps paint. A brush is one or more weighted variations, each a [`TileClip`] (so a brush has
//! per-layer parts and carries its tilesets into any map), with a mode and a size. Ported from
//! Tiled's `TileStamp` and the tools that paint one (`reference/tiled/src/tiled/tilestamp.cpp`,
//! `stampbrush.cpp`, `abstracttilefilltool.cpp`, `capturestamphelper.cpp`, `tilesetdock.cpp`);
//! the size is better-tiled's.

mod compose;
mod fill;
mod placement;
mod random;

use std::collections::HashSet;

use bt_core::{LayerId, Map, MapGeometry, PlacedCell};
use bt_utils::IRect;
use bt_utils::rng::RandomPicker;
use glam::IVec2;

use crate::clip::{ClipLayer, ClipTilesets, TileClip, copy};
use crate::error::EditError;
use crate::lattice::{FlipDirection, Lattice, RotateDirection};
use crate::region::TileRegion;

/// How a brush paints.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum BrushMode {
    /// Its variations as they are: Tiled's stamp.
    #[default]
    Stamp,
    /// One tile a cell, chosen among all of its tiles by each tile's probability: Tiled's random
    /// mode.
    Random,
}

/// One form of a brush: a clip, which a brush picks among its variations by weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Variation {
    /// The relative weight; a variation whose weight is not positive is never picked.
    pub probability: f64,
    /// The tiles, with their layers and tilesets.
    pub clip: TileClip,
}

/// What the painting tools paint. Its variations' cells name tilesets the variations list, which
/// every constructor checks, so whatever a brush makes pastes into any map. Empty cells never
/// erase (Tiled's rule), and a brush whose variations hold no tile paints nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    variations: Vec<Variation>,
    mode: BrushMode,
    size: u32,
}

/// Whether every cell of `clip` names a tileset the clip lists.
fn check_tilesets(clip: &TileClip) -> Result<(), EditError> {
    for layer in &clip.layers {
        for placed in layer.cells.cells() {
            if let Some(slot) = placed.cell.slot()
                && clip.tilesets.get(slot.index()).is_none()
            {
                return Err(EditError::NoSuchTileset(slot));
            }
        }
    }
    Ok(())
}

impl Brush {
    /// A stamp brush of one clip, as Paste makes one of the clipboard (`clip.trimmed()`). A cell
    /// naming a tileset the clip does not list is an error.
    pub fn from_clip(clip: TileClip) -> Result<Brush, EditError> {
        check_tilesets(&clip)?;
        Ok(Brush {
            variations: vec![Variation {
                probability: 1.0,
                clip,
            }],
            mode: BrushMode::Stamp,
            size: 1,
        })
    }

    /// A brush of tiles of `map`'s tilesets, each at its position, as the Tiles panel's
    /// selection or a tile picked off the map makes one: one layer without a name (as in
    /// Tiled), positions not given and empty cells left as holes. The brush's rectangle starts
    /// at the smallest position given, which is also its origin. A tileset the map lacks, or a
    /// tile its tileset lacks, is an error.
    pub fn from_cells(map: &Map, cells: &[PlacedCell]) -> Result<Brush, EditError> {
        let bounds = IRect::bounding(cells.iter().map(|placed| placed.at)).unwrap_or_default();
        let mut tilesets = ClipTilesets::new();
        let mut layer = ClipLayer::named("");
        for placed in cells {
            if let Some(slot) = placed.cell.slot() {
                let tileset = map.tileset(slot).ok_or(EditError::NoSuchTileset(slot))?;
                if !tileset.has_tile(placed.cell.tile()) {
                    return Err(EditError::NoSuchTile {
                        slot,
                        tile: placed.cell.tile(),
                    });
                }
            }
            let cell = tilesets.adopt(&map.tilesets, placed.cell)?;
            layer.put(placed.at - bounds.min(), cell);
        }
        Brush::from_clip(TileClip {
            origin: bounds.min(),
            size: bounds.size(),
            layers: vec![layer],
            tilesets: tilesets.into_list(),
        })
    }

    /// The Stamp tool's capture (Tiled's `CaptureStampHelper`): the tiles of the tile layers
    /// among `layers` inside `rect`, a brush layer for each, cut to their tiles. Several layers
    /// make a multi-layer brush. An area without tiles gives a brush with those layers and no
    /// tiles, which paints nothing; with no tile layer among `layers` it has no layers either
    /// (Tiled then keeps the brush it had, which is for the caller to decide).
    pub fn capture(map: &Map, layers: &[LayerId], rect: IRect) -> Result<Brush, EditError> {
        Brush::from_clip(copy(map, layers, &TileRegion::from_rect(rect))?.trimmed())
    }

    /// Adds a variation, as a saved stamp has several. A cell naming a tileset the clip does
    /// not list is an error.
    pub fn add_variation(&mut self, clip: TileClip, probability: f64) -> Result<(), EditError> {
        check_tilesets(&clip)?;
        self.variations.push(Variation { probability, clip });
        Ok(())
    }

    /// The same brush painting in `mode`.
    pub fn with_mode(self, mode: BrushMode) -> Brush {
        Brush { mode, ..self }
    }

    /// The same brush with `size`: the side of the square a one-tile or random brush paints
    /// around each point (see [`Brush::sized`]).
    pub fn with_size(self, size: u32) -> Brush {
        Brush { size, ..self }
    }

    /// The variations.
    pub fn variations(&self) -> &[Variation] {
        &self.variations
    }

    /// How it paints.
    pub fn mode(&self) -> BrushMode {
        self.mode
    }

    /// The side of the square it paints around each point when that applies; 1 for none.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Whether the brush paints a square of `size` around each point: it is random, or every
    /// variation is a single cell, and the size is 2 or more.
    fn paints_squares(&self) -> bool {
        let one_tile = !self.variations.is_empty()
            && self
                .variations
                .iter()
                .all(|variation| variation.clip.size == IVec2::ONE);
        self.size >= 2 && (self.mode == BrushMode::Random || one_tile)
    }

    /// The side of the squares, as a number of cells.
    fn side(&self) -> i32 {
        // No square that wide could be painted; the largest side a cell count holds stands in.
        i32::try_from(self.size).unwrap_or(i32::MAX)
    }

    /// The cells a stroke through `points` paints: with a size of 2 or more a one-tile brush
    /// (every variation a single cell) or a random brush paints the size × size square around
    /// each point, centred as a stamp is (for an even size the point is the later of the two
    /// middle cells), each cell once in the order the squares reach it. Other brushes, and sizes
    /// below 2, paint the points as they are.
    pub fn sized(&self, points: &[IVec2]) -> Vec<IVec2> {
        if !self.paints_squares() {
            return points.to_vec();
        }
        let side = self.side();
        let mut seen = HashSet::new();
        let mut cells = Vec::new();
        for &point in points {
            let square = IRect::from_min_size(point - IVec2::splat(side / 2), IVec2::splat(side));
            cells.extend(square.points().filter(|&cell| seen.insert(cell)));
        }
        cells
    }

    /// The cells a placement at `point` can cover, for previews and for masking by the
    /// selection: the square a one-tile or random brush of size 2 or more paints, a random
    /// brush's one cell, or the rectangle covering every variation centred on the point, fitted
    /// to the rows (columns) it lands on on staggered and hexagonal maps. Without variations it
    /// is the point with no size.
    pub fn footprint(&self, point: IVec2, geometry: &MapGeometry) -> IRect {
        if self.paints_squares() {
            let side = self.side();
            return IRect::from_min_size(point - IVec2::splat(side / 2), IVec2::splat(side));
        }
        if self.mode == BrushMode::Random {
            return IRect::from_min_size(point, IVec2::ONE);
        }
        let stagger = Lattice::of(geometry).stagger();
        self.variations
            .iter()
            .map(|variation| {
                let size = placement::placed_size(&variation.clip, point, stagger);
                IRect::from_min_size(point - size / 2, size)
            })
            .reduce(|a, b| a.union(b))
            .unwrap_or(IRect::from_min_size(point, IVec2::ZERO))
    }

    /// The brush with every variation mirrored on screen (Tiled's X and Y), each tile with it;
    /// see [`TileClip::flipped`].
    pub fn flipped(&self, direction: FlipDirection, geometry: &MapGeometry) -> Brush {
        self.with_clips(|clip| clip.flipped(direction, geometry))
    }

    /// The brush with every variation turned on screen (Tiled's Z and Shift+Z): a quarter turn,
    /// a sixth on hexagonal maps; see [`TileClip::rotated`].
    pub fn rotated(&self, direction: RotateDirection, geometry: &MapGeometry) -> Brush {
        self.with_clips(|clip| clip.rotated(direction, geometry))
    }

    /// The brush with `change` made to each variation's clip.
    fn with_clips(&self, change: impl Fn(&TileClip) -> TileClip) -> Brush {
        Brush {
            variations: self
                .variations
                .iter()
                .map(|variation| Variation {
                    probability: variation.probability,
                    clip: change(&variation.clip),
                })
                .collect(),
            ..self.clone()
        }
    }

    /// The variations' indexes, weighted by their probabilities.
    fn variation_picker(&self) -> RandomPicker<usize> {
        let mut picker = RandomPicker::new();
        for (index, variation) in self.variations.iter().enumerate() {
            picker.add(index, variation.probability);
        }
        picker
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, Orientation, TileId, TilesetSlot};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::rng::Rng;
    use glam::UVec2;

    use super::*;
    use crate::clip::{PasteTarget, cut, erase, erase_targets, find_target_layers, paste};
    use crate::testutil::{assert_valid, atlas, clip_grid, every_grid, tile, tile_of, trimmed};

    fn two_tilesets() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::new(6, 4))
            .tileset(atlas("a"))
            .tileset(atlas("b"))
            .tile_layer("ground", "1 2 . .\n3 4@1 . .\n. . . .\n. . . .")
            .tile_layer("top", ". 5 . .\n. . 6 .\n. . . .\n. . . .")
            .build()
            .unwrap()
    }

    fn placed(x: i32, y: i32, cell: Cell) -> PlacedCell {
        PlacedCell {
            at: IVec2::new(x, y),
            cell,
        }
    }

    #[test]
    fn picked_tiles_keep_their_holes_and_list_the_tilesets_they_use() {
        let map = two_tilesets();
        let cells = [
            placed(2, 3, tile_of(1, 1)),
            placed(4, 3, tile(2)),
            placed(2, 4, Cell::EMPTY),
        ];
        let brush = Brush::from_cells(&map, &cells).unwrap();
        let clip = &brush.variations()[0].clip;
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(2, 3), IVec2::new(3, 2))
        );
        assert_eq!(clip.layers[0].name, "");
        assert_eq!(clip_grid(clip, 0), trimmed("1 . 2@1\n. . ."));
        assert_eq!(clip.tilesets[0].tileset.name, "b");
        assert_eq!((brush.mode(), brush.size()), (BrushMode::Stamp, 1));
        let missing = Cell::new(TilesetSlot::new(2).unwrap(), TileId(0));
        assert_eq!(
            Brush::from_cells(&map, &[placed(0, 0, missing)]),
            Err(EditError::NoSuchTileset(TilesetSlot::new(2).unwrap()))
        );
        assert_eq!(
            Brush::from_cells(&map, &[placed(0, 0, tile(16))]),
            Err(EditError::NoSuchTile {
                slot: TilesetSlot::new(0).unwrap(),
                tile: TileId(16)
            })
        );
    }

    #[test]
    fn several_captured_layers_make_a_multi_layer_brush_cut_to_its_tiles() {
        let map = two_tilesets();
        let rect = IRect {
            x: 1,
            y: 0,
            w: 4,
            h: 4,
        };
        let brush = Brush::capture(&map, &[LayerId(2), LayerId(1)], rect).unwrap();
        let clip = &brush.variations()[0].clip;
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(1, 0), IVec2::new(2, 2))
        );
        assert_eq!(clip_grid(clip, 0), trimmed("2 .\n4@1 ."));
        assert_eq!(clip_grid(clip, 1), trimmed("5 .\n. 6"));
        let empty = IRect {
            x: 4,
            y: 2,
            w: 2,
            h: 2,
        };
        let nothing = Brush::capture(&map, &[LayerId(1), LayerId(2)], empty).unwrap();
        assert_eq!(nothing.variations()[0].clip.layers.len(), 2);
        let geometry = map.geometry();
        let mut rng = Rng::new(1);
        assert_eq!(nothing.placement(&[IVec2::ONE], &geometry, &mut rng), None);
        assert_eq!(nothing.fill(&TileRegion::from_rect(rect), &mut rng), None);
    }

    #[test]
    fn variations_must_name_tilesets_they_list() {
        let map = two_tilesets();
        let mut brush = Brush::from_cells(&map, &[placed(0, 0, tile(1))]).unwrap();
        let mut stray = brush.variations()[0].clip.clone();
        stray.tilesets.clear();
        assert_eq!(
            brush.add_variation(stray.clone(), 1.0),
            Err(EditError::NoSuchTileset(TilesetSlot::new(0).unwrap()))
        );
        assert_eq!(
            Brush::from_clip(stray),
            Err(EditError::NoSuchTileset(TilesetSlot::new(0).unwrap()))
        );
        let other = brush.variations()[0].clip.clone();
        brush.add_variation(other, 3.0).unwrap();
        assert_eq!(brush.variations().len(), 2);
        assert_eq!(brush.variations()[1].probability, 3.0);
    }

    #[test]
    fn one_tile_and_random_brushes_paint_squares_of_their_size() {
        let map = two_tilesets();
        let one = Brush::from_cells(&map, &[placed(0, 0, tile(1))])
            .unwrap()
            .with_size(3);
        let points = [IVec2::new(5, 5), IVec2::new(6, 5)];
        let cells = one.sized(&points);
        assert_eq!(cells.len(), 12, "two 3 × 3 squares overlapping in 6 cells");
        assert_eq!(cells[0], IVec2::new(4, 4));
        assert_eq!(
            TileRegion::from_cells(cells.iter().copied()).bounds(),
            Some(IRect {
                x: 4,
                y: 4,
                w: 4,
                h: 3
            })
        );
        let even = one.clone().with_size(2);
        assert_eq!(
            even.sized(&[IVec2::ZERO]),
            [
                IVec2::new(-1, -1),
                IVec2::new(0, -1),
                IVec2::new(-1, 0),
                IVec2::ZERO
            ]
        );
        let geometry = map.geometry();
        assert_eq!(
            one.footprint(IVec2::new(5, 5), &geometry),
            IRect {
                x: 4,
                y: 4,
                w: 3,
                h: 3
            }
        );
        // A stamp of several cells ignores the size, unless it paints at random.
        let stamp = Brush::capture(
            &map,
            &[LayerId(1)],
            IRect::from_min_size(IVec2::ZERO, IVec2::splat(2)),
        )
        .unwrap()
        .with_size(3);
        assert_eq!(stamp.sized(&points), points);
        assert_eq!(
            stamp
                .clone()
                .with_mode(BrushMode::Random)
                .sized(&points)
                .len(),
            12
        );
        assert_eq!(one.clone().with_size(1).sized(&points), points);
        assert_eq!(one.with_size(0).sized(&points), points);
    }

    #[test]
    fn the_footprint_covers_every_variation_centred_on_the_point() {
        let map = two_tilesets();
        let mut brush = Brush::capture(
            &map,
            &[LayerId(1)],
            IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 2)),
        )
        .unwrap();
        let wide = Brush::capture(
            &map,
            &[LayerId(1)],
            IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 1)),
        )
        .unwrap()
        .flipped(FlipDirection::Horizontal, &map.geometry())
        .rotated(RotateDirection::Clockwise, &map.geometry());
        brush
            .add_variation(wide.variations()[0].clip.clone(), 1.0)
            .unwrap();
        // 2 × 2 at (9, 9), and 1 × 2 at (10, 9).
        assert_eq!(
            brush.footprint(IVec2::new(10, 10), &map.geometry()),
            IRect {
                x: 9,
                y: 9,
                w: 2,
                h: 2
            }
        );
        let random = brush.with_mode(BrushMode::Random);
        assert_eq!(
            random.footprint(IVec2::new(10, 10), &map.geometry()),
            IRect::from_min_size(IVec2::new(10, 10), IVec2::ONE)
        );
    }

    #[test]
    fn whole_turns_and_double_mirrors_of_a_brush_are_the_identity() {
        let fuzz = Fuzz::from_env(0xB2_05E, 40);
        let atlas = atlas("a");
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            for geometry in every_grid() {
                let hexagonal = geometry.orientation == Orientation::Hexagonal;
                let mut brush: Option<Brush> = None;
                for _ in 0..2 {
                    let mut grid = String::new();
                    let (w, h) = (1 + rng.below(4).unwrap(), 1 + rng.below(4).unwrap());
                    for _ in 0..h {
                        let row: Vec<String> = (0..w)
                            .map(|_| match rng.below(3).unwrap() {
                                0 => ".".to_owned(),
                                1 => format!("{}h", rng.below(16).unwrap()),
                                _ if hexagonal => format!("{}x", rng.below(16).unwrap()),
                                _ => format!("{}vd", rng.below(16).unwrap()),
                            })
                            .collect();
                        grid.push_str(&row.join(" "));
                        grid.push('\n');
                    }
                    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
                        .tileset(Arc::clone(&atlas))
                        .tile_layer_at("ground", IVec2::new(1, 2), &grid)
                        .build()
                        .unwrap();
                    let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(8));
                    let captured = Brush::capture(&map, &[LayerId(1)], rect).unwrap();
                    let clip = captured.variations()[0].clip.clone();
                    match &mut brush {
                        None => brush = Some(captured),
                        Some(brush) => brush.add_variation(clip, 2.0).unwrap(),
                    }
                }
                let brush = brush.unwrap().with_mode(BrushMode::Random);
                let turns = if hexagonal { 6 } else { 4 };
                let mut turned = brush.clone();
                for _ in 0..turns {
                    turned = turned.rotated(RotateDirection::CounterClockwise, &geometry);
                }
                let context = format!("case {case} ({}) on {geometry:?}", fuzz.reproduce());
                assert_eq!(turned, brush, "{context}");
                for direction in [FlipDirection::Horizontal, FlipDirection::Vertical] {
                    let mirrored = brush
                        .flipped(direction, &geometry)
                        .flipped(direction, &geometry);
                    assert_eq!(mirrored, brush, "{direction:?}: {context}");
                }
            }
        }
    }

    /// Rows of random tiles of a 16-tile atlas, flipped now and then, with holes.
    fn random_grid(rng: &mut Rng, size: IVec2) -> String {
        (0..size.y)
            .map(|_| {
                (0..size.x)
                    .map(|_| match rng.below(4).unwrap() {
                        0 => ".".to_owned(),
                        1 => format!("{}h", rng.below(16).unwrap()),
                        _ => rng.below(16).unwrap().to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn random_rect(rng: &mut Rng) -> IRect {
        IRect {
            x: rng.below(12).unwrap() as i32 - 3,
            y: rng.below(12).unwrap() as i32 - 3,
            w: 1 + rng.below(6).unwrap() as i32,
            h: 1 + rng.below(6).unwrap() as i32,
        }
    }

    #[test]
    fn every_operation_keeps_the_map_valid() {
        let fuzz = Fuzz::from_env(0x0A11_D0E5, 120);
        let grids = every_grid();
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let geometry = grids[rng.below(grids.len() as u64).unwrap() as usize];
            let mut builder = MapBuilder::new(UVec2::splat(32), UVec2::splat(8))
                .orientation(geometry.orientation)
                .stagger(geometry.stagger_axis, geometry.stagger_index)
                .hex_side_length(geometry.hex_side_length)
                .tileset(atlas("a"));
            if rng.below(2) == Some(0) {
                builder = builder.infinite();
            }
            let (ground, top) = (
                random_grid(&mut rng, IVec2::splat(8)),
                random_grid(&mut rng, IVec2::splat(8)),
            );
            let mut map = builder
                .tile_layer("ground", &ground)
                .tile_layer("top", &top)
                .build()
                .unwrap();
            map.layers[1].locked = rng.below(3) == Some(0);
            let layers = [LayerId(1), LayerId(2)];
            let context = |step: u32| format!("case {case} ({}), step {step}", fuzz.reproduce());
            for step in 0..10 {
                let rect = random_rect(&mut rng);
                let mask = if rng.below(2) == Some(0) {
                    TileRegion::new()
                } else {
                    TileRegion::from_rect(random_rect(&mut rng))
                };
                let geometry = map.geometry();
                let painted = match rng.below(6).unwrap() {
                    0 | 1 => {
                        let mut brush = Brush::capture(&map, &layers, rect).unwrap();
                        if rng.below(2) == Some(0) {
                            brush = brush.rotated(RotateDirection::Clockwise, &geometry);
                        }
                        if rng.below(2) == Some(0) {
                            brush = brush.flipped(FlipDirection::Vertical, &geometry);
                        }
                        if rng.below(3) == Some(0) {
                            brush = brush.with_mode(BrushMode::Random).with_size(2);
                        }
                        let points: Vec<IVec2> = (0..1 + rng.below(6).unwrap())
                            .map(|_| random_rect(&mut rng).min())
                            .collect();
                        if rng.below(2) == Some(0) {
                            brush.placement(&points, &geometry, &mut rng)
                        } else {
                            brush.fill(&TileRegion::from_rect(random_rect(&mut rng)), &mut rng)
                        }
                    }
                    2 => {
                        cut(&mut map, &layers, &TileRegion::from_rect(rect)).unwrap();
                        None
                    }
                    3 => {
                        let targets =
                            erase_targets(&map, &layers, rng.below(2) == Some(0)).unwrap();
                        erase(&mut map, &targets, &TileRegion::from_rect(rect)).unwrap();
                        None
                    }
                    _ => {
                        let mut clip = copy(&map, &layers, &TileRegion::from_rect(rect)).unwrap();
                        clip.origin += random_rect(&mut rng).min();
                        Some(clip)
                    }
                };
                if let Some(clip) = painted {
                    let current = (rng.below(2) == Some(0)).then_some(LayerId(1));
                    let mut targets = find_target_layers(&map, &clip, current, &layers).unwrap();
                    if rng.below(4) == Some(0) {
                        targets = vec![PasteTarget::NewLayer; clip.layers.len()];
                    }
                    paste(&mut map, &clip, clip.origin, &targets, &mask).unwrap();
                }
                let violations = map.validate();
                assert!(violations.is_empty(), "{violations:?}: {}", context(step));
            }
            assert_valid(&map);
        }
    }
}
