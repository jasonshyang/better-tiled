//! Grids (docs/design/12-godot-export.md): the TileSet's shape lines, the cell box, the shift, and flipped tall
//! tiles, checked against bt-core's geometry and a port of Godot's draw rule.

use Orientation::{Hexagonal, Isometric, Orthogonal, Staggered};
use StaggerAxis::{X, Y};
use StaggerIndex::{Even, Odd};
use bt_core::{Map, MapGeometry, Orientation, StaggerAxis, StaggerIndex};
use glam::{DVec2, IVec2};

use super::super::grid::{FLIP_H, FLIP_V, Grid, TRANSPOSE, tiled_centre};
use super::super::text;
use super::support::*;
use super::*;

/// A map of `tile` px cells, `rows` tiles high.
fn grid_map(
    orientation: Orientation,
    tile: (u32, u32),
    side: i32,
    axis: StaggerAxis,
    index: StaggerIndex,
    rows: u32,
) -> Map {
    Map {
        orientation,
        tile_width: tile.0,
        tile_height: tile.1,
        width: 10,
        height: rows,
        hex_side_length: side,
        stagger_axis: axis,
        stagger_index: index,
        ..Map::default()
    }
}

fn grid(
    orientation: Orientation,
    tile: (u32, u32),
    side: i32,
    axis: StaggerAxis,
    index: StaggerIndex,
) -> Grid {
    Grid::of(&grid_map(orientation, tile, side, axis, index, 10)).unwrap()
}

fn lines(grid: &Grid) -> Vec<String> {
    grid.tileset_lines()
        .into_iter()
        .map(|(k, v)| format!("{k} = {v}"))
        .chain([format!("tile_size = {}", text::vector2i(grid.tile_size()))])
        .collect()
}

/// A grid and the TileSet lines it gets.
type GridCase = (
    Orientation,
    (u32, u32),
    i32,
    StaggerAxis,
    StaggerIndex,
    &'static [&'static str],
);

#[test]
fn tilesets_get_the_shape_layout_and_axis_of_each_grid() {
    let cases: [GridCase; 12] = [
        (
            Orthogonal,
            (32, 16),
            0,
            Y,
            Odd,
            &["tile_size = Vector2i(32, 16)"],
        ),
        (
            Isometric,
            (64, 32),
            0,
            Y,
            Odd,
            &[
                "tile_shape = 1",
                "tile_layout = 5",
                "tile_size = Vector2i(64, 32)",
            ],
        ),
        (
            Isometric,
            (63, 31),
            0,
            Y,
            Odd,
            &[
                "tile_shape = 1",
                "tile_layout = 5",
                "tile_size = Vector2i(63, 31)",
            ],
        ),
        (
            Staggered,
            (64, 32),
            0,
            Y,
            Odd,
            &["tile_shape = 1", "tile_size = Vector2i(64, 32)"],
        ),
        (
            Staggered,
            (64, 32),
            0,
            Y,
            Even,
            &[
                "tile_shape = 1",
                "tile_layout = 1",
                "tile_size = Vector2i(64, 32)",
            ],
        ),
        (
            Staggered,
            (64, 32),
            0,
            X,
            Odd,
            &[
                "tile_shape = 1",
                "tile_offset_axis = 1",
                "tile_size = Vector2i(64, 32)",
            ],
        ),
        (
            Staggered,
            (64, 32),
            0,
            X,
            Even,
            &[
                "tile_shape = 1",
                "tile_layout = 1",
                "tile_offset_axis = 1",
                "tile_size = Vector2i(64, 32)",
            ],
        ),
        // Tiled rounds halves down: the grid is a pixel smaller than the map's tile size.
        (
            Staggered,
            (63, 31),
            0,
            Y,
            Odd,
            &["tile_shape = 1", "tile_size = Vector2i(62, 30)"],
        ),
        // Exact hexagons: the straight side is half the height (or width).
        (
            Hexagonal,
            (14, 12),
            6,
            Y,
            Odd,
            &["tile_shape = 3", "tile_size = Vector2i(14, 12)"],
        ),
        (
            Hexagonal,
            (60, 60),
            30,
            X,
            Odd,
            &[
                "tile_shape = 3",
                "tile_offset_axis = 1",
                "tile_size = Vector2i(60, 60)",
            ],
        ),
        (
            Hexagonal,
            (15, 13),
            6,
            Y,
            Even,
            &[
                "tile_shape = 3",
                "tile_layout = 1",
                "tile_size = Vector2i(14, 12)",
            ],
        ),
        // Other sides: one row (or column) as a half-offset square.
        (
            Hexagonal,
            (14, 14),
            7,
            Y,
            Odd,
            &["tile_shape = 2", "tile_size = Vector2i(14, 10)"],
        ),
    ];
    for (orientation, tile, side, axis, index, expected) in cases {
        assert_eq!(
            lines(&grid(orientation, tile, side, axis, index)),
            expected,
            "{orientation:?} {tile:?} side {side} {axis:?} {index:?}"
        );
    }
    assert_eq!(
        lines(&grid(Hexagonal, (14, 14), 7, X, Even)),
        [
            "tile_shape = 2",
            "tile_layout = 1",
            "tile_offset_axis = 1",
            "tile_size = Vector2i(10, 14)"
        ]
    );
    // Side 0 and 8 on 32 px drift row by row as hexagons.
    assert_eq!(
        lines(&grid(Hexagonal, (32, 32), 0, Y, Odd)),
        ["tile_shape = 2", "tile_size = Vector2i(32, 16)"]
    );
    assert_eq!(
        lines(&grid(Hexagonal, (32, 32), 8, Y, Odd)),
        ["tile_shape = 2", "tile_size = Vector2i(32, 20)"]
    );
}

