//! What the tiles of an atlas look like near each lane: the mean colour, which detection compares,
//! and the colours, which Label All compares.

use std::collections::BTreeMap;

use bt_core::{TileId, Tileset, WangId, WangSetType};
use bt_image::DecodedImage;
use bt_utils::IRect;
use glam::UVec2;

use super::histogram::Histogram;
use super::{LAYOUTS, block_tile};
use crate::lanes::kind_lanes;
use crate::pixels::{MeanColor, mean_color, region_pixels};

/// What the tiles of an atlas look like near each corner and edge, sampled once per image.
#[derive(Clone, Debug)]
pub struct Samples {
    columns: u32,
    rows: u32,
    /// By tile id; `None` for a tile the image does not have.
    tiles: Vec<Option<TileLanes>>,
}

/// What one tile looks like near each of its eight lanes.
#[derive(Clone, Debug)]
struct TileLanes {
    means: [MeanColor; 8],
    colors: [Histogram; 8],
}

/// The colours of the two sides of a block: its terrain, and what the terrain is drawn over.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SideColors {
    /// The colours near the lanes the layout gives the terrain.
    pub terrain: Histogram,
    /// The colours near the other lanes.
    pub under: Histogram,
}

impl Samples {
    /// Samples the tiles of an atlas `columns` tiles wide from `image`: `rects` gives each tile's
    /// rectangle in the image, by tile id (`None` for a tile the image lacks); a tile is
    /// `tile_size` pixels.
    pub fn new(
        image: &DecodedImage,
        tile_size: UVec2,
        columns: u32,
        rects: impl IntoIterator<Item = Option<IRect>>,
    ) -> Samples {
        let tiles: Vec<Option<TileLanes>> = rects
            .into_iter()
            .map(|rect| rect.map(|rect| TileLanes::sample(image, tile_size, rect)))
            .collect();
        let count = u32::try_from(tiles.len()).unwrap_or(u32::MAX);
        let rows = if columns == 0 {
            0
        } else {
            count.div_ceil(columns)
        };
        Samples {
            columns,
            rows,
            tiles,
        }
    }

    /// Samples the tiles of an atlas tileset from its decoded image. A collection of images has
    /// no grid, so no layout fits it: its samples are empty.
    pub fn from_tileset(tileset: &Tileset, image: &DecodedImage) -> Samples {
        let rects = (0..tileset.tile_count()).map(|id| {
            if tileset.is_collection() {
                None
            } else {
                tileset.tile_image(TileId(id)).map(|tile| tile.rect)
            }
        });
        let columns = if tileset.is_collection() {
            0
        } else {
            tileset.columns()
        };
        Samples::new(image, tileset.tile_size(), columns, rects)
    }

    /// The atlas's size in tiles: columns and rows.
    pub fn size(&self) -> UVec2 {
        UVec2::new(self.columns, self.rows)
    }

    /// The mean colours near each lane of a tile.
    pub(crate) fn means(&self, tile: TileId) -> Option<&[MeanColor; 8]> {
        let lanes = self.tiles.get(usize::try_from(tile.0).ok()?)?.as_ref()?;
        Some(&lanes.means)
    }

    /// The colours near each lane of a tile.
    fn colors(&self, tile: TileId) -> Option<&[Histogram; 8]> {
        let lanes = self.tiles.get(usize::try_from(tile.0).ok()?)?.as_ref()?;
        Some(&lanes.colors)
    }

    /// The colours of the two sides of the block of layout `layout` (an index into
    /// [`LAYOUTS`]) at `origin`: its terrain, and what that is drawn over.
    pub fn side_colors(&self, layout: usize, origin: UVec2) -> SideColors {
        let mut sides = SideColors::default();
        let Some(layout) = LAYOUTS.get(layout) else {
            return sides;
        };
        let width = layout.size().x;
        for (index, slot) in layout.slots().into_iter().enumerate() {
            let Some(slot) = slot else { continue };
            let Some(colors) =
                block_tile(origin, self.columns, width, index).and_then(|tile| self.colors(tile))
            else {
                continue;
            };
            for &lane in kind_lanes(layout.kind) {
                let side = if slot.is_terrain(lane) {
                    &mut sides.terrain
                } else {
                    &mut sides.under
                };
                if let Some(lane_colors) = colors.get(lane) {
                    side.add(lane_colors);
                }
            }
        }
        sides
    }

