//! Whether a terrain's edge tiles are opaque (`10-agents.md`, §10.4, §11): whether the tiles
//! where the terrain meets nothing draw their own ground there, like a frozen lake whose edges
//! show snow, and so hide what is under them. Measured on a few edge tiles: under each empty
//! lane, the outer quarter of the tile toward that lane is at least nine tenths opaque. The
//! catalog says it of each terrain, and the checks of what such edges lie over use it.

use bt_core::{Lane, Map, TileId};
use serde::{Deserialize, Serialize};

use crate::names::NamedTerrain;
use crate::tile_pictures::{TilePictures, TilePixels};

/// The most edge tiles measured.
const MEASURED: usize = 6;

/// The alpha at which a pixel counts as opaque.
const OPAQUE_ALPHA: u8 = 128;

/// The share of the measured pixels that must be opaque, in tenths.
const OPAQUE_TENTHS: usize = 9;

/// What a terrain's edge tiles are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EdgeOpacity {
    /// The set has no tile with the terrain and an empty lane.
    NoEdges,
    /// They draw their own ground where the terrain ends.
    Opaque,
    /// What is under them shows through where the terrain ends.
    SeeThrough,
    /// Their images can't be read.
    Unmeasured {
        /// Why.
        why: String,
    },
}

/// Measures terrains' edge tiles, decoding each image once.
#[derive(Debug, Default)]
pub struct EdgeMeter {
    pictures: TilePictures,
}

/// A part of a tile, in quarters of its width and height: columns `left..right`, rows
/// `top..bottom`.
#[derive(Clone, Copy, Debug)]
struct Quarters {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// A tile with the terrain and lanes of nothing.
struct EdgeTile {
    id: TileId,
    /// Its lanes that are empty.
    empty: Vec<Lane>,
}

/// Opaque pixels of all measured.
#[derive(Clone, Copy, Debug, Default)]
struct Count {
    opaque: usize,
    total: usize,
}

impl EdgeMeter {
    /// No image decoded yet.
    pub fn new() -> EdgeMeter {
        EdgeMeter::default()
    }

