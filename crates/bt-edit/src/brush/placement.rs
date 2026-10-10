//! Where a brush's variations go along a stroke (Tiled's `StampBrush::drawPreviewLayer`,
//! `reference/tiled/src/tiled/stampbrush.cpp`): one centred on each point unless it would
//! overlap tiles the stroke placed before, a new variation picked after each placement, and on
//! staggered and hexagonal maps each fitted to the parity of the rows (columns) it lands on.

use bt_core::{MapGeometry, StaggerAxis};
use bt_utils::rng::Rng;
use glam::IVec2;

use super::compose::{Composer, Shape};
use super::{Brush, BrushMode};
use crate::clip::TileClip;
use crate::lattice::{Lattice, Stagger};
use crate::region::TileRegion;

/// Whether `clip` centred on `point` lands with its first row (column, where columns are
/// shifted) shifted where it was not, or the other way round. Its rows would then stand half a
/// cell the wrong way from each other; moving the rows that were shifted on by a cell keeps the
/// shape (Tiled's `shiftRows` and `shiftColumns`). A clip one row (column) deep needs no
/// fitting.
fn needs_fitting(clip: &TileClip, point: IVec2, stagger: Option<Stagger>) -> bool {
    let Some(stagger) = stagger else {
        return false;
    };
    match stagger.axis {
        StaggerAxis::Y => {
            clip.size.y > 1
                && stagger.shifts(clip.origin.y) != stagger.shifts(point.y - clip.size.y / 2)
        }
        StaggerAxis::X => {
            clip.size.x > 1
                && stagger.shifts(clip.origin.x) != stagger.shifts(point.x - clip.size.x / 2)
        }
    }
}

/// `clip`'s tiles with its shifted rows (columns) moved on a cell, in a rectangle a cell wider
/// (taller).
fn fitted(clip: &TileClip, stagger: Stagger) -> Shape {
    match stagger.axis {
        StaggerAxis::Y => Shape::moved(clip, clip.size + IVec2::X, |at| {
            if stagger.shifts(clip.origin.y + at.y) {
                at + IVec2::X
            } else {
                at
            }
        }),
        StaggerAxis::X => Shape::moved(clip, clip.size + IVec2::Y, |at| {
            if stagger.shifts(clip.origin.x + at.x) {
                at + IVec2::Y
            } else {
                at
            }
        }),
    }
}

/// The size of the rectangle `clip` covers when centred on `point`: a cell more across the
/// shifted rows (columns) when it is fitted to them.
pub(crate) fn placed_size(clip: &TileClip, point: IVec2, stagger: Option<Stagger>) -> IVec2 {
    match stagger {
        Some(stagger) if needs_fitting(clip, point, Some(stagger)) => match stagger.axis {
            StaggerAxis::Y => clip.size + IVec2::X,
            StaggerAxis::X => clip.size + IVec2::Y,
        },
        _ => clip.size,
    }
}

/// The shapes of a brush's variations, as they are and fitted, made when first needed.
struct Shapes {
    plain: Vec<Option<Shape>>,
    fitted: Vec<Option<Shape>>,
}

impl Shapes {
    fn new(count: usize) -> Shapes {
        Shapes {
            plain: vec![None; count],
            fitted: vec![None; count],
        }
    }

    /// The shape of variation `index` (`clip`), fitted to `fitting` or as it is.
    fn get(&mut self, index: usize, clip: &TileClip, fitting: Option<Stagger>) -> Option<&Shape> {
        let cache = if fitting.is_some() {
            &mut self.fitted
        } else {
            &mut self.plain
        };
        let shape = cache.get_mut(index)?.get_or_insert_with(|| match fitting {
            Some(stagger) => fitted(clip, stagger),
            None => Shape::of(clip),
        });
        Some(shape)
    }
}

/// A variation put down by a stroke.
struct Placed {
    variation: usize,
    /// The stagger it was fitted to, if it was.
    fitting: Option<Stagger>,
    top_left: IVec2,
}

impl Brush {
    /// What a click or a stroke through `points` paints on a map of `geometry`, as a clip whose
    /// origin is where it goes (paste it there); `None` when it would paint nothing. The points
    /// are first [`Brush::sized`].
    ///
    /// A stamp brush centres a variation on each point ([`TileClip::centered_at`]) unless it
    /// would overlap tiles placed earlier in the same call, which is how a line or an ellipse
    /// of a large stamp stays clean; a variation is picked by weight from `rng` before the first
    /// point and after each placement, as in Tiled. On staggered and hexagonal maps a variation
    /// landing on rows (columns) of the other parity from where it came from has its shifted
    /// rows moved on a cell, so it keeps its shape. Layers are matched by name: the n-th layer
    /// of a name in a variation goes to the n-th result layer of that name.
    ///
    /// A random brush paints one tile at each cell, chosen by the tiles' probabilities from a
    /// generator keyed by the cell and a seed drawn once from `rng`, so what is painted at a
    /// cell does not depend on the order or the company it is painted in.
    pub fn placement(
        &self,
        points: &[IVec2],
        geometry: &MapGeometry,
        rng: &mut Rng,
    ) -> Option<TileClip> {
        let points = self.sized(points);
        match self.mode {
            BrushMode::Random => self.random_clip(&points, rng),
            BrushMode::Stamp => self.stamped(&points, Lattice::of(geometry).stagger(), rng),
        }
    }

