//! Tiles land on the pixels Tiled puts them on, on every orientation, and overlapping tiles
//! stack in Tiled's order across chunk boundaries; Tiled's own example maps match an
//! independent CPU compositor.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{
    Cell, Flip, MapObject, ObjectId, Orientation, StaggerAxis, StaggerIndex, TileId, TilesetSlot,
};
use bt_render::Render;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use common::{assert_tile, cpu_reference, device, differing_share, keep, picture, pixel_of};
use glam::{DVec2, IVec2, UVec2};

fn slot0() -> TilesetSlot {
    TilesetSlot::new(0).unwrap()
}

#[test]
fn orthogonal_tiles_hang_from_their_cell_and_flip() {
    let atlas = Atlas::builder().tile_size(16, 32).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 3))
        .tileset(atlas.tileset.clone())
        .tile_layer("tall", ". . . . . .\n. 1 . 2d . .\n. . 3hv . . .")
        .build()
        .unwrap();
    let Some(picture) = picture(&map) else { return };
    // Anchored at the cell's bottom-left: (1, 1) spans y 0..32.
    assert_tile(
        &picture,
        pixel_of(&map, IVec2::new(16, 0)),
        UVec2::new(16, 32),
        &atlas,
        1,
        Flip::empty(),
        "(1, 1)",
    );
    assert_tile(
        &picture,
        pixel_of(&map, IVec2::new(32, 16)),
        UVec2::new(16, 32),
        &atlas,
        3,
        Flip::H | Flip::V,
        "(2, 2)",
    );
    // Transposed: 32 wide, 16 high, still on the cell's bottom-left.
    assert_tile(
        &picture,
        pixel_of(&map, IVec2::new(48, 16)),
        UVec2::new(32, 16),
        &atlas,
        2,
        Flip::D,
        "(3, 1)",
    );
}

#[test]
fn isometric_tiles_sit_on_the_diamond_and_draw_by_diagonal() {
    let atlas = Atlas::builder().tile_size(16, 16).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 8), UVec2::new(3, 2))
        .orientation(Orientation::Isometric)
        .tileset(atlas.tileset.clone())
        .tile_layer("blocks", "0 . .\n. 5h 6")
        .build()
        .unwrap();
    let Some(picture) = picture(&map) else { return };
    assert_eq!((picture.width, picture.height), (5 * 8, 5 * 4));
    // Tile (x, y): diamond top at ((x − y)·8 + 16, (x + y)·4); the image's bottom-left 8 left
    // of it and 8 down; it is 16 high.
    let top_left = |x: i32, y: i32| IVec2::new((x - y) * 8 + 16 - 8, (x + y) * 4 + 8 - 16);
    // (2, 1) is drawn last (diagonal 3), over (1, 1).
    assert_tile(
        &picture,
        pixel_of(&map, top_left(2, 1)),
        UVec2::new(16, 16),
        &atlas,
        6,
        Flip::empty(),
        "(2, 1)",
    );
    // (1, 1) shows where (2, 1) does not cover it: its left half above y = 4.
    let at = pixel_of(&map, top_left(1, 1));
    assert_eq!(
        picture.pixel(at.x as u32 + 15, at.y as u32),
        Some(bt_test_data::fixtures::MARKER),
        "mirrored marker"
    );
}

#[test]
fn staggered_rows_shift_and_d_transposes() {
    let atlas = Atlas::builder().tile_size(16, 8).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 8), UVec2::new(3, 3))
        .orientation(Orientation::Staggered)
        .stagger(StaggerAxis::Y, StaggerIndex::Odd)
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 . .\n. 1 .\n. . .")
        .build()
        .unwrap();
    let Some(picture) = picture(&map) else { return };
    assert_eq!((picture.width, picture.height), (56, 16));
    assert_tile(
        &picture,
        IVec2::new(0, 0),
        UVec2::new(16, 8),
        &atlas,
        0,
        Flip::empty(),
        "(0, 0)",
    );
    // Row 1 is shifted right by half a tile.
    assert_tile(
        &picture,
        IVec2::new(24, 4),
        UVec2::new(16, 8),
        &atlas,
        1,
        Flip::empty(),
        "(1, 1)",
    );

    let square = Atlas::builder().tile_size(8, 8).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(8, 8), UVec2::new(2, 2))
        .orientation(Orientation::Staggered)
        .stagger(StaggerAxis::Y, StaggerIndex::Even)
        .tileset(square.tileset.clone())
        .tile_layer("ground", ". .\n. 2d")
        .build()
        .unwrap();
    let Some(picture) = common::picture(&map) else {
        return;
    };
    // Even index: row 1 is not shifted; D transposes (no hexagonal turn on staggered maps).
    assert_tile(
        &picture,
        IVec2::new(8, 4),
        UVec2::new(8, 8),
        &square,
        2,
        Flip::D,
        "(1, 1)",
    );
}

