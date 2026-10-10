//! Tiled's random mode (`StampBrush::updateRandomList` and `AbstractTileFillTool::randomFill`,
//! `reference/tiled/src/tiled/stampbrush.cpp`, `abstracttilefilltool.cpp`): one tile a cell,
//! chosen among every tile of every variation, each as often as it occurs, weighted by its
//! tile's probability. The choice for a cell comes from a generator keyed by the cell and a seed
//! (`bt_utils::rng::cell_rng`) rather than one sequence, so it does not depend on the order or
//! the company cells are painted in, and a preview is what gets painted.

use bt_core::Cell;
use bt_utils::IRect;
use bt_utils::rng::{RandomPicker, Rng, cell_rng};
use glam::IVec2;

use super::Brush;
use crate::clip::{ClipLayer, ClipTilesets, TileClip};

/// What random mode chooses from.
struct RandomTiles {
    /// Every tile, with the slots of `tilesets`.
    picker: RandomPicker<Cell>,
    tilesets: ClipTilesets,
    /// The layer the tiles are painted as: the brush's first layer, without its cells.
    layer: ClipLayer,
}

impl Brush {
    /// Every tile of every variation, weighted by its tile's probability (1 for a tile its
    /// tileset keeps nothing about); `None` when the brush has no tile to choose.
    fn random_tiles(&self) -> Option<RandomTiles> {
        let mut picker = RandomPicker::new();
        let mut tilesets = ClipTilesets::new();
        let mut first_layer = None;
        for variation in &self.variations {
            let clip = &variation.clip;
            for layer in &clip.layers {
                first_layer.get_or_insert_with(|| layer.emptied());
                for placed in layer.cells.cells() {
                    let probability = placed
                        .cell
                        .slot()
                        .and_then(|slot| clip.tilesets.get(slot.index()))
                        .and_then(|tileset| tileset.tileset.tile(placed.cell.tile()))
                        .map_or(1.0, |data| data.probability);
                    // A brush's cells name tilesets its clips list (its constructors check).
                    let Ok(cell) = tilesets.adopt(&clip.tilesets, placed.cell) else {
                        continue;
                    };
                    picker.add(cell, probability);
                }
            }
        }
        if picker.is_empty() {
            return None;
        }
        Some(RandomTiles {
            picker,
            tilesets,
            layer: first_layer?,
        })
    }

    /// A random tile at each of `cells`, as a clip of one layer whose origin is where it goes;
    /// `None` when there are no cells or no tiles. The seed is drawn from `rng` once.
    pub(crate) fn random_clip(&self, cells: &[IVec2], rng: &mut Rng) -> Option<TileClip> {
        let seed = rng.next_u64();
        let tiles = self.random_tiles()?;
        let bounds = IRect::bounding(cells.iter().copied())?;
        let mut layer = tiles.layer;
        for &at in cells {
            let cell = *tiles.picker.pick(&mut cell_rng(seed, at))?;
            layer.put(at - bounds.min(), cell);
        }
        Some(TileClip {
            origin: bounds.min(),
            size: bounds.size(),
            layers: vec![layer],
            tilesets: tiles.tilesets.into_list(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{LayerId, MapGeometry, Orientation, TileData, TileId, Tileset};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;
    use crate::brush::BrushMode;
    use crate::region::TileRegion;
    use crate::testutil::{atlas, clip_grid, tile};

    /// A random brush of tiles 1, 2 and 3 (3 twice), where tile 2 has probability 0 and tile 1
    /// probability 3.
    fn weighted() -> Brush {
        let mut tileset: Tileset = atlas("a").as_ref().clone();
        let weight = |probability| TileData {
            probability,
            ..TileData::default()
        };
        tileset.tiles.insert(TileId(1), weight(3.0));
        tileset.tiles.insert(TileId(2), weight(0.0));
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", "1 2\n3 3")
            .build()
            .unwrap();
        let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(4));
        Brush::capture(&map, &[LayerId(1)], rect)
            .unwrap()
            .with_mode(BrushMode::Random)
    }

    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(16))
    }

    #[test]
    fn each_cell_is_chosen_by_its_seed_and_position_among_the_weighted_tiles() {
        let brush = weighted();
        let points: Vec<IVec2> = (0..40).map(|i| IVec2::new(i % 8, i / 8)).collect();
        let clip = brush
            .placement(&points, &square(), &mut Rng::new(12))
            .unwrap();
        assert_eq!(clip.layers.len(), 1);
        assert_eq!(clip.layers[0].name, "ground");
        let seed = Rng::new(12).next_u64();
        let mut picker = RandomPicker::new();
        picker.add(tile(1), 3.0);
        picker.add(tile(3), 1.0);
        picker.add(tile(3), 1.0);
        for &at in &points {
            let expected = *picker.pick(&mut cell_rng(seed, at)).unwrap();
            assert_eq!(
                clip.layers[0].cells.cell(at - clip.origin),
                expected,
                "{at}"
            );
        }
        let painted: Vec<Cell> = clip.layers[0].cells.cells().map(|p| p.cell).collect();
        assert!(painted.contains(&tile(1)) && painted.contains(&tile(3)));
        assert!(!painted.contains(&tile(2)), "probability 0 is never chosen");
    }

    #[test]
    fn the_same_seed_and_cells_give_the_same_tiles_whatever_the_order() {
        let brush = weighted();
        let fuzz = Fuzz::from_env(0x005E_ED0F, 100);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let mut points: Vec<IVec2> = (0..12)
                .map(|_| {
                    IVec2::new(
                        rng.below(20).unwrap() as i32 - 10,
                        rng.below(20).unwrap() as i32 - 10,
                    )
                })
                .collect();
            let seed = rng.next_u64();
            let forward = brush.placement(&points, &square(), &mut Rng::new(seed));
            points.reverse();
            let backward = brush.placement(&points, &square(), &mut Rng::new(seed));
            assert_eq!(forward, backward, "case {case} ({})", fuzz.reproduce());
            // A fill of the same cells paints them the same.
            let region: TileRegion = points.iter().copied().collect();
            let filled = brush.fill(&region, &mut Rng::new(seed));
            assert_eq!(filled, forward, "case {case} ({})", fuzz.reproduce());
            // So does each cell alone.
            let forward = forward.unwrap();
            for &at in &points {
                let alone = brush
                    .placement(&[at], &square(), &mut Rng::new(seed))
                    .unwrap();
                assert_eq!(
                    alone.layers[0].cells.cell(IVec2::ZERO),
                    forward.layers[0].cells.cell(at - forward.origin)
                );
            }
        }
    }

    #[test]
    fn a_random_brush_with_a_size_paints_squares() {
        let brush = weighted().with_size(3);
        let clip = brush
            .placement(&[IVec2::new(5, 5)], &square(), &mut Rng::new(4))
            .unwrap();
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(4, 4), IVec2::splat(3))
        );
        assert_eq!(clip.layers[0].cells.cells().count(), 9);
        assert!(!clip_grid(&clip, 0).contains('.'));
    }

    #[test]
    fn a_brush_without_tiles_paints_nothing_at_random() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(atlas("a"))
            .tile_layer("ground", ". .")
            .build()
            .unwrap();
        let rect = IRect::from_min_size(IVec2::ZERO, IVec2::splat(4));
        let brush = Brush::capture(&map, &[LayerId(1)], rect)
            .unwrap()
            .with_mode(BrushMode::Random);
        let mut rng = Rng::new(1);
        assert_eq!(brush.placement(&[IVec2::ZERO], &square(), &mut rng), None);
        assert_eq!(brush.fill(&TileRegion::from_rect(rect), &mut rng), None);
    }
}
