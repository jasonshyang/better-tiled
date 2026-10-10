//! What the crate's tests share: regions drawn as text, cells of the first tileset, and maps of
//! the fixture tilesets.

use bt_core::{Cell, TileId, TilesetSlot};
use glam::IVec2;

use crate::region::TileRegion;

/// A region drawn as rows of `#` (in) and `.` (out) from (0, 0); blank lines are skipped.
pub(crate) fn drawn(text: &str) -> TileRegion {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .zip(0..)
        .flat_map(|(line, y)| {
            line.trim()
                .chars()
                .zip(0..)
                .filter(|(c, _)| *c == '#')
                .map(move |(_, x)| IVec2::new(x, y))
        })
        .collect()
}

/// Tile `id` of the tileset in `slot`.
pub(crate) fn tile_of(slot: usize, id: u32) -> Cell {
    Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id))
}

/// Tile `id` of the first tileset.
pub(crate) fn tile(id: u32) -> Cell {
    tile_of(0, id)
}

/// The cells of a tile layer in `rect` as an ASCII grid (the fixtures' notation): what a test
/// compares a layer against.
pub(crate) fn grid_of(layer: &bt_core::TileLayer, rect: bt_utils::IRect) -> String {
    (rect.y..rect.y + rect.h)
        .map(|y| {
            (rect.x..rect.x + rect.w)
                .map(|x| cell_token(layer.cell(IVec2::new(x, y))))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell_token(cell: Cell) -> String {
    let Some(slot) = cell.slot() else {
        return ".".to_owned();
    };
    let mut token = cell.tile().0.to_string();
    if slot.index() != 0 {
        token.push_str(&format!("@{}", slot.index()));
    }
    for (flag, letter) in [
        (bt_core::Flip::H, 'h'),
        (bt_core::Flip::V, 'v'),
        (bt_core::Flip::D, 'd'),
        (bt_core::Flip::HEX120, 'x'),
    ] {
        if cell.flip().contains(flag) {
            token.push(letter);
        }
    }
    token
}

/// The text of a grid with its lines trimmed, so a test can indent the grid it expects.
pub(crate) fn trimmed(grid: &str) -> String {
    grid.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A grid of every shape: orthogonal, isometric, oblique, and staggered and hexagonal along both
/// axes with both indexes.
pub(crate) fn every_grid() -> Vec<bt_core::MapGeometry> {
    use bt_core::{MapGeometry, Orientation, StaggerAxis, StaggerIndex};
    let size = IVec2::new(12, 10);
    let mut grids = vec![
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), size),
        MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), size),
        MapGeometry {
            skew: IVec2::new(8, 0),
            ..MapGeometry::new(Orientation::Oblique, IVec2::splat(16), size)
        },
    ];
    for orientation in [Orientation::Staggered, Orientation::Hexagonal] {
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            for index in [StaggerIndex::Odd, StaggerIndex::Even] {
                grids.push(MapGeometry {
                    stagger_axis: axis,
                    stagger_index: index,
                    hex_side_length: if orientation == Orientation::Hexagonal {
                        16
                    } else {
                        0
                    },
                    ..MapGeometry::new(orientation, IVec2::new(32, 32), size)
                });
            }
        }
    }
    grids
}

/// A drawn atlas tileset called `name` (16×16 tiles, 4 by 4).
pub(crate) fn atlas(name: &str) -> std::sync::Arc<bt_core::Tileset> {
    bt_test_data::fixtures::Atlas::builder()
        .name(name)
        .build_embedded()
        .unwrap()
        .tileset
}

/// The cells of the tile layer with `id`.
pub(crate) fn tiles(map: &bt_core::Map, id: bt_core::LayerId) -> &bt_core::TileLayer {
    map.layer(id)
        .and_then(bt_core::Layer::as_tile_layer)
        .unwrap()
}

/// The cells of the tile layer with `id` over the map's area, as a grid.
pub(crate) fn map_grid(map: &bt_core::Map, id: bt_core::LayerId) -> String {
    let size = IVec2::new(map.width as i32, map.height as i32);
    grid_of(
        tiles(map, id),
        bt_utils::IRect::from_min_size(IVec2::ZERO, size),
    )
}

/// The cells of a clip's layer over the clip's rectangle, as a grid.
pub(crate) fn clip_grid(clip: &crate::clip::TileClip, layer: usize) -> String {
    grid_of(
        &clip.layers[layer].cells,
        bt_utils::IRect::from_min_size(IVec2::ZERO, clip.size),
    )
}

/// Asserts that a map breaks no invariant.
#[track_caller]
pub(crate) fn assert_valid(map: &bt_core::Map) {
    let violations = map.validate();
    assert!(violations.is_empty(), "{violations:?}");
}

