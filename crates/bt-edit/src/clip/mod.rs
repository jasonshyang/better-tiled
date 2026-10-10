//! The tile clipboard: [`TileClip`], named layers of cells of one size with the tilesets the
//! cells use, which is what Copy puts aside and what every painting operation makes before it is
//! pasted, so a clip pastes into any map. Ported from Tiled's clipboard and stamp maps
//! (`reference/tiled/src/tiled/clipboardmanager.cpp`, `capturestamphelper.cpp`,
//! `tilestamp.cpp`, and `Map::copyLayers` and `normalizeTileLayerPositionsAndMapSize` in
//! `reference/tiled/src/libtiled/map.cpp`).

mod copy;
mod paste;
mod tilesets;

pub use copy::{TileCut, copy, cut, erase, erase_targets};
pub use paste::{PasteTarget, find_target_layers, paste};
pub(crate) use tilesets::ClipTilesets;

use bt_core::{Cell, Color, MapGeometry, PlacedCell, TileLayer, TilesetRef};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::axis::Axis;
use crate::lattice::{Lattice, Motion, RotateDirection};
use crate::region::TileRegion;

/// Tiles set aside: one or more named layers of one size, bottom to top, and the tilesets
/// their cells use, so the clip pastes into another map. Cells are relative to the clip's
/// top-left corner and their slots index [`TileClip::tilesets`]; a clip without tiles still
/// has its layers, and pastes nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileClip {
    /// The cell its top-left came from or goes to: where Copy took it (Paste in Place puts it
    /// back there), where a brush captured it, or where a placement paints it. On staggered and
    /// hexagonal maps its row (column) parity says which rows (columns) of the clip are the
    /// shifted ones, which keeps the clip's shape wherever a brush places it.
    pub origin: IVec2,
    /// The width and height in cells.
    pub size: IVec2,
    /// The layers, bottom to top.
    pub layers: Vec<ClipLayer>,
    /// The tilesets the cells' slots refer to.
    pub tilesets: Vec<TilesetRef>,
}

/// One layer of a clip: its cells and what a layer made for them when pasting gets.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipLayer {
    /// The name paste looks for a target layer by, and the name of a layer made for it.
    pub name: String,
    /// The cells, relative to the clip's top-left; the layer has no bounds.
    pub cells: TileLayer,
    /// The opacity of a layer made for it.
    pub opacity: f64,
    /// The tint of a layer made for it.
    pub tint: Option<Color>,
}

impl ClipLayer {
    /// An opaque, untinted layer called `name`, without cells.
    pub fn named(name: impl Into<String>) -> ClipLayer {
        ClipLayer {
            name: name.into(),
            cells: TileLayer::infinite(UVec2::ZERO),
            opacity: 1.0,
            tint: None,
        }
    }

    /// The same name, opacity and tint, without cells.
    pub(crate) fn emptied(&self) -> ClipLayer {
        ClipLayer {
            name: self.name.clone(),
            cells: TileLayer::infinite(UVec2::ZERO),
            opacity: self.opacity,
            tint: self.tint,
        }
    }

    /// Sets a cell and returns whether it changed. A clip layer has no bounds, so no position is
    /// refused (`TileLayer::set_cell` refuses only positions outside a finite layer).
    pub(crate) fn put(&mut self, at: IVec2, cell: Cell) -> bool {
        self.cells.set_cell(at, cell).unwrap_or(false)
    }
}

impl TileClip {
    /// Whether no layer holds a tile.
    pub fn is_empty(&self) -> bool {
        self.layers
            .iter()
            .all(|layer| layer.cells.cells().next().is_none())
    }

    /// The cells holding a tile in any layer, relative to the clip's top-left; translated by
    /// [`TileClip::origin`], where the clip came from or is painted.
    pub fn content_region(&self) -> TileRegion {
        self.layers
            .iter()
            .flat_map(|layer| layer.cells.cells().map(|placed| placed.at))
            .collect()
    }