fn hex_map(atlas: &Atlas, grid: &str) -> bt_core::Map {
    MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 2))
        .orientation(Orientation::Hexagonal)
        .hex_side_length(8)
        .stagger(StaggerAxis::X, StaggerIndex::Odd)
        .tileset(atlas.tileset.clone())
        .tile_layer("hexes", grid)
        .build()
        .unwrap()
}

#[test]
fn hexagonal_cells_turn_by_60_and_120_degrees() {
    let atlas = Atlas::builder()
        .tile_size(16, 16)
        .marker(8, 8)
        .build_embedded()
        .unwrap();
    // 0°, 60°, 120°, 180°, 240° (H + V + 60°), H + 60°.
    let map = hex_map(&atlas, "0 0d 0x 0dx 0hvd 0hd\n. . . . . .");
    let flips = [
        Flip::empty(),
        Flip::D,
        Flip::HEX120,
        Flip::D | Flip::HEX120,
        Flip::H | Flip::V | Flip::D,
        Flip::H | Flip::D,
    ];
    let geometry = map.geometry();
    let scale = 4.0;
    let Some(device) = device() else { return };
    let picture = Render::of(Arc::new(map.clone()))
        .scale(scale)
        .run(device)
        .unwrap();
    keep(&picture, "hexagonal-turns");
    let bounds = map.bounding_rect();
    for (x, flip) in flips.iter().enumerate() {
        let anchor = geometry.cell_anchor(IVec2::new(x as i32, 0));
        let center = anchor + DVec2::new(8.0, -8.0);
        // The marker's centre (4, 4) from the image's centre, mirrored, then turned clockwise.
        let mut m = DVec2::new(-4.0, -4.0);
        if flip.contains(Flip::H) {
            m.x = -m.x;
        }
        if flip.contains(Flip::V) {
            m.y = -m.y;
        }
        let degrees = 60.0 * f64::from(u8::from(flip.contains(Flip::D)))
            + 120.0 * f64::from(u8::from(flip.contains(Flip::HEX120)));
        let (s, c) = degrees.to_radians().sin_cos();
        let turned = DVec2::new(m.x * c - m.y * s, m.x * s + m.y * c);
        let at = |p: DVec2| {
            let d = (p - bounds.min().as_dvec2()) * scale;
            picture
                .pixel(d.x.floor() as u32, d.y.floor() as u32)
                .unwrap()
        };
        assert_eq!(
            at(center + turned),
            bt_test_data::fixtures::MARKER,
            "column {x} ({degrees}°)"
        );
        assert_eq!(
            at(center - turned),
            Atlas::tile_color(TileId(0)),
            "column {x}: the other side"
        );
    }
}

#[test]
fn a_hexagonal_half_turn_is_pixel_exact() {
    let atlas = Atlas::builder().tile_size(16, 16).build_embedded().unwrap();
    let map = hex_map(&atlas, "1dx . 1hv . . .\n. . . . . .");
    let Some(picture) = picture(&map) else { return };
    let geometry = map.geometry();
    for x in [0, 2] {
        let anchor = geometry.cell_anchor(IVec2::new(x, 0)).as_ivec2();
        let top_left = pixel_of(&map, anchor - IVec2::new(0, 16));
        assert_tile(
            &picture,
            top_left,
            UVec2::new(16, 16),
            &atlas,
            1,
            Flip::H | Flip::V,
            "a half turn",
        );
    }
}