/// The layer tree as text: `id:name` per layer, bottom first, a group's layers in brackets after
/// it. What a test compares a tree against.
pub(crate) fn tree(map: &bt_core::Map) -> String {
    fn walk(layers: &[bt_core::Layer]) -> String {
        layers
            .iter()
            .map(|layer| match &layer.kind {
                bt_core::LayerKind::Group(children) => {
                    format!("{}:{}[{}]", layer.id, layer.name, walk(children))
                }
                _ => format!("{}:{}", layer.id, layer.name),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    walk(&map.layers)
}

/// A tile layer holding an ASCII grid's cells, the grid's size; the map builder gives its id.
pub(crate) fn tile_layer(name: &str, grid: &str) -> bt_core::Layer {
    let cells = bt_test_data::fixtures::grid_layer(grid).unwrap();
    bt_core::Layer::new(bt_core::LayerId(0), name, bt_core::LayerKind::Tile(cells))
}

/// A group of layers; the map builder gives its id.
pub(crate) fn group(name: &str, layers: Vec<bt_core::Layer>) -> bt_core::Layer {
    bt_core::Layer::new(bt_core::LayerId(0), name, bt_core::LayerKind::Group(layers))
}

/// A rectangle object at `(x, y)` of `w × h` pixels; the map builder gives its id.

/// A rectangle object at (`x`, `y`) of `w` × `h` pixels, with id 0 for a map builder to number.
pub(crate) fn rect_object(x: f64, y: f64, w: f64, h: f64) -> bt_core::MapObject {
    bt_core::MapObject {
        pos: glam::DVec2::new(x, y),
        size: glam::DVec2::new(w, h),
        ..bt_core::MapObject::default()
    }
}

/// A 4×3 orthogonal map of 16-pixel tiles with every kind of layer, groups in groups:
/// `1:ground 2:walls[3:floor 4:props[5:crates]] 6:sky`. The crates are objects 1 and 2, the
/// first pointing at the second.
pub(crate) fn nested() -> bt_core::Map {
    let atlas = bt_test_data::fixtures::Atlas::builder()
        .build_embedded()
        .unwrap();
    let mut first = rect_object(0.0, 0.0, 16.0, 16.0);
    first.properties.insert(
        "next".into(),
        bt_core::Property::new(bt_core::Value::Object(bt_core::ObjectId(2))),
    );
    let crates = bt_core::Layer::new(
        bt_core::LayerId(0),
        "crates",
        bt_core::LayerKind::Object(bt_core::ObjectLayer {
            objects: std::sync::Arc::new(vec![first, rect_object(32.0, 16.0, 16.0, 16.0)]),
            ..bt_core::ObjectLayer::default()
        }),
    );
    let props = group("props", vec![crates]);
    let floor = tile_layer("floor", "1 1 1 1\n1 1 1 1\n1 1 1 1");
    bt_test_data::fixtures::MapBuilder::new(glam::UVec2::splat(16), glam::UVec2::new(4, 3))
        .tileset(atlas.tileset)
        .tile_layer("ground", "2 2 . .\n. . . .\n. . 3 3")
        .layer(group("walls", vec![floor, props]))
        .image_layer("sky", bt_core::ImageLayer::default())
        .build()
        .unwrap()
}

/// The ids of a list's objects, bottom first.
pub(crate) fn ids_of(objects: &bt_core::ObjectLayer) -> Vec<u32> {
    objects.objects.iter().map(|o| o.id.0).collect()
}

/// Asserts two points are the same within rounding.
#[track_caller]
pub(crate) fn assert_near(actual: glam::DVec2, expected: glam::DVec2) {
    assert!(
        (actual - expected).length() < 1e-9,
        "{actual} is not {expected}"
    );
}

/// An object of a random kind: a rectangle, ellipse, capsule, point, polygon, polyline, text or
/// tile object (one of the first tileset's first 16 tiles, maybe mirrored); maybe without a
/// size, maybe turned, maybe a template instance. Its id is 0.
pub(crate) fn random_object(rng: &mut bt_utils::rng::Rng) -> bt_core::MapObject {
    use bt_core::{FileRef, Flip, MapObject, Shape, TextData};
    use glam::DVec2;

    let pos = DVec2::new(rng.next_f64() * 200.0, rng.next_f64() * 200.0);
    let size = if rng.below(5) == Some(0) {
        DVec2::ZERO
    } else {
        DVec2::new(1.0 + rng.next_f64() * 60.0, 1.0 + rng.next_f64() * 60.0)
    };
    let rotation = match rng.below(4) {
        Some(0) => 0.0,
        Some(1) => 90.0 * rng.below(4).unwrap() as f64,
        _ => rng.next_f64() * 720.0 - 360.0,
    };
    let points = |rng: &mut bt_utils::rng::Rng, least: u64| {
        let count = least + rng.below(4).unwrap();
        (0..count)
            .map(|_| DVec2::new(rng.next_f64() * 80.0 - 40.0, rng.next_f64() * 80.0 - 40.0))
            .collect::<Vec<_>>()
    };
    let mut object = MapObject {
        pos,
        size,
        rotation,
        ..MapObject::default()
    };
    match rng.below(8).unwrap() {
        0 => object.shape = Shape::Ellipse,
        1 => object.shape = Shape::Capsule,
        2 => {
            object.shape = Shape::Point;
            object.size = DVec2::ZERO;
            object.rotation = 0.0;
        }
        3 => object.shape = Shape::Polygon(points(rng, 3)),
        4 => object.shape = Shape::Polyline(points(rng, 2)),
        5 => object.shape = Shape::Text(TextData::default()),
        6 => {
            let mut flip = Flip::empty();
            flip.set(Flip::H, rng.below(2) == Some(0));
            flip.set(Flip::V, rng.below(2) == Some(0));
            object.tile = Some(tile(rng.below(16).unwrap() as u32).with_flip(flip));
        }
        _ => {}
    }
    if rng.below(4) == Some(0) {
        object.template = Some(FileRef::new("thing.tx", "/maps/thing.tx"));
    }
    object
}