    /// The clip cut to the bounding box of its tiles, all layers together; the origin moves
    /// with them, so the tiles stay where they were (Tiled's
    /// `normalizeTileLayerPositionsAndMapSize`, what a captured or pasted stamp gets). A clip
    /// without tiles keeps its origin and its layers and has no size.
    pub fn trimmed(&self) -> TileClip {
        let Some(content) = self.content_region().bounds() else {
            return TileClip {
                size: IVec2::ZERO,
                ..self.clone()
            };
        };
        let shift = content.min();
        let layers = self
            .layers
            .iter()
            .map(|layer| {
                let mut trimmed = layer.emptied();
                for placed in layer.cells.cells() {
                    trimmed.put(placed.at - shift, placed.cell);
                }
                trimmed
            })
            .collect();
        TileClip {
            origin: self.origin + shift,
            size: content.size(),
            layers,
            tilesets: self.tilesets.clone(),
        }
    }

    /// The top-left that centres the clip on `point`, as Tiled's Stamp Brush places a stamp: for
    /// an even side the point is the later of the two middle cells.
    pub fn centered_at(&self, point: IVec2) -> IVec2 {
        point - self.size / 2
    }

    /// The clip mirrored on screen, each tile mirrored with it (Tiled's X and Y stamp actions).
    /// It keeps its origin; see [`TileClip::rotated`] for how it fits there.
    pub fn flipped(&self, direction: Axis, geometry: &MapGeometry) -> TileClip {
        self.moved(Motion::Mirror(direction), geometry)
    }

    /// The clip turned on screen, each tile turned with it (Tiled's Z and Shift+Z stamp
    /// actions): a quarter turn, a sixth on hexagonal maps. The result keeps the origin and is
    /// shifted along the lattice so its top-left is there; on staggered and hexagonal maps the
    /// shifted rows (columns) stay the ones the origin's parity names, so the shape keeps its
    /// form. Square and diamond grids turn the whole rectangle, empty margins included, as Tiled
    /// does; on staggered and hexagonal maps, where a turned rectangle is no rectangle, the
    /// result is cut to its tiles (as Tiled's hexagonal turn is). Four quarter turns, six sixth
    /// turns or two mirrors give back a clip cut to its tiles exactly, with each tile's flags in
    /// Tiled's form.
    pub fn rotated(&self, direction: RotateDirection, geometry: &MapGeometry) -> TileClip {
        self.moved(Motion::Turn(direction), geometry)
    }

