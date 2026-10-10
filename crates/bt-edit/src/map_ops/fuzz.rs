//! Random maps on every grid: resizing by an offset and back gives the same map, and random
//! sequences of map commands keep every map valid, its tile layers agreeing with it.

use std::sync::Arc;

use bt_core::{
    Cell, ImageLayer, Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Orientation,
    StaggerAxis, StaggerIndex, TileId, TileLayer, TilesetSlot,
};
use bt_test_data::Fuzz;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_utils::IRect;
use bt_utils::rng::Rng;
use glam::{DVec2, IVec2, UVec2};

use super::*;
use crate::error::EditError;
use crate::layer_ops::layer_order;
use crate::testutil::rect_object;
use crate::tile_layers::map_size;

/// A number in `low..high`.
fn between(rng: &mut Rng, low: i32, high: i32) -> i32 {
    let span = u64::try_from(high - low).unwrap_or(1).max(1);
    low + i32::try_from(rng.below(span).unwrap_or(0)).unwrap_or(0)
}

/// A map of a random grid, size and content: two tile layers (one in a group with a picture),
/// objects of several kinds. With `inside`, every tile is inside the map's size even when the
/// map is infinite.
fn random_map(rng: &mut Rng, inside: bool) -> Map {
    let size = UVec2::new(between(rng, 2, 7) as u32, between(rng, 2, 7) as u32);
    let axis = if rng.below(2) == Some(0) {
        StaggerAxis::X
    } else {
        StaggerAxis::Y
    };
    let index = if rng.below(2) == Some(0) {
        StaggerIndex::Odd
    } else {
        StaggerIndex::Even
    };
    let mut builder = match rng.below(5).unwrap_or(0) {
        0 => MapBuilder::new(UVec2::splat(16), size),
        1 => MapBuilder::new(UVec2::new(32, 16), size).orientation(Orientation::Isometric),
        2 => MapBuilder::new(UVec2::new(64, 32), size)
            .orientation(Orientation::Staggered)
            .stagger(axis, index),
        3 => MapBuilder::new(UVec2::splat(32), size)
            .orientation(Orientation::Hexagonal)
            .hex_side_length(16)
            .stagger(axis, index),
        _ => MapBuilder::new(UVec2::splat(16), size)
            .orientation(Orientation::Oblique)
            .skew(IVec2::new(4, 2)),
    };
    let infinite = rng.below(4) == Some(0);
    if infinite {
        builder = builder.infinite();
    }
    let atlas = Atlas::builder().build_embedded().unwrap();
    let ground = random_tiles(rng, size, infinite, inside);
    let inner = random_tiles(rng, size, infinite, inside);
    let picture = Layer {
        offset: DVec2::new(
            f64::from(between(rng, -20, 20)),
            f64::from(between(rng, -20, 20)),
        ),
        ..Layer::new(
            LayerId(0),
            "picture",
            LayerKind::Image(ImageLayer::default()),
        )
    };
    let tile = Cell::new(TilesetSlot::new(0).unwrap(), TileId(3));
    let objects: Vec<MapObject> = (0..between(rng, 0, 5))
        .map(|_| {
            let x = f64::from(between(rng, -40, 120));
            let y = f64::from(between(rng, -40, 120));
            let rotation = f64::from(between(rng, 0, 4) * 90);
            MapObject {
                rotation,
                tile: (rng.below(3) == Some(0)).then_some(tile),
                ..rect_object(x, y, 16.0, 8.0)
            }
        })
        .collect();
    builder
        .tileset(atlas.tileset)
        .layer(Layer::new(LayerId(0), "ground", LayerKind::Tile(ground)))
        .layer(crate::testutil::group(
            "group",
            vec![
                Layer::new(LayerId(0), "inner", LayerKind::Tile(inner)),
                picture,
            ],
        ))
        .layer(Layer::new(
            LayerId(0),
            "things",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(objects),
                ..ObjectLayer::default()
            }),
        ))
        .build()
        .unwrap()
}

fn random_tiles(rng: &mut Rng, size: UVec2, infinite: bool, inside: bool) -> TileLayer {
    let mut tiles = if infinite {
        TileLayer::infinite(size)
    } else {
        TileLayer::finite(size)
    };
    let spill = if infinite && !inside { 20 } else { 0 };
    let (w, h) = (size.x as i32, size.y as i32);
    for _ in 0..between(rng, 0, 12) {
        let at = IVec2::new(
            between(rng, -spill, w + spill),
            between(rng, -spill, h + spill),
        );
        let cell = Cell::new(
            TilesetSlot::new(0).unwrap(),
            TileId(between(rng, 0, 16) as u32),
        );
        tiles.set_cell(at, cell).unwrap();
    }
    tiles
}