#[test]
fn oblique_tiles_stand_upright_at_sheared_places() {
    let atlas = Atlas::builder().tile_size(8, 8).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(8, 8), UVec2::new(3, 3))
        .orientation(Orientation::Oblique)
        .skew(IVec2::new(4, -2))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", ". . .\n. 4 .\n7 . .")
        .build()
        .unwrap();
    let Some(picture) = picture(&map) else { return };
    let geometry = map.geometry();
    for (cell, tile) in [(IVec2::new(1, 1), 4), (IVec2::new(0, 2), 7)] {
        let anchor = geometry.cell_anchor(cell).as_ivec2();
        assert_tile(
            &picture,
            pixel_of(&map, anchor - IVec2::new(0, 8)),
            UVec2::new(8, 8),
            &atlas,
            tile,
            Flip::empty(),
            "oblique",
        );
    }
}

#[test]
fn isometric_tile_objects_stand_bottom_centred_on_their_projection() {
    let atlas = Atlas::builder().tile_size(8, 8).build_embedded().unwrap();
    let object = MapObject {
        id: ObjectId(1),
        pos: DVec2::new(16.0, 8.0),
        size: DVec2::new(8.0, 8.0),
        tile: Some(Cell::new(slot0(), TileId(3))),
        ..MapObject::default()
    };
    let map = MapBuilder::new(UVec2::new(16, 8), UVec2::new(4, 4))
        .orientation(Orientation::Isometric)
        .tileset(atlas.tileset.clone())
        .object_layer("things", vec![object])
        .build()
        .unwrap();
    let Some(picture) = picture(&map) else { return };
    // Pixel (16, 8) is tile (2, 1): screen ((2 − 1)·8 + 32, (2 + 1)·4) = (40, 12).
    assert_tile(
        &picture,
        pixel_of(&map, IVec2::new(36, 4)),
        UVec2::new(8, 8),
        &atlas,
        3,
        Flip::empty(),
        "tile object",
    );
}

/// Tiles wider and taller than the grid on a map of several chunks.
fn overlapping(orientation: Orientation) -> (Atlas, bt_core::Map) {
    let atlas = Atlas::builder()
        .tile_size(24, 20)
        .grid(4, 1)
        .build_embedded()
        .unwrap();
    let mut rows = Vec::new();
    for y in 0..40 {
        let row: Vec<String> = (0..40)
            .map(|x| {
                let n: u32 = (x * 7 + y * 13 + x * y) as u32 % 6;
                if n < 4 { n.to_string() } else { ".".to_owned() }
            })
            .collect();
        rows.push(row.join(" "));
    }
    let mut builder =
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(40, 40)).orientation(orientation);
    if orientation == Orientation::Hexagonal {
        builder = builder.hex_side_length(8);
    }
    let map = builder
        .tileset(atlas.tileset.clone())
        .tile_layer("overlapping", &rows.join("\n"))
        .build()
        .unwrap();
    (atlas, map)
}

#[test]
fn overlapping_tiles_stack_in_tileds_order_across_chunks() {
    for orientation in [
        Orientation::Orthogonal,
        Orientation::Isometric,
        Orientation::Staggered,
        Orientation::Hexagonal,
        Orientation::Oblique,
    ] {
        let (_, map) = overlapping(orientation);
        let Some(picture) = picture(&map) else { return };
        let expected = cpu_reference(&map);
        keep(&picture, &format!("overlapping-{orientation:?}"));
        let share = differing_share(&picture, &expected, 2);
        assert!(
            share == 0.0,
            "{orientation:?}: {:.3} % of pixels differ",
            share * 100.0
        );
    }
}

#[test]
fn tileds_example_maps_match_the_cpu_compositor() {
    for name in [
        "desert.tmx",
        "isometric_grass_and_water.tmx",
        "isometric_staggered_grass_and_water.tmx",
        "hexagonal-mini.tmx",
        "perspective_walls.tmx",
        "sewers.tmx",
    ] {
        let Some(path) = bt_test_data::test_data(&format!("reference/tiled/examples/{name}"))
        else {
            return;
        };
        let map = bt_tmx::Loader::default().load_map(&path).unwrap().value;
        let Some(picture) = picture(&map) else { return };
        keep(&picture, name);
        let expected = cpu_reference(&map);
        keep(&expected, &format!("{name}-reference"));
        let share = differing_share(&picture, &expected, 2);
        assert!(
            share == 0.0,
            "{name}: {:.3} % of pixels differ",
            share * 100.0
        );
    }
}