#[test]
fn cell_boxes_are_tileds_image_anchors() {
    let cases = [
        (Orthogonal, (32, 16), 0, Y, (32, 16)),
        (Isometric, (64, 32), 0, Y, (64, 32)),
        // Tiled anchors isometric images half the width, rounded down, left of the centre.
        (Isometric, (63, 31), 0, Y, (62, 31)),
        (Staggered, (63, 31), 0, X, (62, 30)),
        (Hexagonal, (14, 12), 6, Y, (14, 12)),
        (Hexagonal, (60, 60), 30, X, (60, 60)),
        // The fallback's box is bt-core's hexagon, 2⌊(th − s)/2⌋ + s high, not 2⌊th/2⌋.
        (Hexagonal, (14, 14), 7, Y, (14, 13)),
        (Hexagonal, (14, 14), 7, X, (13, 14)),
    ];
    for (orientation, tile, side, axis, expected) in cases {
        assert_eq!(
            grid(orientation, tile, side, axis, Odd).cell_box(),
            IVec2::from(expected),
            "{orientation:?} {tile:?} side {side} {axis:?}"
        );
    }
    // On every grid, the box is centred on the cell with its bottom-left at Tiled's anchor.
    for_grids(|map, grid| {
        let geometry = MapGeometry::from_map(map);
        let b = grid.cell_box().as_dvec2();
        for cell in around_origin() {
            let anchor = geometry.cell_anchor(cell);
            assert_eq!(
                anchor - tiled_centre(&geometry, cell),
                DVec2::new(-b.x, b.y) / 2.0,
                "{} cell {cell}",
                describe(map)
            );
        }
    });
}