    /// The colours of each terrain of a set of `kind` with `terrains` terrains, as its labels
    /// show them on these tiles: terrain `n` at `n - 1`, `None` for a terrain no tile shows.
    pub fn terrain_colors(
        &self,
        kind: WangSetType,
        labels: &BTreeMap<TileId, WangId>,
        terrains: usize,
    ) -> Vec<Option<Histogram>> {
        let mut known: Vec<Option<Histogram>> = vec![None; terrains];
        for (&tile, label) in labels {
            let Some(colors) = self.colors(tile) else {
                continue;
            };
            let values = label.lanes();
            for &lane in kind_lanes(kind) {
                let terrain = values.get(lane).copied().unwrap_or(0);
                let Some(slot) = usize::from(terrain)
                    .checked_sub(1)
                    .and_then(|index| known.get_mut(index))
                else {
                    continue;
                };
                if let Some(lane_colors) = colors.get(lane) {
                    slot.get_or_insert_default().add(lane_colors);
                }
            }
        }
        known
    }
}

impl TileLanes {
    fn sample(image: &DecodedImage, tile_size: UVec2, rect: IRect) -> TileLanes {
        let region = |lane: usize| sample_region(lane, tile_size).translated(rect.min());
        TileLanes {
            means: std::array::from_fn(|lane| mean_color(image, region(lane))),
            colors: std::array::from_fn(|lane| {
                Histogram::of_pixels(region_pixels(image, region(lane)))
            }),
        }
    }
}

/// Where a lane is sampled in a tile of `size`, relative to the tile: a quarter of the tile at
/// its corner, or the middle of its side, where the lane's terrain shows even on blob tiles
/// (whose inner corners are small notches).
fn sample_region(lane: usize, size: UVec2) -> IRect {
    let w = i32::try_from(size.x).unwrap_or(i32::MAX);
    let h = i32::try_from(size.y).unwrap_or(i32::MAX);
    let rect = |x: i32, y: i32, w: i32, h: i32| IRect { x, y, w, h };
    let (corner_w, corner_h) = ((w / 4).max(1), (h / 4).max(1));
    let (across_w, across_h) = ((w * 2 / 5).max(1), (h / 5).max(1));
    let (side_w, side_h) = ((w / 5).max(1), (h * 2 / 5).max(1));
    match lane {
        0 => rect((w - across_w) / 2, 0, across_w, across_h),
        1 => rect(w - corner_w, 0, corner_w, corner_h),
        2 => rect(w - side_w, (h - side_h) / 2, side_w, side_h),
        3 => rect(w - corner_w, h - corner_h, corner_w, corner_h),
        4 => rect((w - across_w) / 2, h - across_h, across_w, across_h),
        5 => rect(0, h - corner_h, corner_w, corner_h),
        6 => rect(0, (h - side_h) / 2, side_w, side_h),
        _ => rect(0, 0, corner_w, corner_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_are_sampled_near_their_corner_or_side() {
        let size = UVec2::new(20, 20);
        let r = |lane| sample_region(lane, size);
        assert_eq!(
            r(7),
            IRect {
                x: 0,
                y: 0,
                w: 5,
                h: 5
            }
        );
        assert_eq!(
            r(1),
            IRect {
                x: 15,
                y: 0,
                w: 5,
                h: 5
            }
        );
        assert_eq!(
            r(0),
            IRect {
                x: 6,
                y: 0,
                w: 8,
                h: 4
            }
        );
        assert_eq!(
            r(2),
            IRect {
                x: 16,
                y: 6,
                w: 4,
                h: 8
            }
        );
        assert_eq!(
            r(4),
            IRect {
                x: 6,
                y: 16,
                w: 8,
                h: 4
            }
        );
        assert_eq!(
            r(6),
            IRect {
                x: 0,
                y: 6,
                w: 4,
                h: 8
            }
        );
        assert_eq!(
            r(3),
            IRect {
                x: 15,
                y: 15,
                w: 5,
                h: 5
            }
        );
        assert_eq!(
            r(5),
            IRect {
                x: 0,
                y: 15,
                w: 5,
                h: 5
            }
        );
        assert_eq!(
            sample_region(1, UVec2::new(2, 2)),
            IRect {
                x: 1,
                y: 0,
                w: 1,
                h: 1
            },
            "never empty"
        );
    }
}