/// The map breaks no invariant and every tile layer has its size and bounds.
fn assert_consistent(map: &Map, context: &dyn Fn() -> String) {
    assert_eq!(map.validate(), [], "{}", context());
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            assert_eq!(tiles.size(), map_size(map), "{}", context());
            assert_eq!(tiles.is_infinite(), map.infinite, "{}", context());
        }
    });
}

#[test]
fn resizing_by_an_offset_and_back_keeps_the_map() {
    let fuzz = Fuzz::from_env(0x2E51_2E00, 300);
    for case in 0..fuzz.cases {
        let mut rng = Rng::new(fuzz.case_seed(case));
        let mut map = random_map(&mut rng, true);
        let original = map.clone();
        let mut offset = IVec2::new(between(&mut rng, 0, 4), between(&mut rng, 0, 4));
        // An odd step along a staggered axis changes which rows are shifted: objects follow the
        // new first cell and do not come back (Tiled's rule).
        if matches!(
            map.orientation,
            Orientation::Staggered | Orientation::Hexagonal
        ) {
            match map.stagger_axis {
                StaggerAxis::X => offset.x -= offset.x % 2,
                StaggerAxis::Y => offset.y -= offset.y % 2,
            }
        }
        let size = map_size(&map);
        let context = || {
            format!(
                "case {case} ({}): {:?} {:?} {:?} by {offset}",
                fuzz.reproduce(),
                original.orientation,
                original.stagger_axis,
                original.stagger_index
            )
        };
        resize_map(&mut map, size + offset.as_uvec2(), offset, false).unwrap();
        assert_consistent(&map, &context);
        resize_map(&mut map, size, -offset, false).unwrap();
        assert_eq!(map, original, "{}", context());
    }
}

/// A random rectangle of cells near the map.
fn random_rect(rng: &mut Rng) -> IRect {
    IRect {
        x: between(rng, -3, 6),
        y: between(rng, -3, 6),
        w: between(rng, 0, 6),
        h: between(rng, 0, 6),
    }
}

/// Runs a random map command; what it was, and whether it was refused.
struct Ran {
    command: String,
    refused: bool,
}

fn random_command(map: &mut Map, rng: &mut Rng) -> Ran {
    let refused = |result: Result<(), EditError>| result.is_err();
    match rng.below(7).unwrap_or(0) {
        0 => {
            let size = UVec2::new(between(rng, 0, 8) as u32, between(rng, 0, 8) as u32);
            let offset = IVec2::new(between(rng, -3, 4), between(rng, -3, 4));
            let remove = rng.below(2) == Some(0);
            Ran {
                command: format!("resize to {size} by {offset}, removing {remove}"),
                refused: refused(resize_map(map, size, offset, remove).map(|_| ())),
            }
        }
        1 => {
            let rect = random_rect(rng);
            Ran {
                command: format!("crop to {rect:?}"),
                refused: refused(crop_to(map, rect).map(|_| ())),
            }
        }
        2 => Ran {
            command: "autocrop".to_owned(),
            refused: refused(autocrop(map).map(|_| ())),
        },
        3 => {
            let layer = LayerId(between(rng, 1, 7) as u32);
            Ran {
                command: format!("autocrop to {layer}"),
                refused: refused(autocrop_to_layer(map, layer).map(|_| ())),
            }
        }
        4 => {
            let layers: Vec<LayerId> = layer_order(map)
                .into_iter()
                .filter(|_| rng.below(2) == Some(0))
                .collect();
            let area = (rng.below(2) == Some(0)).then(|| random_rect(rng));
            let offset = IVec2::new(between(rng, -5, 6), between(rng, -5, 6));
            let wrap_x = rng.below(2) == Some(0);
            let wrap_y = rng.below(2) == Some(0);
            Ran {
                command: format!("offset {layers:?} in {area:?} by {offset}, {wrap_x} {wrap_y}"),
                refused: refused(
                    offset_map(map, &layers, area, offset, wrap_x, wrap_y).map(|_| ()),
                ),
            }
        }
        5 => Ran {
            command: "infinite".to_owned(),
            refused: refused(make_infinite(map).map(|_| ())),
        },
        _ => Ran {
            command: "finite".to_owned(),
            refused: refused(make_finite(map).map(|_| ())),
        },
    }
}

#[test]
fn random_map_commands_keep_the_map_valid() {
    let fuzz = Fuzz::from_env(0x3A9_C0DE, 300);
    for case in 0..fuzz.cases {
        let mut rng = Rng::new(fuzz.case_seed(case));
        let mut map = random_map(&mut rng, false);
        let mut done: Vec<String> = Vec::new();
        for _ in 0..10 {
            let before = map.clone();
            let ran = random_command(&mut map, &mut rng);
            done.push(ran.command);
            let context = || format!("case {case} ({}) after {done:?}", fuzz.reproduce());
            if ran.refused {
                assert_eq!(map, before, "a refusal: {}", context());
            }
            assert_consistent(&map, &context);
        }
    }
}