#[test]
fn the_shift_is_tileds_centre_minus_godots() {
    let shift = |orientation, tile, side, axis, rows| {
        Grid::of(&grid_map(orientation, tile, side, axis, Odd, rows))
            .unwrap()
            .shift()
    };
    assert_eq!(
        shift(Isometric, (64, 32), 0, Y, 25),
        DVec2::new(768.0, 0.0),
        "the 25-row example"
    );
    assert_eq!(shift(Isometric, (64, 32), 0, Y, 10), DVec2::new(288.0, 0.0));
    assert_eq!(
        shift(Isometric, (63, 31), 0, Y, 7),
        DVec2::new(188.5, 0.0),
        "half a pixel for odd widths"
    );
    let mut infinite = grid_map(Isometric, (64, 32), 0, Y, Odd, 50);
    infinite.infinite = true;
    assert_eq!(
        Grid::of(&infinite).unwrap().shift(),
        DVec2::new(1568.0, 0.0),
        "infinite maps keep their height's origin"
    );
    for (orientation, tile, side, axis) in [
        (Orthogonal, (32, 32), 0, Y),
        (Staggered, (63, 31), 0, X),
        (Hexagonal, (14, 12), 6, Y),
        (Hexagonal, (60, 60), 30, X),
    ] {
        assert_eq!(
            shift(orientation, tile, side, axis, 10),
            DVec2::ZERO,
            "{orientation:?}"
        );
    }
    // The fallback: half a side offset, across the rows (or columns).
    assert_eq!(shift(Hexagonal, (14, 14), 7, Y, 10), DVec2::new(0.0, 1.5));
    assert_eq!(shift(Hexagonal, (14, 14), 7, X, 10), DVec2::new(1.5, 0.0));

    // Every cell's centre, negative ones included, is the shift from Godot's.
    for_grids(|map, grid| {
        let geometry = MapGeometry::from_map(map);
        for cell in around_origin() {
            assert_eq!(
                tiled_centre(&geometry, cell) - grid.godot_centre(cell),
                grid.shift(),
                "{} cell {cell}",
                describe(map)
            );
        }
    });
}

#[test]
fn texture_origins_put_tall_and_flipped_tiles_where_tiled_draws_them() {
    // The worked examples of §4.3.
    let iso = grid(Isometric, (64, 32), 0, Y, Odd);
    assert_eq!(
        iso.texture_origin(IVec2::new(64, 64), IVec2::new(0, 16), 0),
        (IVec2::ZERO, true)
    );
    let mini = grid(Hexagonal, (14, 12), 6, Y, Odd);
    assert_eq!(
        mini.texture_origin(IVec2::new(18, 18), IVec2::new(0, 1), 0),
        (IVec2::new(-2, 2), true)
    );
    // Flipped, Godot would mirror (-2, 2) and draw it 4 px off.
    assert_eq!(
        mini.texture_origin(IVec2::new(18, 18), IVec2::new(0, 1), FLIP_H),
        (IVec2::new(2, 2), true)
    );
    assert_eq!(
        mini.texture_origin(IVec2::new(18, 18), IVec2::new(0, 1), FLIP_V),
        (IVec2::new(-2, -2), true)
    );
    let square = grid(Orthogonal, (32, 32), 0, Y, Odd);
    let big = IVec2::new(64, 64);
    assert_eq!(
        square.texture_origin(big, IVec2::ZERO, 0).0,
        IVec2::new(-16, 16)
    );
    assert_eq!(
        square.texture_origin(big, IVec2::ZERO, TRANSPOSE).0,
        IVec2::new(16, -16)
    );
    // A tile wider than tall turned sideways moves even with a zero origin.
    assert_eq!(
        square.texture_origin(IVec2::new(32, 16), IVec2::ZERO, 0).0,
        IVec2::new(0, -8)
    );
    assert_eq!(
        square
            .texture_origin(IVec2::new(32, 16), IVec2::ZERO, TRANSPOSE)
            .0,
        IVec2::new(0, 8)
    );
    assert!(
        !square.texture_origin(IVec2::new(33, 32), IVec2::ZERO, 0).1,
        "half a pixel off"
    );

    // On every grid, size, offset and flip, Godot's draw rule puts the image where Tiled does.
    let sizes = [
        (14, 12),
        (18, 18),
        (64, 64),
        (32, 16),
        (16, 48),
        (63, 31),
        (7, 9),
    ];
    let offsets = [(0, 0), (0, 1), (-3, 2), (5, -7)];
    for_grids(|map, grid| {
        let geometry = MapGeometry::from_map(map);
        for (size, offset) in sizes.iter().flat_map(|s| {
            offsets
                .iter()
                .map(move |o| (IVec2::from(*s), IVec2::from(*o)))
        }) {
            for flags in (0..8).map(|f| f * FLIP_H) {
                let (origin, exact) = grid.texture_origin(size, offset, flags);
                let godot = godot_top_left(size, origin, flags);
                let cell = IVec2::new(3, -2);
                let drawn = if flags & TRANSPOSE != 0 {
                    IVec2::new(size.y, size.x)
                } else {
                    size
                };
                let tiled = geometry.cell_image_rect(cell, drawn.as_dvec2()).min
                    + offset.as_dvec2()
                    - tiled_centre(&geometry, cell);
                let error = (godot - tiled).abs().max_element();
                assert!(
                    if exact { error == 0.0 } else { error == 0.5 },
                    "{} size {size} offset {offset} flags {flags}: Godot {godot}, Tiled {tiled}",
                    describe(map)
                );
            }
        }
    });
}

