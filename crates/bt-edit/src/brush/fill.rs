//! What Bucket Fill and Shape Fill paint (Tiled's `AbstractTileFillTool::fillWithStamp` and
//! `randomFill`, `reference/tiled/src/tiled/abstracttilefilltool.cpp`).

use bt_utils::rng::Rng;
use glam::IVec2;

use super::compose::{Composer, Shape};
use super::{Brush, BrushMode};
use crate::clip::TileClip;
use crate::region::TileRegion;

impl Brush {
    /// What a fill of `region` paints, as a clip whose origin is where it goes (paste it there);
    /// `None` when it would paint nothing.
    ///
    /// A stamp brush tiles the region's bounding rectangle with variations from its top-left
    /// corner, so the pattern is anchored at the fill area's corner: row by row, each variation
    /// picked by weight from `rng` and put down beside the one before, each row as tall as the
    /// tallest variation in it; then only the cells of `region` are kept. A random brush paints
    /// one tile at each cell of `region`, as [`Brush::placement`] does.
    pub fn fill(&self, region: &TileRegion, rng: &mut Rng) -> Option<TileClip> {
        match self.mode {
            BrushMode::Random => {
                let cells: Vec<IVec2> = region.iter().collect();
                self.random_clip(&cells, rng)
            }
            BrushMode::Stamp => self.tiled_fill(region, rng),
        }
    }

    /// The stamp brush's fill.
    fn tiled_fill(&self, region: &TileRegion, rng: &mut Rng) -> Option<TileClip> {
        let bounds = region.bounds()?;
        let picker = self.variation_picker();
        if picker.is_empty() {
            return None;
        }
        let shapes: Vec<Shape> = self
            .variations
            .iter()
            .map(|variation| Shape::of(&variation.clip))
            .collect();
        let mut composer = Composer::new(bounds.min());
        let mut y = 0;
        while y < bounds.h {
            let mut row_height = 1;
            let mut x = 0;
            while x < bounds.w {
                let index = *picker.pick(rng)?;
                let variation = self.variations.get(index)?;
                let shape = shapes.get(index)?;
                row_height = row_height.max(variation.clip.size.y);
                let top_left = bounds.min() + IVec2::new(x, y);
                composer.place(index, &variation.clip, shape, top_left, |at| {
                    region.contains(at)
                });
                x += variation.clip.size.x.max(1);
            }
            y += row_height;
        }
        composer.finish(bounds.size())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{LayerId, Tileset};
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::IRect;
    use bt_utils::rng::RandomPicker;
    use glam::UVec2;

    use super::*;
    use crate::testutil::{atlas, clip_grid, drawn, trimmed};

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
    fn the_pattern_is_anchored_at_the_fill_areas_corner() {
        let brush = brush_of(&atlas("a"), "1 2\n3 4");
        let region = TileRegion::from_rect(IRect {
            x: 3,
            y: 5,
            w: 5,
            h: 3,
        });
        let clip = brush.fill(&region, &mut Rng::new(1)).unwrap();
        assert_eq!(
            (clip.origin, clip.size),
            (IVec2::new(3, 5), IVec2::new(5, 3))
        );
        assert_eq!(
            clip_grid(&clip, 0),
            trimmed(
                "
                1 2 1 2 1
                3 4 3 4 3
                1 2 1 2 1
                "
            )
        );
        let corner = drawn("###\n#..\n#..").translated(IVec2::ONE);
        let clip = brush.fill(&corner, &mut Rng::new(1)).unwrap();
        assert_eq!(clip.origin, IVec2::ONE);
        assert_eq!(clip_grid(&clip, 0), trimmed("1 2 1\n3 . .\n1 . ."));
        assert_eq!(brush.fill(&TileRegion::new(), &mut Rng::new(1)), None);
    }

    #[test]
    fn each_row_is_as_tall_as_its_tallest_variation() {
        let tileset = atlas("a");
        let mut brush = brush_of(&tileset, "1");
        let tall = brush_of(&tileset, "2\n3").variations()[0].clip.clone();
        brush.add_variation(tall, 1.0).unwrap();
        let never = brush_of(&tileset, "9").variations()[0].clip.clone();
        brush.add_variation(never, 0.0).unwrap();
        for seed in 0..20 {
            let region = TileRegion::from_rect(IRect::from_min_size(IVec2::ZERO, IVec2::new(4, 5)));
            let clip = brush.fill(&region, &mut Rng::new(seed)).unwrap();
            // Tiled's loop, by hand.
            let mut picker = RandomPicker::new();
            picker.add(1, 1.0);
            picker.add(2, 1.0);
            let mut rng = Rng::new(seed);
            let mut expected = vec![vec![".".to_owned(); 4]; 5];
            let mut y = 0;
            while y < 5 {
                let heights: Vec<usize> = (0..4).map(|_| *picker.pick(&mut rng).unwrap()).collect();
                for (x, &height) in heights.iter().enumerate() {
                    expected[y][x] = if height == 1 { "1" } else { "2" }.to_owned();
                    if height == 2 && y + 1 < 5 {
                        expected[y + 1][x] = "3".to_owned();
                    }
                }
                y += heights.iter().max().unwrap();
            }
            let expected: Vec<String> = expected.iter().map(|row| row.join(" ")).collect();
            assert_eq!(clip_grid(&clip, 0), expected.join("\n"), "seed {seed}");
        }
    }
}