    /// The stamp brush's placement.
    fn stamped(
        &self,
        points: &[IVec2],
        stagger: Option<Stagger>,
        rng: &mut Rng,
    ) -> Option<TileClip> {
        let picker = self.variation_picker();
        let mut shapes = Shapes::new(self.variations.len());
        let mut variation = *picker.pick(rng)?;
        let mut painted = TileRegion::new();
        let mut placements = Vec::new();
        for &point in points {
            let clip = &self.variations.get(variation)?.clip;
            let fitting = stagger.filter(|_| needs_fitting(clip, point, stagger));
            let shape = shapes.get(variation, clip, fitting)?;
            let top_left = point - shape.size / 2;
            let region = shape.region.translated(top_left);
            if painted.intersects(&region) {
                continue;
            }
            for run in region.runs() {
                painted.insert_rect(run);
            }
            placements.push(Placed {
                variation,
                fitting,
                top_left,
            });
            variation = *picker.pick(rng)?;
        }
        let bounds = painted.bounds()?;
        let mut composer = Composer::new(bounds.min());
        for placed in placements {
            let clip = &self.variations.get(placed.variation)?.clip;
            let shape = shapes.get(placed.variation, clip, placed.fitting)?;
            composer.place(placed.variation, clip, shape, placed.top_left, |_| true);
        }
        composer.finish(bounds.size())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{LayerId, Map, Orientation, StaggerIndex, Tileset};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::IRect;
    use bt_utils::rng::RandomPicker;
    use glam::UVec2;

    use super::*;
    use crate::axis::Axis;
    use crate::clip::{find_target_layers, paste};
    use crate::lattice::RotateDirection;
    use crate::testutil::{assert_valid, atlas, clip_grid, every_grid, map_grid, trimmed};

    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(16))
    }

    /// A brush of one variation drawn as a grid (the fixtures' notation) on one layer.
    fn brush_of(tileset: &Arc<Tileset>, grid: &str) -> Brush {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .tileset(Arc::clone(tileset))
            .tile_layer("ground", grid)
            .build()
            .unwrap();
        let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(8));
        Brush::capture(&map, &[LayerId(1)], rect).unwrap()
    }

    #[test]
    fn a_click_centres_the_variation_on_the_point() {
        let brush = brush_of(&atlas("a"), "1 2 3\n4 . 6");
        let clip = brush
            .placement(&[IVec2::new(5, 5)], &square(), &mut Rng::new(1))
            .unwrap();
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(4, 4), IVec2::new(3, 2))
        );
        assert_eq!(clip_grid(&clip, 0), trimmed("1 2 3\n4 . 6"));
        assert_eq!(brush.placement(&[], &square(), &mut Rng::new(1)), None);
    }

    #[test]
    fn a_line_skips_copies_that_would_overlap_tiles_placed_before() {
        // Tiled's Shift-line of a 2 × 1 stamp: every other point.
        let brush = brush_of(&atlas("a"), "1 2");
        let line: Vec<IVec2> = (0..7).map(|x| IVec2::new(x, 0)).collect();
        let clip = brush.placement(&line, &square(), &mut Rng::new(1)).unwrap();
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(-1, 0), IVec2::new(8, 1))
        );
        assert_eq!(clip_grid(&clip, 0), trimmed("1 2 1 2 1 2 1 2"));
        // Overlap is of tiles, not rectangles: a stamp with a hole interleaves.
        let holey = brush_of(&atlas("a"), "1 . 2");
        let clip = holey
            .placement(&line[..2], &square(), &mut Rng::new(1))
            .unwrap();
        assert_eq!(clip_grid(&clip, 0), trimmed("1 1 2 2"));
    }

    #[test]
    fn a_new_variation_is_picked_after_each_placement() {
        let tileset = atlas("a");
        let mut brush = brush_of(&tileset, "1");
        for id in 2..5 {
            let clip = brush_of(&tileset, &id.to_string()).variations()[0]
                .clip
                .clone();
            brush.add_variation(clip, f64::from(id)).unwrap();
        }
        // The second point is the first again: skipped, with no new pick.
        let points = [IVec2::ZERO, IVec2::ZERO, IVec2::X, IVec2::new(2, 0)];
        let clip = brush
            .placement(&points, &square(), &mut Rng::new(77))
            .unwrap();
        let mut picker = RandomPicker::new();
        for (index, weight) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
            picker.add(index, weight);
        }
        let mut rng = Rng::new(77);
        let expected: Vec<String> = (0..3)
            .map(|_| (picker.pick(&mut rng).unwrap() + 1).to_string())
            .collect();
        assert_eq!(clip_grid(&clip, 0), expected.join(" "));
    }

    #[test]
    fn a_placement_pastes_into_the_map_with_its_layers_by_name() {
        let tileset = atlas("a");
        let mut source = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(Arc::clone(&tileset))
            .tile_layer("ground", "1 2\n3 4")
            .tile_layer("decor", ". 5\n. .")
            .build()
            .unwrap();
        let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(2));
        let brush = Brush::capture(&source, &[LayerId(1), LayerId(2)], rect).unwrap();
        let clip = brush
            .placement(&[IVec2::new(3, 3)], &source.geometry(), &mut Rng::new(5))
            .unwrap();
        let targets = find_target_layers(&source, &clip, Some(LayerId(1)), &[LayerId(1)]).unwrap();
        paste(
            &mut source,
            &clip,
            clip.origin,
            &targets,
            &TileRegion::new(),
        )
        .unwrap();
        assert_eq!(
            map_grid(&source, LayerId(1)),
            trimmed("1 2 . .\n3 4 . .\n. . 1 2\n. . 3 4")
        );
        assert_eq!(
            map_grid(&source, LayerId(2)),
            trimmed(". 5 . .\n. . . .\n. . . 5\n. . . .")
        );
        assert_valid(&source);
    }

    /// Cells of a staggered or hexagonal map, each tile once: (1, 2) to (2, 4) and around.
    const SHAPE: &str = "
        . 1 2 .
        3 4 . .
        . 5 6 7
        . . 8 .
    ";

    /// Where each tile of a painted or captured shape is, by tile.
    fn positions(clip: &TileClip) -> Vec<IVec2> {
        let mut cells: Vec<(u32, IVec2)> = clip.layers[0]
            .cells
            .cells()
            .map(|placed| (placed.cell.tile().0, clip.origin + placed.at))
            .collect();
        cells.sort_by_key(|(tile, _)| *tile);
        cells.into_iter().map(|(_, at)| at).collect()
    }

    #[test]
    fn a_brush_captured_on_shifted_rows_keeps_its_shape_on_either_parity() {
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            if lattice.stagger().is_none() {
                continue;
            }
            for corner in [IVec2::new(2, 3), IVec2::new(3, 2)] {
                let map = MapBuilder::new(UVec2::splat(32), UVec2::splat(12))
                    .orientation(geometry.orientation)
                    .stagger(geometry.stagger_axis, geometry.stagger_index)
                    .hex_side_length(geometry.hex_side_length)
                    .tileset(atlas("a"))
                    .tile_layer_at("ground", corner, SHAPE)
                    .build()
                    .unwrap();
                let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(12));
                let brush = Brush::capture(&map, &[LayerId(1)], rect).unwrap();
                let captured = positions(&brush.variations()[0].clip);
                for point in [
                    IVec2::new(20, 20),
                    IVec2::new(21, 20),
                    IVec2::new(20, 21),
                    IVec2::new(21, 21),
                ] {
                    let clip = brush
                        .placement(&[point], &geometry, &mut Rng::new(3))
                        .unwrap();
                    let painted = positions(&clip);
                    let step = lattice.step(captured[0], painted[0]);
                    let moved: Vec<IVec2> = captured
                        .iter()
                        .map(|&at| lattice.stepped(at, step))
                        .collect();
                    assert_eq!(painted, moved, "at {point} from {corner} on {geometry:?}");
                    let footprint = brush.footprint(point, &geometry);
                    let covered = IRect::from_min_size(clip.origin, clip.size);
                    assert_eq!(footprint.union(covered), footprint, "inside the footprint");
                }
            }
        }
    }

    #[test]
    fn random_shapes_keep_their_form_wherever_they_are_placed() {
        let fuzz = Fuzz::from_env(0x0057_A6E5, 100);
        let grids: Vec<MapGeometry> = every_grid()
            .into_iter()
            .filter(|geometry| Lattice::of(geometry).stagger().is_some())
            .collect();
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            for geometry in &grids {
                // Each tile once, so a tile names its cell.
                let (w, h) = (1 + rng.below(4).unwrap(), 1 + rng.below(4).unwrap());
                let mut next = 0;
                let grid: Vec<String> = (0..h)
                    .map(|_| {
                        let row: Vec<String> = (0..w)
                            .map(|_| {
                                if rng.below(3) == Some(0) {
                                    return ".".to_owned();
                                }
                                next += 1;
                                next.to_string()
                            })
                            .collect();
                        row.join(" ")
                    })
                    .collect();
                let corner = IVec2::new(rng.below(5).unwrap() as i32, rng.below(5).unwrap() as i32);
                let map = MapBuilder::new(UVec2::splat(32), UVec2::splat(10))
                    .orientation(geometry.orientation)
                    .stagger(geometry.stagger_axis, geometry.stagger_index)
                    .hex_side_length(geometry.hex_side_length)
                    .tileset(atlas("a"))
                    .tile_layer_at("ground", corner, &grid.join("\n"))
                    .build()
                    .unwrap();
                let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(10));
                let brush = Brush::capture(&map, &[LayerId(1)], rect).unwrap();
                // Mirrored and turned brushes keep their shapes too.
                let brush = match rng.below(5).unwrap() {
                    0 => brush.flipped(Axis::Horizontal, geometry),
                    1 => brush.flipped(Axis::Vertical, geometry),
                    2 => brush.rotated(RotateDirection::Clockwise, geometry),
                    3 => brush.rotated(RotateDirection::CounterClockwise, geometry),
                    _ => brush,
                };
                let point = IVec2::new(
                    rng.below(21).unwrap() as i32 - 10,
                    rng.below(21).unwrap() as i32 - 10,
                );
                let Some(clip) = brush.placement(&[point], geometry, &mut rng) else {
                    assert!(brush.variations()[0].clip.is_empty());
                    continue;
                };
                let captured = positions(&brush.variations()[0].clip);
                let painted = positions(&clip);
                let lattice = Lattice::of(geometry);
                let step = lattice.step(captured[0], painted[0]);
                let moved: Vec<IVec2> = captured
                    .iter()
                    .map(|&at| lattice.stepped(at, step))
                    .collect();
                assert_eq!(
                    painted,
                    moved,
                    "case {case} ({}) at {point} on {geometry:?}",
                    fuzz.reproduce()
                );
            }
        }
    }

    #[test]
    fn fitting_moves_the_rows_that_were_shifted_as_tiled_does() {
        // Odd rows shifted; captured from row 0, so its row 1 was the shifted one. Centred on
        // row 5 its top row is 4, unshifted too; centred on row 4 it is 3, which is shifted, so
        // the stamp's row 1 moves on a cell to stay below and to the right of its row 0.
        let geometry = MapGeometry {
            stagger_axis: StaggerAxis::Y,
            stagger_index: StaggerIndex::Odd,
            ..MapGeometry::new(Orientation::Staggered, IVec2::new(32, 16), IVec2::splat(9))
        };
        let brush = brush_of(&atlas("a"), "1 2\n3 4");
        let same = brush
            .placement(&[IVec2::new(4, 5)], &geometry, &mut Rng::new(1))
            .unwrap();
        assert_eq!(
            (same.origin, same.size),
            (IVec2::new(3, 4), IVec2::new(2, 2))
        );
        assert_eq!(clip_grid(&same, 0), trimmed("1 2\n3 4"));
        let other = brush
            .placement(&[IVec2::new(4, 4)], &geometry, &mut Rng::new(1))
            .unwrap();
        assert_eq!(
            (other.origin, other.size),
            (IVec2::new(3, 3), IVec2::new(3, 2))
        );
        assert_eq!(clip_grid(&other, 0), trimmed("1 2 .\n. 3 4"));
        assert_eq!(geometry.bottom_right(IVec2::new(3, 3)), IVec2::new(4, 4));
        // One row needs no fitting.
        let row = brush_of(&atlas("a"), "1 2");
        let clip = row
            .placement(&[IVec2::new(4, 5)], &geometry, &mut Rng::new(1))
            .unwrap();
        assert_eq!(clip.size, IVec2::new(2, 1));
    }

    #[test]
    fn painting_keeps_the_map_valid_on_every_grid() {
        for geometry in every_grid() {
            let tileset = atlas("a");
            let brush = brush_of(&tileset, SHAPE);
            let mut map: Map = MapBuilder::new(UVec2::splat(32), UVec2::splat(10))
                .orientation(geometry.orientation)
                .stagger(geometry.stagger_axis, geometry.stagger_index)
                .hex_side_length(geometry.hex_side_length)
                .tileset(Arc::clone(&tileset))
                .tile_layer("ground", ". .")
                .build()
                .unwrap();
            let line: Vec<IVec2> = (0..12).map(|i| IVec2::new(i, i / 2)).collect();
            let clip = brush
                .placement(&line, &map.geometry(), &mut Rng::new(9))
                .unwrap();
            let targets = find_target_layers(&map, &clip, Some(LayerId(1)), &[]).unwrap();
            paste(&mut map, &clip, clip.origin, &targets, &TileRegion::new()).unwrap();
            assert_valid(&map);
            assert!(map.layers[0].as_tile_layer().unwrap().cells().count() > 0);
        }
    }
}