/// Where Godot draws a tile image's top-left, from its cell's centre: TileMapLayer's rule as note
/// 16's independent check ported it (transpose, then mirror about the centre).
fn godot_top_left(size: IVec2, origin: IVec2, flags: u16) -> DVec2 {
    let mut s = size.as_dvec2();
    let mut p = -s / 2.0 - origin.as_dvec2();
    if flags & TRANSPOSE != 0 {
        (p, s) = (DVec2::new(p.y, p.x), DVec2::new(s.y, s.x));
    }
    if flags & FLIP_H != 0 {
        p.x = -(p.x + s.x);
    }
    if flags & FLIP_V != 0 {
        p.y = -(p.y + s.y);
    }
    p
}

/// Cells around the origin, negative ones included.
fn around_origin() -> impl Iterator<Item = IVec2> {
    (-3..=3).flat_map(|y| (-3..=3).map(move |x| IVec2::new(x, y)))
}

/// Calls `f` on a sweep of grids: every orientation Godot gets, sizes odd and even, both stagger
/// axes and indices, and every hex side length up to the tile size.
fn for_grids(mut f: impl FnMut(&Map, &Grid)) {
    let sizes = [
        (1, 1),
        (2, 3),
        (14, 12),
        (14, 14),
        (15, 13),
        (32, 32),
        (60, 60),
        (63, 31),
        (17, 40),
    ];
    for (tw, th) in sizes {
        for orientation in [Orthogonal, Isometric, Staggered, Hexagonal] {
            for axis in [X, Y] {
                for index in [Odd, Even] {
                    let sides = if orientation == Hexagonal {
                        0..=(if axis == X { tw } else { th }) as i32
                    } else {
                        0..=0
                    };
                    for side in sides {
                        let map = grid_map(orientation, (tw, th), side, axis, index, 7);
                        // Grids too small for Godot are refused (tested below).
                        if let Ok(grid) = Grid::of(&map) {
                            f(&map, &grid);
                        }
                    }
                }
            }
        }
    }
}

fn describe(map: &Map) -> String {
    format!(
        "{:?} {}x{} side {} {:?} {:?}",
        map.orientation,
        map.tile_width,
        map.tile_height,
        map.hex_side_length,
        map.stagger_axis,
        map.stagger_index
    )
}