    /// The clip after a mirror or a turn, anchored at its origin.
    fn moved(&self, motion: Motion, geometry: &MapGeometry) -> TileClip {
        let lattice = Lattice::of(geometry);
        let moved: Vec<Vec<PlacedCell>> = self
            .layers
            .iter()
            .map(|layer| {
                layer
                    .cells
                    .cells()
                    .map(|placed| PlacedCell {
                        at: lattice.moved(self.origin + placed.at, motion),
                        cell: placed
                            .cell
                            .with_flip(lattice.moved_flip(placed.cell.flip(), motion)),
                    })
                    .collect()
            })
            .collect();
        let mut frame: Vec<IVec2> = moved.iter().flatten().map(|placed| placed.at).collect();
        let rect = IRect::from_min_size(self.origin, self.size);
        if lattice.keeps_rectangles() && !rect.is_empty() {
            let corners = [rect.min(), rect.max() - IVec2::ONE];
            frame.extend(corners.map(|corner| lattice.moved(corner, motion)));
        }
        // No frame means no tiles and no area: there is nothing to move, and no size.
        let step = lattice
            .anchoring_step(&frame, self.origin)
            .unwrap_or(IVec2::ZERO);
        let anchored: Vec<IVec2> = frame
            .iter()
            .map(|&cell| lattice.stepped(cell, step))
            .collect();
        let size = IRect::bounding(anchored).map_or(IVec2::ZERO, |bounds| bounds.size());
        let layers = self
            .layers
            .iter()
            .zip(moved)
            .map(|(layer, cells)| {
                let mut result = layer.emptied();
                for placed in cells {
                    result.put(lattice.stepped(placed.at, step) - self.origin, placed.cell);
                }
                result
            })
            .collect();
        TileClip {
            origin: self.origin,
            size,
            layers,
            tilesets: self.tilesets.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Flip, Orientation, StaggerAxis, StaggerIndex};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::grid_layer;
    use bt_utils::rng::Rng;

    use super::*;
    use crate::testutil::{every_grid, grid_of, trimmed};

    /// A one-layer clip of a grid (the fixtures' notation) at `origin`.
    fn clip_of(grid: &str, origin: IVec2) -> TileClip {
        let layer = grid_layer(grid).unwrap();
        let size = layer.size().as_ivec2();
        let mut clip_layer = ClipLayer::named("ground");
        for placed in layer.cells() {
            clip_layer.put(placed.at, placed.cell);
        }
        TileClip {
            origin,
            size,
            layers: vec![clip_layer],
            tilesets: Vec::new(),
        }
    }

    fn grid(clip: &TileClip) -> String {
        grid_of(
            &clip.layers[0].cells,
            IRect::from_min_size(IVec2::ZERO, clip.size),
        )
    }

    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(10))
    }

    #[test]
    fn trimming_cuts_to_the_tiles_and_moves_the_origin_with_them() {
        let clip = clip_of(
            "
            . . . .
            . 1 . .
            . . 2h .
            ",
            IVec2::new(5, -3),
        );
        let cut = clip.trimmed();
        assert_eq!(
            (cut.origin, cut.size),
            (IVec2::new(6, -2), IVec2::new(2, 2))
        );
        assert_eq!(grid(&cut), trimmed("1 .\n. 2h"));
        assert_eq!(
            cut.content_region(),
            TileRegion::from_cells([IVec2::ZERO, IVec2::ONE])
        );
        let empty = clip_of(". .", IVec2::new(4, 4));
        let cut = empty.trimmed();
        assert_eq!((cut.origin, cut.size), (IVec2::new(4, 4), IVec2::ZERO));
        assert!(cut.is_empty() && cut.layers.len() == 1);
        assert_eq!(clip.centered_at(IVec2::new(10, 10)), IVec2::new(8, 9));
    }

    #[test]
    fn square_grids_turn_and_mirror_the_whole_rectangle_as_tiled_does() {
        // Tiled's RotateRight puts (x, y) at (height - 1 - y, x); empty margins turn too.
        let clip = clip_of(
            "
            1 2 .
            3 . 4v
            ",
            IVec2::new(2, 3),
        );
        let right = clip.rotated(RotateDirection::Clockwise, &square());
        assert_eq!(
            (right.origin, right.size),
            (IVec2::new(2, 3), IVec2::new(2, 3))
        );
        assert_eq!(grid(&right), trimmed("3hd 1hd\n. 2hd\n4d ."));
        let left = clip.rotated(RotateDirection::CounterClockwise, &square());
        assert_eq!(grid(&left), trimmed(". 4hvd\n2vd .\n1vd 3vd"));
        let mirrored = clip.flipped(Axis::Horizontal, &square());
        assert_eq!(grid(&mirrored), trimmed(". 2h 1h\n4hv . 3h"));
        let upside_down = clip.flipped(Axis::Vertical, &square());
        assert_eq!(grid(&upside_down), trimmed("3v . 4\n1v 2v ."));
    }

    #[test]
    fn an_isometric_mirror_swaps_the_axes() {
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), IVec2::ONE);
        let clip = clip_of("1 2 3", IVec2::ZERO);
        let mirrored = clip.flipped(Axis::Horizontal, &iso);
        assert_eq!(mirrored.size, IVec2::new(1, 3));
        assert_eq!(grid(&mirrored), trimmed("1h\n2h\n3h"));
    }

    #[test]
    fn hexagonal_mirrors_and_turns_use_the_hexagonal_tables() {
        let hex = MapGeometry {
            hex_side_length: 16,
            ..MapGeometry::new(Orientation::Hexagonal, IVec2::splat(32), IVec2::ONE)
        };
        let clip = clip_of("1d", IVec2::ZERO);
        let mirrored = clip.flipped(Axis::Horizontal, &hex);
        let cell = mirrored.layers[0].cells.cell(IVec2::ZERO);
        assert_eq!(cell.flip(), Flip::D.flipped_hex_horizontally());
        let turned = clip.rotated(RotateDirection::Clockwise, &hex);
        let cell = turned.layers[0].cells.cell(IVec2::ZERO);
        assert_eq!(cell.flip(), Flip::D.rotated_hex_cw());
        assert_eq!(turned.size, IVec2::ONE);
    }

    #[test]
    fn a_turned_hexagonal_clip_keeps_its_shape() {
        // Cells next to each other stay next to each other: the steps between turned cells are
        // the turned steps.
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            for index in [StaggerIndex::Odd, StaggerIndex::Even] {
                let hex = MapGeometry {
                    hex_side_length: 16,
                    stagger_axis: axis,
                    stagger_index: index,
                    ..MapGeometry::new(Orientation::Hexagonal, IVec2::splat(32), IVec2::ONE)
                };
                // Each tile appears once, so a tile names its cell before and after.
                let clip = clip_of("1 2 .\n. 3 4\n5 . .", IVec2::new(3, 1));
                let turned = clip.rotated(RotateDirection::Clockwise, &hex);
                let cubes = |clip: &TileClip| {
                    let mut cubes: Vec<(u32, bt_core::Hex)> = clip.layers[0]
                        .cells
                        .cells()
                        .map(|placed| {
                            let at = clip.origin + placed.at;
                            (
                                placed.cell.tile().0,
                                bt_core::Hex::from_offset(at, axis, index),
                            )
                        })
                        .collect();
                    cubes.sort_by_key(|(tile, _)| *tile);
                    cubes
                };
                let before = cubes(&clip);
                let after = cubes(&turned);
                assert_eq!(before.len(), after.len());
                let offset = after[0].1 - before[0].1.rotated_cw();
                for ((tile_a, a), (tile_b, b)) in before.iter().zip(&after) {
                    assert_eq!(tile_a, tile_b);
                    assert_eq!(*b - a.rotated_cw(), offset, "{axis:?} {index:?}");
                }
                assert_eq!(turned.trimmed(), turned, "the result is cut to its tiles");
            }
        }
    }

    /// A random clip cut to its tiles, with flags in Tiled's form for the grid (hexagonal
    /// tables give a turn by both 60° and 120° back in another form).
    fn random_clip(rng: &mut Rng, hexagonal: bool) -> TileClip {
        let w = 1 + rng.below(4).unwrap() as i32;
        let h = 1 + rng.below(4).unwrap() as i32;
        let mut layers = Vec::new();
        for name in ["ground", "top"] {
            let mut layer = ClipLayer::named(name);
            for at in IRect::from_min_size(IVec2::ZERO, IVec2::new(w, h)).points() {
                if rng.below(3) == Some(0) {
                    continue;
                }
                let mut flip = Flip::from_bits_truncate(rng.below(16).unwrap() as u8);
                if hexagonal && flip.contains(Flip::D | Flip::HEX120) {
                    flip.remove(Flip::HEX120);
                }
                let cell = crate::testutil::tile(rng.below(8).unwrap() as u32).with_flip(flip);
                layer.put(at, cell);
            }
            layers.push(layer);
        }
        let origin = IVec2::new(
            rng.below(9).unwrap() as i32 - 4,
            rng.below(9).unwrap() as i32 - 4,
        );
        TileClip {
            origin,
            size: IVec2::new(w, h),
            layers,
            tilesets: Vec::new(),
        }
        .trimmed()
    }

    #[test]
    fn whole_turns_and_double_mirrors_give_the_clip_back_on_every_grid() {
        let fuzz = Fuzz::from_env(0x7_7C11, 60);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            for geometry in every_grid() {
                let hexagonal = geometry.orientation == Orientation::Hexagonal;
                let clip = random_clip(&mut rng, hexagonal);
                let context = || format!("case {case} ({}) on {geometry:?}", fuzz.reproduce());
                let turns = if hexagonal { 6 } else { 4 };
                for direction in [
                    RotateDirection::Clockwise,
                    RotateDirection::CounterClockwise,
                ] {
                    let mut turned = clip.clone();
                    for _ in 0..turns {
                        turned = turned.rotated(direction, &geometry);
                    }
                    assert_eq!(turned, clip, "{direction:?}, {}", context());
                }
                for direction in [Axis::Horizontal, Axis::Vertical] {
                    let mirrored = clip
                        .flipped(direction, &geometry)
                        .flipped(direction, &geometry);
                    assert_eq!(mirrored, clip, "{direction:?}, {}", context());
                }
                let there_and_back = clip
                    .rotated(RotateDirection::Clockwise, &geometry)
                    .rotated(RotateDirection::CounterClockwise, &geometry);
                assert_eq!(there_and_back, clip, "{}", context());
            }
        }
    }
}