    /// Whether `terrain`'s edge tiles on `map` are opaque.
    pub fn measure(&mut self, map: &Map, terrain: &NamedTerrain) -> EdgeOpacity {
        let Some(tileset) = map.tileset(terrain.slot) else {
            return EdgeOpacity::Unmeasured {
                why: "its tileset is not the map's".to_owned(),
            };
        };
        let Some(set) = tileset.wang_sets.get(terrain.set) else {
            return EdgeOpacity::Unmeasured {
                why: "its terrain set is gone".to_owned(),
            };
        };
        let lanes: Vec<Lane> = Lane::ALL
            .into_iter()
            .filter(|lane| set.kind.uses(*lane))
            .collect();
        let edge_tiles: Vec<EdgeTile> = set
            .tiles
            .iter()
            .filter_map(|(&id, label)| {
                let has = lanes
                    .iter()
                    .any(|&lane| label.lane(lane) == terrain.color.0);
                let empty: Vec<Lane> = lanes
                    .iter()
                    .copied()
                    .filter(|&lane| label.lane(lane) == 0)
                    .collect();
                (has && !empty.is_empty()).then_some(EdgeTile { id, empty })
            })
            .collect();
        if edge_tiles.is_empty() {
            return EdgeOpacity::NoEdges;
        }
        let mut count = Count::default();
        let mut measured = 0;
        let mut unreadable: Option<String> = None;
        for EdgeTile { id, empty } in edge_tiles {
            if measured >= MEASURED {
                break;
            }
            let pixels = match self.pictures.tile(tileset, &tileset.name, id) {
                Ok(Some(pixels)) => pixels,
                Ok(None) => continue,
                Err(error) => {
                    unreadable.get_or_insert(error.to_string());
                    continue;
                }
            };
            measured += 1;
            for lane in empty {
                count_part(&pixels, quarters_under(lane), &mut count);
            }
        }
        if count.total == 0 {
            return EdgeOpacity::Unmeasured {
                why: unreadable.unwrap_or_else(|| "its edge tiles have no image".to_owned()),
            };
        }
        if count.opaque * 10 >= count.total * OPAQUE_TENTHS {
            EdgeOpacity::Opaque
        } else {
            EdgeOpacity::SeeThrough
        }
    }
}

/// The outer quarter of a tile toward a lane: a corner's quarter of its quadrant, an edge's
/// middle half of the strip along it.
fn quarters_under(lane: Lane) -> Quarters {
    let part = |left, top, right, bottom| Quarters {
        left,
        top,
        right,
        bottom,
    };
    match lane {
        Lane::TopLeft => part(0, 0, 1, 1),
        Lane::Top => part(1, 0, 3, 1),
        Lane::TopRight => part(3, 0, 4, 1),
        Lane::Right => part(3, 1, 4, 3),
        Lane::BottomRight => part(3, 3, 4, 4),
        Lane::Bottom => part(1, 3, 3, 4),
        Lane::BottomLeft => part(0, 3, 1, 4),
        Lane::Left => part(0, 1, 1, 3),
    }
}

/// Counts the opaque pixels of a part of a tile.
fn count_part(pixels: &TilePixels, quarters: Quarters, count: &mut Count) {
    let (w, h) = (pixels.rect.w, pixels.rect.h);
    for y in h * quarters.top / 4..h * quarters.bottom / 4 {
        for x in w * quarters.left / 4..w * quarters.right / 4 {
            if let Some([_, _, _, alpha]) = pixels.pixel(x, y) {
                count.total += 1;
                if alpha >= OPAQUE_ALPHA {
                    count.opaque += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{
        Atlas, MapBuilder, TerrainSetBuilder, blank_picture, corners, paint_lanes,
    };
    use glam::UVec2;

    use super::*;
    use crate::names::Names;

    /// A corner set of water over nothing: tile 0 all water, tiles 1 and 2 water at the top
    /// half; the edges of tile 1 are painted solid, tile 2's left see-through.
    fn ice_map(opaque_edges: bool) -> Map {
        let mut picture = blank_picture(48, 16);
        let water = Some([40, 90, 200, 255]);
        let snow = Some([240, 240, 250, 255]);
        for tile in 0..3u32 {
            let bottom = if tile == 0 {
                water
            } else if opaque_edges {
                snow
            } else {
                None
            };
            // Lanes 7 and 1 are the top corners, 5 and 3 the bottom ones.
            let mut colors = [None; 9];
            colors[1] = water;
            colors[7] = water;
            colors[3] = bottom;
            colors[5] = bottom;
            paint_lanes(
                &mut picture,
                UVec2::new(tile * 16, 0),
                16,
                WangSetType::Corner,
                colors,
            );
        }
        let atlas = Atlas::builder()
            .name("ice")
            .grid(3, 1)
            .picture(picture)
            .build_embedded()
            .unwrap();
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Ice")
            .terrains(&["Frozen"])
            .label(0, corners([1, 1, 1, 1]))
            .label(1, corners([1, 0, 0, 1]))
            .label(2, corners([1, 0, 0, 1]));
        let tileset: Arc<Tileset> = Arc::new(set.added_to(Tileset::clone(&atlas.tileset)));
        MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(tileset)
            .build()
            .unwrap()
    }

    #[test]
    fn edges_that_draw_their_own_ground_are_opaque() {
        for (opaque, expected) in [
            (true, EdgeOpacity::Opaque),
            (false, EdgeOpacity::SeeThrough),
        ] {
            let map = ice_map(opaque);
            let names = Names::of(&map);
            let frozen = names.terrain("frozen").unwrap();
            assert_eq!(EdgeMeter::new().measure(&map, frozen), expected);
        }
        let plain = TerrainSetBuilder::new(WangSetType::Corner)
            .terrains(&["Stone"])
            .label(0, corners([1, 1, 1, 1]));
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
            .tileset(Arc::new(plain.added_to(Tileset::clone(&atlas.tileset))))
            .build()
            .unwrap();
        let names = Names::of(&map);
        assert_eq!(
            EdgeMeter::new().measure(&map, &names.terrains()[0]),
            EdgeOpacity::NoEdges
        );
    }
}