#[test]
fn oblique_maps_and_tiny_cells_are_refused() {
    let oblique = Grid::of(&grid_map(Orientation::Oblique, (32, 32), 0, Y, Odd, 10))
        .err()
        .unwrap();
    assert!(oblique.contains("oblique"), "{oblique}");
    let tiny = Grid::of(&grid_map(Staggered, (1, 1), 0, Y, Odd, 10))
        .err()
        .unwrap();
    assert!(tiny.contains("1×1 px"), "{tiny}");

    // A map with objects goes on Godot's isometric grid too (objects.rs places them).
    let dir = project("grid-objects");
    let level = dir.join("maps/level.tmx");
    let mut map = bt_tmx::Loader::builder()
        .build()
        .load_map(&level)
        .unwrap()
        .value;
    map.orientation = Isometric;
    bt_tmx::WriteRequest::for_map(&map, &dir.join("maps/iso.tmx"))
        .write()
        .unwrap();
    let report = export(
        &dir.join("maps/iso.tmx"),
        &mut loader(&dir),
        &dir.join("iso.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("iso.tscn")).unwrap();
    assert!(
        tscn.contains("tile_shape = 1\ntile_layout = 5\ntile_size = Vector2i(32, 32)\n"),
        "{tscn}"
    );
    let shift = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/grid-shift")
        .expect("the report states the shift");
    assert!(
        shift.message.contains("plus (32, 0) px") && shift.severity == report::Severity::Info,
        "{}",
        shift.message
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_layers_move_by_the_shift_and_tile_layers_keep_their_offsets() {
    let mut map = grid_map(Isometric, (64, 32), 0, Y, Odd, 10);
    let image = bt_core::Layer {
        offset: DVec2::new(5.0, 7.0),
        ..bt_core::Layer::new(
            bt_core::LayerId(1),
            "bg",
            LayerKind::Image(Default::default()),
        )
    };
    map.layers.push(image);
    let grid = Grid::of(&map).unwrap();
    assert_eq!(
        grid.layer_position(&map.layers[0].kind, DVec2::new(5.0, 7.0)),
        DVec2::new(-283.0, 7.0)
    );
    let tiles = LayerKind::Tile(bt_core::TileLayer::finite(glam::UVec2::new(10, 10)));
    assert_eq!(
        grid.layer_position(&tiles, DVec2::new(5.0, 7.0)),
        DVec2::new(5.0, 7.0)
    );
    let hex = Grid::of(&grid_map(Hexagonal, (14, 14), 7, Y, Odd, 10)).unwrap();
    assert_eq!(
        hex.layer_position(&map.layers[0].kind, DVec2::ZERO),
        DVec2::new(0.0, -1.5)
    );
    let codes: Vec<&str> = hex.notes().iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["godot/hex-fallback", "godot/grid-shift"]);
    assert!(
        grid_notes(Hexagonal, (14, 12), 6).is_empty()
            && grid_notes(Orthogonal, (32, 32), 0).is_empty()
            && grid_notes(Staggered, (64, 32), 0).is_empty()
    );
}

fn grid_notes(orientation: Orientation, tile: (u32, u32), side: i32) -> Vec<report::Note> {
    grid(orientation, tile, side, Y, Odd).notes()
}

#[test]
fn flipped_tall_tiles_get_alternatives_with_their_own_origin() {
    let dir = project("grid-flips");
    // The 64×64 tile (gid 9, 32×32 cells) painted flipped: H, V, HV, diagonally, and H again.
    let row = "2147483657,1073741833,3221225481,536870921,2147483657,0,";
    let features = FEATURES.replacen(
        "5,0,0,9,0,0,\n0,0,0,0,0,0,\n",
        &format!("5,0,0,9,0,0,\n{row}\n"),
        1,
    );
    assert_ne!(features, FEATURES);
    std::fs::write(dir.join("maps/flips.tmx"), features).unwrap();
    let target = dir.join("levels/flips.tscn");
    let report = export(
        &dir.join("maps/flips.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    // Notes only: bg's parallax origin, and the mirrored big tiles' collision cell by cell.
    assert_eq!(
        report
            .notes
            .iter()
            .map(|n| n.code.as_str())
            .collect::<Vec<_>>(),
        ["godot/left-out", "godot/physics-quadrant-size"],
        "{:?}",
        report.warnings()
    );
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);
    // The atlas source of the 64×64 tiles (sources are numbered by tileset).
    let (big_header, _) = s
        .iter()
        .find(|(h, body)| {
            h.contains("TileSetAtlasSource_")
                && body
                    .iter()
                    .any(|l| l == "texture_region_size = Vector2i(64, 64)")
        })
        .unwrap();
    let n: i32 = big_header
        .split("TileSetAtlasSource_")
        .nth(1)
        .unwrap()
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap();
    let data: Vec<[i32; 6]> = cells(value(node(&s, "cells", "."), "tile_map_data"))
        .into_iter()
        .filter(|c| c[1] == 2)
        .collect();
    assert_eq!(
        data,
        [
            [0, 2, n, 0, 0, 1 | 4096],
            [1, 2, n, 0, 0, 2 | 8192],
            [2, 2, n, 0, 0, 3 | 4096 | 8192],
            [3, 2, n, 0, 0, 4 | 16384],
            [4, 2, n, 0, 0, 1 | 4096]
        ]
    );
    let big = s
        .iter()
        .find(|(h, _)| h.contains(&format!("id=\"TileSetAtlasSource_{n}\"")))
        .unwrap();
    let expected = [
        "0:0/0 = 0",
        "0:0/0/texture_origin = Vector2i(-16, 16)",
        "0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-16, -16, 48, -16, 48, 16, -16, 16)",
        // Mirrored ahead, with the collision moved along.
        "0:0/1 = 1",
        "0:0/1/texture_origin = Vector2i(16, 16)",
        "0:0/1/physics_layer_0/polygon_0/points = PackedVector2Array(-48, -16, 16, -16, 16, 16, -48, 16)",
        "0:0/2 = 2",
        "0:0/2/texture_origin = Vector2i(-16, -16)",
        "0:0/2/physics_layer_0/polygon_0/points = PackedVector2Array(-16, 16, 48, 16, 48, 48, -16, 48)",
        "0:0/3 = 3",
        "0:0/3/texture_origin = Vector2i(16, -16)",
        "0:0/3/physics_layer_0/polygon_0/points = PackedVector2Array(-48, 16, 16, 16, 16, 48, -48, 48)",
        "0:0/4 = 4",
        "0:0/4/texture_origin = Vector2i(16, -16)",
        "0:0/4/physics_layer_0/polygon_0/points = PackedVector2Array(-48, 16, 16, 16, 16, 48, -48, 48)",
    ];
    let first = big.1.iter().position(|l| l == "0:0/0 = 0").unwrap();
    assert_eq!(big.1[first..], expected, "{tscn}");
    // Grid-sized tiles keep flipping on alternative 0.
    assert!(cells(value(node(&s, "cells", "."), "tile_map_data")).contains(&[1, 0, 0, 1, 0, 4096]));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn hexagonal_half_turns_are_mirrors_and_other_turns_are_dropped_with_a_warning() {
    let dir = project("grid-hex-turns");
    // Grid-sized tiles turned 60° (D), 120° (the hex bit), 180° (both), 180° mirrored, mirrored.
    let gids = [
        0x2000_0001u32,
        0x1000_0001,
        0x3000_0001,
        0xb000_0001,
        0x8000_0001,
    ];
    let row: Vec<String> = gids.iter().map(u32::to_string).collect();
    let tmx = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="hexagonal" renderorder="right-down" width="5" height="1" tilewidth="14" tileheight="12" infinite="0" hexsidelength="6" staggeraxis="y" staggerindex="odd" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" name="hex" tilewidth="14" tileheight="12" tilecount="20" columns="4">
  <image source="tiles.png" width="64" height="64"/>
 </tileset>
 <layer id="1" name="ground" width="5" height="1">
  <data encoding="csv">
{}
</data>
 </layer>
</map>
"#,
        row.join(",")
    );
    std::fs::write(dir.join("maps/turns.tmx"), tmx).unwrap();
    let target = dir.join("turns.tscn");
    let report = export(
        &dir.join("maps/turns.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let codes: Vec<&str> = report.notes.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(
        codes,
        ["godot/hex-rotation"],
        "one warning for every turned tile"
    );
    let tscn = std::fs::read_to_string(&target).unwrap();
    let alternatives: Vec<i32> = cells(value(
        node(&sections(&tscn), "ground", "."),
        "tile_map_data",
    ))
    .iter()
    .map(|c| c[5])
    .collect();
    assert_eq!(
        alternatives,
        [0, 0, (FLIP_H | FLIP_V) as i32, FLIP_V as i32, FLIP_H as i32]
    );
    std::fs::remove_dir_all(&dir).ok();
}
