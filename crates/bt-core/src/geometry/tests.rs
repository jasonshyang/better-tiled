use std::collections::HashSet;

use bt_utils::rng::Rng as Generator;
use glam::UVec2;

use super::*;
use crate::cell::Cell;
use crate::ids::{LayerId, TileId, TilesetSlot};
use crate::layer::{Layer, LayerKind};
use crate::tile_layer::TileLayer;

fn geometry(orientation: Orientation, tile: (i32, i32), map: (i32, i32)) -> MapGeometry {
    MapGeometry::new(
        orientation,
        IVec2::new(tile.0, tile.1),
        IVec2::new(map.0, map.1),
    )
}

fn hexagonal(tile: (i32, i32), side: i32, stagger_x: bool, even: bool) -> MapGeometry {
    MapGeometry {
        hex_side_length: side,
        stagger_axis: if stagger_x {
            StaggerAxis::X
        } else {
            StaggerAxis::Y
        },
        stagger_index: if even {
            StaggerIndex::Even
        } else {
            StaggerIndex::Odd
        },
        ..geometry(Orientation::Hexagonal, tile, (20, 20))
    }
}

fn staggered(tile: (i32, i32), stagger_x: bool, even: bool) -> MapGeometry {
    MapGeometry {
        orientation: Orientation::Staggered,
        ..hexagonal(tile, 0, stagger_x, even)
    }
}

fn oblique(tile: (i32, i32), skew: (i32, i32)) -> MapGeometry {
    MapGeometry {
        skew: IVec2::new(skew.0, skew.1),
        ..geometry(Orientation::Oblique, tile, (12, 9))
    }
}

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn t(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

fn r(x: i32, y: i32, w: i32, h: i32) -> IRect {
    IRect { x, y, w, h }
}

/// The seeded generator, with the ranges these tests draw.
struct Rng(Generator);

impl Rng {
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let span = u64::try_from(i64::from(hi) - i64::from(lo) + 1).unwrap();
        lo + i32::try_from(self.0.below(span).unwrap()).unwrap()
    }

    fn unit(&mut self) -> f64 {
        self.0.next_f64()
    }
}

fn rng(seed: u64) -> Rng {
    Rng(Generator::new(seed))
}

/// Every orientation with several parameter sets, including odd sizes.
fn all_geometries() -> Vec<MapGeometry> {
    let mut list = vec![
        geometry(Orientation::Orthogonal, (32, 32), (10, 8)),
        geometry(Orientation::Orthogonal, (17, 9), (10, 8)),
        MapGeometry {
            render_order: RenderOrder::LeftUp,
            ..geometry(Orientation::Orthogonal, (16, 16), (10, 8))
        },
        MapGeometry {
            render_order: RenderOrder::RightUp,
            ..geometry(Orientation::Orthogonal, (16, 24), (10, 8))
        },
        geometry(Orientation::Isometric, (64, 32), (25, 25)),
        geometry(Orientation::Isometric, (65, 33), (5, 7)),
        geometry(Orientation::Isometric, (32, 32), (6, 3)),
        oblique((32, 32), (12, 0)),
        oblique((24, 16), (-8, 5)),
        MapGeometry {
            render_order: RenderOrder::LeftDown,
            ..oblique((20, 20), (7, -3))
        },
    ];
    for (stagger_x, even) in [(false, false), (false, true), (true, false), (true, true)] {
        list.push(hexagonal((70, 60), 20, stagger_x, even));
        list.push(hexagonal((14, 12), 6, stagger_x, even));
        list.push(hexagonal((15, 13), 5, stagger_x, even));
        list.push(staggered((64, 32), stagger_x, even));
        list.push(staggered((33, 17), stagger_x, even));
    }
    list
}

// --- Golden vectors ----------------------------------------------------------------------------

/// `tests/staggeredrenderer/test_staggeredrenderer.cpp` in Tiled.
#[test]
fn staggered_matches_tiled_unit_test() {
    let g = staggered((64, 32), false, false);
    let g = MapGeometry {
        width: 10,
        height: 10,
        ..g
    };
    assert_eq!(g.map_bounding_rect(), r(0, 0, 10 * 64 + 32, 10 * 16 + 16));
    assert_eq!(g.bounding_rect(r(0, 0, 1, 1)), r(0, 0, 64, 32));
    assert_eq!(
        g.bounding_rect(r(0, 1, 2, 2)),
        r(0, 16, 64 * 2 + 32, 32 + 16)
    );
    assert_eq!(
        g.bounding_rect(r(1, 1, 3, 3)),
        r(64, 16, 64 * 3 + 32, 32 * 2)
    );

    for (screen, tile) in [
        ((10.0, 16.0), t(0, 0)),
        ((5.0, 5.0), t(-1, -1)),
        ((1.0, 20.0), t(-1, 1)),
        ((64.0, 32.0), t(0, 1)),
        ((32.0, -16.0), t(0, -2)),
    ] {
        let s = v(screen.0, screen.1);
        assert_eq!(
            floor_i(g.screen_to_tile(s)),
            tile,
            "screen_to_tile{screen:?}"
        );
        assert_eq!(g.tile_at(s), tile);
    }
    for (tile, screen) in [
        ((0.0, 0.0), v(0.0, 0.0)),
        ((1.0, 0.0), v(64.0, 0.0)),
        ((0.0, 1.0), v(32.0, 16.0)),
        ((-1.0, -1.0), v(-32.0, -16.0)),
    ] {
        assert_eq!(g.tile_to_screen(v(tile.0, tile.1)), screen);
    }

    assert_eq!(g.top_left(t(0, 0)), t(-1, -1));
    assert_eq!(g.top_right(t(0, 0)), t(0, -1));
    assert_eq!(g.bottom_left(t(0, 0)), t(-1, 1));
    assert_eq!(g.bottom_right(t(0, 0)), t(0, 1));
    assert_eq!(g.top_left(t(1, 1)), t(1, 0));
    assert_eq!(g.top_right(t(1, 1)), t(2, 0));
    assert_eq!(g.bottom_left(t(1, 1)), t(1, 2));
    assert_eq!(g.bottom_right(t(1, 1)), t(2, 2));
}

type OracleCase = (
    (i32, i32, i32, bool, bool),
    (i32, i32, i32, i32),
    &'static [((f64, f64), (i32, i32))],
);

/// Tiled's hexagonal renderer's answers: `screenToTileCoords` of random points and
/// `boundingRect(-2, -3, 7, 5)`, computed with an independent port of
/// `reference/tiled/src/libtiled/hexagonalrenderer.cpp`.
const HEX_ORACLE: &[OracleCase] = &[
    (
        (70, 60, 20, true, false),
        (-90, -180, 340, 330),
        &[
            ((-105.70, -209.49), (-3, -4)),
            ((90.56, -256.54), (1, -5)),
            ((21.53, -80.59), (0, -2)),
            ((-265.20, 4.46), (-7, -1)),
            ((-277.50, -39.81), (-7, -2)),
            ((-258.09, -245.57), (-7, -5)),
            ((-45.29, 196.11), (-2, 3)),
            ((-225.72, -166.06), (-6, -3)),
            ((76.46, 268.63), (1, 3)),
            ((46.26, -61.99), (0, -2)),
            ((285.75, -272.05), (6, -5)),
            ((215.08, -126.23), (4, -3)),
        ],
    ),
    (
        (70, 60, 20, false, true),
        (-140, -120, 525, 220),
        &[
            ((-213.45, -229.32), (-4, -6)),
            ((-114.91, 189.68), (-3, 4)),
            ((-191.56, 48.96), (-4, 0)),
            ((83.35, -76.56), (1, -3)),
            ((28.65, -262.33), (0, -7)),
            ((-264.24, -176.42), (-4, -5)),
            ((108.24, -43.44), (1, -2)),
            ((-111.51, 51.34), (-2, 1)),
            ((-28.09, -120.14), (-1, -4)),
            ((176.63, 119.40), (2, 3)),
            ((-153.54, 44.65), (-3, 0)),
            ((15.12, 225.08), (0, 5)),
        ],
    ),
    (
        (14, 12, 6, false, false),
        (-28, -27, 105, 48),
        &[
            ((137.67, -127.24), (9, -15)),
            ((288.10, -229.16), (20, -26)),
            ((-49.13, 154.28), (-4, 16)),
            ((-208.81, -6.62), (-16, -1)),
            ((-276.48, 100.93), (-21, 11)),
            ((158.74, 43.82), (11, 4)),
            ((225.29, -111.75), (15, -13)),
            ((117.18, 56.62), (8, 6)),
            ((47.94, -26.28), (3, -4)),
            ((203.98, 266.81), (14, 29)),
            ((-15.54, 98.49), (-2, 10)),
            ((-263.60, 120.90), (-20, 13)),
        ],
    ),
    (
        (60, 60, 30, true, true),
        (-90, -180, 330, 330),
        &[
            ((88.28, 295.86), (1, 4)),
            ((193.15, -129.24), (4, -3)),
            ((-68.53, 101.19), (-2, 1)),
            ((-286.46, -22.98), (-7, -1)),
            ((-199.17, -229.74), (-5, -4)),
            ((-264.63, 160.94), (-7, 2)),
            ((-222.40, -151.43), (-5, -3)),
            ((-65.43, 222.85), (-2, 3)),
            ((-251.65, -30.49), (-6, -2)),
            ((29.66, 230.03), (0, 3)),
            ((191.57, 218.39), (3, 3)),
            ((-132.95, -50.82), (-4, -2)),
        ],
    ),
    (
        (15, 13, 5, true, false),
        (-20, -36, 75, 66),
        &[
            ((-84.74, 230.52), (-9, 18)),
            ((274.64, -209.45), (27, -18)),
            ((-194.27, -160.83), (-20, -14)),
            ((-160.00, -9.02), (-17, -2)),
            ((53.47, -142.35), (5, -13)),
            ((-297.54, -48.63), (-31, -5)),
            ((-78.45, 39.80), (-8, 3)),
            ((271.86, 114.30), (26, 9)),
            ((9.29, 70.56), (0, 5)),
            ((105.72, -267.60), (10, -23)),
            ((239.72, 167.98), (23, 13)),
            ((224.71, 178.72), (22, 14)),
        ],
    ),
    (
        (15, 13, 5, false, true),
        (-28, -27, 105, 49),
        &[
            ((-64.57, -60.61), (-5, -7)),
            ((-237.88, 80.57), (-18, 8)),
            ((-262.65, -259.59), (-20, -30)),
            ((-174.74, -202.62), (-13, -23)),
            ((-95.97, -268.45), (-8, -30)),
            ((-299.86, -209.24), (-22, -24)),
            ((-239.12, -81.83), (-18, -10)),
            ((-284.70, 224.60), (-21, 24)),
            ((68.44, -210.87), (4, -24)),
            ((-148.65, -91.57), (-11, -11)),
            ((-81.50, -226.29), (-7, -26)),
            ((209.36, 295.86), (14, 32)),
        ],
    ),
];

#[test]
fn hexagonal_maps_match_tileds_renderer() {
    for &((tw, th, side, stagger_x, even), (x, y, w, h), points) in HEX_ORACLE {
        let g = hexagonal((tw, th), side, stagger_x, even);
        assert_eq!(g.bounding_rect(r(-2, -3, 7, 5)), r(x, y, w, h), "{g:?}");
        for &((sx, sy), (tx, ty)) in points {
            assert_eq!(g.tile_at(v(sx, sy)), t(tx, ty), "{g:?} at ({sx}, {sy})");
            assert_eq!(g.screen_to_tile(v(sx, sy)), v(tx as f64, ty as f64));
        }
    }
}

// --- Conversions -------------------------------------------------------------------------------

#[test]
fn hexagonal_centers_round_trip() {
    for g in all_geometries().into_iter().filter(|g| g.is_hex_grid()) {
        let p = g.hex_params();
        for y in -6..6 {
            for x in -6..6 {
                let center = g.tile_to_screen(v(x as f64, y as f64))
                    + v(p.tile_width as f64, p.tile_height as f64) / 2.0;
                assert_eq!(g.tile_at(center), t(x, y), "{g:?}");
                assert_eq!(
                    g.tile_to_screen(g.tile_at(center).as_dvec2()),
                    g.tile_to_screen(v(x as f64 + 0.7, y as f64 + 0.2))
                );
            }
        }
    }
}

#[test]
fn hexagonal_parameters_and_positions() {
    // hexagonal-mini.tmx: 14×12 tiles, side 6, stagger axis Y, odd.
    let g = hexagonal((14, 12), 6, false, false);
    let p = g.hex_params();
    assert_eq!(
        (
            p.side_offset_x,
            p.side_offset_y,
            p.column_width,
            p.row_height,
            p.tile_width,
            p.tile_height
        ),
        (7, 3, 7, 9, 14, 12)
    );
    assert_eq!(
        g.tile_to_screen(v(0.0, 1.0)),
        v(7.0, 9.0),
        "odd rows are shifted right"
    );
    assert_eq!(g.tile_to_screen(v(2.0, 2.0)), v(28.0, 18.0));
    assert_eq!(g.cell_anchor(t(2, 2)), v(28.0, 30.0));
    // 20×20 map: 20 columns plus the shift, 20 rows overlapping by the side offset.
    assert_eq!(
        MapGeometry {
            width: 20,
            height: 20,
            ..g
        }
        .map_bounding_rect(),
        r(0, 0, 20 * 14 + 7, 20 * 9 + 3)
    );
    // Pointy-top: the left and right sides are vertical, the top is a corner.
    assert_eq!(
        g.tile_polygon(t(0, 0)),
        vec![
            v(0.0, 9.0),
            v(0.0, 3.0),
            v(7.0, 0.0),
            v(14.0, 3.0),
            v(14.0, 9.0),
            v(7.0, 12.0)
        ]
    );

    // test_hexagonal_tile_60x60x30.tmx: stagger axis X, flat-top.
    let g = hexagonal((60, 60), 30, true, false);
    let p = g.hex_params();
    assert_eq!(
        (
            p.side_offset_x,
            p.side_offset_y,
            p.column_width,
            p.row_height,
            p.tile_width,
            p.tile_height
        ),
        (15, 30, 45, 30, 60, 60)
    );
    assert_eq!(
        g.tile_to_screen(v(1.0, 0.0)),
        v(45.0, 30.0),
        "odd columns are shifted down"
    );
    assert_eq!(
        g.tile_polygon(t(0, 0)),
        vec![
            v(0.0, 30.0),
            v(15.0, 0.0),
            v(45.0, 0.0),
            v(60.0, 30.0),
            v(45.0, 60.0),
            v(15.0, 60.0)
        ]
    );
    assert_eq!(
        MapGeometry {
            width: 20,
            height: 20,
            ..g
        }
        .map_bounding_rect(),
        r(0, 0, 20 * 45 + 15, 20 * 60 + 30)
    );
    // A single column has no shifted neighbour to make room for.
    assert_eq!(g.bounding_rect(r(1, 0, 1, 2)), r(45, 30, 60, 120));
}

#[test]
fn isometric_conversions() {
    let g = geometry(Orientation::Isometric, (64, 32), (25, 25));
    assert_eq!(g.map_bounding_rect(), r(0, 0, 50 * 32, 50 * 16));
    assert_eq!(g.tile_to_screen(v(0.0, 0.0)), v(800.0, 0.0));
    assert_eq!(g.tile_to_screen(v(1.0, 0.0)), v(832.0, 16.0));
    assert_eq!(g.tile_to_screen(v(0.0, 1.0)), v(768.0, 16.0));
    assert_eq!(g.cell_anchor(t(0, 0)), v(768.0, 32.0));
    assert_eq!(g.screen_to_tile(v(832.0, 16.0)), v(1.0, 0.0));
    assert_eq!(g.tile_at(v(800.0, 31.0)), t(0, 0));
    assert_eq!(g.tile_at(v(800.0, 33.0)), t(1, 1));
    // Objects: tile (1, 0) is at pixel (32, 0); pixel space uses tile-height squares.
    assert_eq!(
        g.pixel_to_screen(v(32.0, 0.0)),
        g.tile_to_screen(v(1.0, 0.0))
    );
    assert_eq!(g.pixel_to_tile(v(48.0, 16.0)), v(1.5, 0.5));
    assert_eq!(
        g.tile_polygon(t(0, 0)),
        vec![
            v(800.0, 0.0),
            v(832.0, 16.0),
            v(800.0, 32.0),
            v(768.0, 16.0)
        ]
    );

    // Odd sizes: the origin and the bounding rectangle use integer halves.
    let g = geometry(Orientation::Isometric, (65, 33), (5, 7));
    assert_eq!(g.tile_to_screen(v(0.0, 0.0)), v(227.0, 0.0));
    assert_eq!(g.map_bounding_rect(), r(0, 0, 12 * 65 / 2, 12 * 33 / 2));
    assert_eq!(
        g.bounding_rect(r(-1, 2, 3, 3)),
        r((-1 - 5) * 65 / 2 + 227, 33 / 2, 6 * 65 / 2, 6 * 33 / 2)
    );
    assert_eq!(g.cell_anchor(t(1, 0)), v(227.0 + 32.5 - 32.0, 16.5 + 33.0));
}

#[test]
fn round_trips() {
    let mut rng = rng(0x5eed);
    for g in all_geometries() {
        for _ in 0..300 {
            let tile = v(
                rng.range(-40, 40) as f64 + rng.unit(),
                rng.range(-40, 40) as f64 + rng.unit(),
            );
            let pixel = v(
                rng.range(-2000, 2000) as f64 + rng.unit(),
                rng.range(-2000, 2000) as f64 + rng.unit(),
            );
            let screen = g.pixel_to_screen(pixel);
            assert!(
                g.screen_to_pixel(screen).distance(pixel) < 1e-6,
                "{g:?} pixel {pixel}"
            );
            match g.orientation {
                Orientation::Hexagonal | Orientation::Staggered => {
                    // Integer cells; staggered maps add the position inside the diamond.
                    let cell = floor_i(tile);
                    let center = g.tile_polygon(cell).iter().sum::<DVec2>()
                        / g.tile_polygon(cell).len() as f64;
                    assert_eq!(g.tile_at(center), cell, "{g:?}");
                    assert_eq!(floor_i(g.pixel_to_tile(center)), cell);
                }
                _ => {
                    assert!(
                        g.screen_to_tile(g.tile_to_screen(tile)).distance(tile) < 1e-9,
                        "{g:?} tile {tile}"
                    );
                    assert!(g.pixel_to_tile(g.tile_to_pixel(tile)).distance(tile) < 1e-9);
                    assert_eq!(g.tile_at(g.tile_to_screen(tile)), floor_i(tile));
                }
            }
        }
    }
}

#[test]
fn staggered_fraction_is_position_inside_the_diamond() {
    let g = staggered((64, 32), false, false);
    for (x, y) in [(0, 0), (3, 1), (-2, -3), (5, 4)] {
        let top = g.tile_to_screen(v(x as f64, y as f64)) + v(32.0, 0.0);
        // Top, right, bottom and left corners of the diamond in its own axes.
        for (offset, local) in [
            (v(0.0, 0.0), v(0.0, 0.0)),
            (v(32.0, 16.0), v(1.0, 0.0)),
            (v(0.0, 32.0), v(1.0, 1.0)),
            (v(-32.0, 16.0), v(0.0, 1.0)),
        ] {
            let inside = top + offset * 0.999 + v(0.0, 32.0) * 0.0005;
            let expected = v(x as f64, y as f64) + local;
            assert!(
                g.screen_to_tile(inside).distance(expected) < 0.01,
                "({x}, {y}) {offset}: {}",
                g.screen_to_tile(inside)
            );
        }
    }
}

#[test]
fn oblique_conversions() {
    let g = oblique((32, 32), (12, 0));
    assert_eq!(
        g.tile_to_screen(v(0.0, 1.0)),
        v(12.0, 32.0),
        "each row shifts by skewx"
    );
    assert_eq!(g.pixel_to_screen(v(32.0, 64.0)), v(56.0, 64.0));
    assert_eq!(g.cell_anchor(t(2, 0)), v(76.0, 32.0));
    assert_eq!(
        MapGeometry {
            width: 10,
            height: 10,
            ..g
        }
        .map_bounding_rect(),
        r(0, 0, 320 + 120, 320)
    );
    let g = MapGeometry {
        width: 10,
        height: 10,
        ..oblique((32, 16), (-8, 4))
    };
    assert_eq!(g.map_bounding_rect(), r(-80, 0, 320 + 80, 160 + 40));
    assert_eq!(g.bounding_rect(r(1, 1, 1, 1)), r(16, 20, 40, 20));
    // A singular shear (det = 0) falls back to the identity, like Qt.
    let g = oblique((10, 10), (10, 10));
    assert_eq!(g.screen_to_pixel(v(3.0, 4.0)), v(3.0, 4.0));
}

#[test]
fn tile_at_finds_the_cell_under_points_inside_it() {
    let mut rng = rng(42);
    for g in all_geometries() {
        for _ in 0..200 {
            let tile = t(rng.range(-30, 30), rng.range(-30, 30));
            let polygon = g.tile_polygon(tile);
            let center = polygon.iter().sum::<DVec2>() / polygon.len() as f64;
            // Points between the center and the corners, well inside the outline.
            for corner in &polygon {
                let p = center + (*corner - center) * (0.1 + 0.6 * rng.unit());
                assert_eq!(g.tile_at(p), tile, "{g:?} point {p} of {polygon:?}");
            }
        }
    }
}

// --- Draw order and iteration ------------------------------------------------------------------

#[test]
fn iteration_follows_draw_order_and_covers_the_rect() {
    let mut rng = rng(7);
    for g in all_geometries() {
        let tile = g.tile_size_f();
        for _ in 0..40 {
            let min = v(
                rng.range(-600, 900) as f64 + rng.unit(),
                rng.range(-600, 900) as f64 + rng.unit(),
            );
            let size = v(rng.range(1, 500) as f64, rng.range(1, 500) as f64);
            let exposed = DRect::from_min_size(min, size);
            let mut visited = Vec::new();
            g.for_each_tile(exposed, |tile, anchor| {
                assert_eq!(anchor, g.cell_anchor(tile));
                visited.push(tile);
            });
            for pair in visited.windows(2) {
                assert!(
                    g.draw_order_key(pair[0]) < g.draw_order_key(pair[1]),
                    "{g:?} {exposed:?}: {pair:?}"
                );
            }
            // Every cell whose outline contains a point of the rectangle is visited (finite oblique
            // maps are clamped to the map, like in Tiled).
            let set: HashSet<IVec2> = visited.iter().copied().collect();
            let step = (tile.min_element() / 4.0).max(size.max_element() / 50.0);
            let (nx, ny) = ((size.x / step) as i32, (size.y / step) as i32);
            for i in 0..=nx {
                for j in 0..=ny {
                    let p = (min + v(i as f64 * step, j as f64 * step)).min(exposed.max - 0.01);
                    let near = g.tile_at(p);
                    for cell in r(near.x - 1, near.y - 1, 3, 3).points() {
                        let clamped = !g.infinite
                            && g.orientation == Orientation::Oblique
                            && !r(0, 0, g.width + 1, g.height + 1).contains(cell);
                        if !clamped && polygon_contains(&g.tile_polygon(cell), p) {
                            assert!(
                                set.contains(&cell),
                                "{g:?} {exposed:?}: {cell} at {p} not visited"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Whether `p` is strictly inside a convex polygon.
fn polygon_contains(polygon: &[DVec2], p: DVec2) -> bool {
    let n = polygon.len();
    let sides: Vec<f64> = (0..n)
        .map(|i| (polygon[(i + 1) % n] - polygon[i]).perp_dot(p - polygon[i]))
        .collect();
    sides.iter().all(|s| *s > 1e-9) || sides.iter().all(|s| *s < -1e-9)
}

#[test]
fn render_order_is_honored_only_where_tiled_does() {
    for orientation in [
        Orientation::Orthogonal,
        Orientation::Oblique,
        Orientation::Isometric,
        Orientation::Hexagonal,
    ] {
        let order = |render_order| {
            let g = MapGeometry {
                render_order,
                hex_side_length: 8,
                ..geometry(orientation, (32, 32), (4, 4))
            };
            let mut tiles = Vec::new();
            g.for_each_tile(DRect::from(g.map_bounding_rect()), |tile, _| {
                tiles.push(tile)
            });
            tiles
        };
        let (down, up) = (order(RenderOrder::RightDown), order(RenderOrder::LeftUp));
        let honored = matches!(orientation, Orientation::Orthogonal | Orientation::Oblique);
        assert_eq!(down != up, honored, "{orientation:?}");
    }
}

#[test]
fn isometric_draw_order_is_by_diagonal() {
    let g = geometry(Orientation::Isometric, (64, 32), (3, 3));
    let mut tiles = Vec::new();
    g.for_each_tile(DRect::from(g.map_bounding_rect()), |tile, _| {
        if r(0, 0, 3, 3).contains(tile) {
            tiles.push((tile.x, tile.y));
        }
    });
    assert_eq!(
        tiles,
        [
            (0, 0),
            (0, 1),
            (1, 0),
            (0, 2),
            (1, 1),
            (2, 0),
            (1, 2),
            (2, 1),
            (2, 2)
        ]
    );
}

#[test]
fn hexagonal_stagger_x_draws_half_rows() {
    let g = MapGeometry {
        width: 4,
        height: 2,
        ..hexagonal((60, 60), 30, true, false)
    };
    let mut tiles = Vec::new();
    g.for_each_tile(DRect::from(g.map_bounding_rect()), |tile, _| {
        if r(0, 0, 4, 2).contains(tile) {
            tiles.push((tile.x, tile.y));
        }
    });
    assert_eq!(
        tiles,
        [
            (0, 0),
            (2, 0),
            (1, 0),
            (3, 0),
            (0, 1),
            (2, 1),
            (1, 1),
            (3, 1)
        ]
    );
}

// --- Outlines, grid and snapping ---------------------------------------------------------------

/// Whether `b` is `a` starting at another vertex.
fn same_loop(a: &[DVec2], b: &[DVec2]) -> bool {
    a.len() == b.len() && (0..a.len()).any(|k| (0..a.len()).all(|i| a[(i + k) % a.len()] == b[i]))
}

#[test]
fn outlines() {
    let g = geometry(Orientation::Orthogonal, (16, 16), (8, 8));
    assert_eq!(
        g.rect_outline(r(1, 2, 3, 2)),
        vec![vec![
            v(16.0, 32.0),
            v(64.0, 32.0),
            v(64.0, 64.0),
            v(16.0, 64.0)
        ]]
    );
    // An L-shape: six corners, the shared edges and collinear points removed.
    let l = g.region_outline([t(0, 0), t(0, 1), t(1, 1)]);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].len(), 6);
    // Two separate cells give two loops.
    assert_eq!(g.region_outline([t(0, 0), t(5, 5), t(0, 0)]).len(), 2);

    let iso = geometry(Orientation::Isometric, (64, 32), (4, 4));
    assert_eq!(
        iso.rect_outline(r(0, 0, 2, 1)),
        vec![vec![
            v(128.0, 0.0),
            v(192.0, 32.0),
            v(160.0, 48.0),
            v(96.0, 16.0)
        ]]
    );
    assert_eq!(
        iso.region_outline(r(0, 0, 2, 1).points()),
        iso.rect_outline(r(0, 0, 2, 1))
    );

    for g in all_geometries()
        .into_iter()
        .filter(|g| g.orientation == Orientation::Hexagonal)
    {
        // A hexagon and its six neighbors: a single loop with 18 corners.
        let cells: Vec<IVec2> = std::iter::once(t(3, 3))
            .chain(g.neighbors(t(3, 3)))
            .collect();
        let loops = g.region_outline(cells);
        assert_eq!(loops.len(), 1, "{g:?}");
        assert_eq!(loops[0].len(), 18, "{g:?}");
        assert_eq!(g.rect_outline(r(1, 1, 3, 3)).len(), 1);
        assert!(same_loop(
            &g.region_outline([t(2, 2)])[0],
            &g.tile_polygon(t(2, 2))
        ));
    }
    // Staggered cells next to each other in a row only touch at a corner.
    let g = staggered((64, 32), false, false);
    let outline = g.rect_outline(r(0, 0, 2, 1));
    assert_eq!(outline.iter().map(|l| l.len()).sum::<usize>(), 8);
    assert_eq!(g.rect_outline(r(0, 0, 2, 2)).len(), 1);
}

#[test]
fn orthogonal_grid_leaves_the_far_edges_to_the_border() {
    let g = geometry(Orientation::Orthogonal, (16, 16), (4, 3));
    let lines = g.grid_lines(
        DRect {
            min: v(-100.0, -100.0),
            max: v(500.0, 500.0),
        },
        IVec2::new(2, 0),
    );
    let vertical: Vec<f64> = lines
        .iter()
        .filter(|l| l.from.x == l.to.x)
        .map(|l| l.from.x)
        .collect();
    assert_eq!(vertical, [0.0, 16.0, 32.0, 48.0]);
    assert_eq!(lines.iter().filter(|l| l.from.y == l.to.y).count(), 3);
    assert_eq!(
        lines.iter().filter(|l| l.major).count(),
        2,
        "x = 0 and x = 2"
    );
    assert_eq!(
        lines[0],
        GridLine {
            from: v(0.0, 0.0),
            to: v(0.0, 48.0),
            major: true
        }
    );
}

#[test]
fn grid_lines_follow_cell_outlines() {
    for g in all_geometries() {
        let g = MapGeometry {
            width: 6,
            height: 5,
            ..g
        };
        let bounds = DRect::from(g.map_bounding_rect()).expanded(v(40.0, 40.0));
        let lines = g.grid_lines(bounds, IVec2::ZERO);
        assert!(!lines.is_empty(), "{g:?}");
        // Every grid line runs along edges of cells: both of its ends are cell corners.
        let mut corners = HashSet::new();
        for y in -2..8 {
            for x in -2..9 {
                for p in g.tile_polygon(t(x, y)) {
                    corners.insert(((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64));
                }
            }
        }
        for line in &lines {
            for p in [line.from, line.to] {
                assert!(
                    corners.contains(&((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64)),
                    "{g:?}: {line:?}"
                );
            }
        }
    }
    // Hexagonal maps draw each edge once: a 2×2 map with stagger axis Y.
    let g = MapGeometry {
        width: 2,
        height: 2,
        ..hexagonal((14, 12), 6, false, false)
    };
    let lines = g.grid_lines(
        DRect::from(g.map_bounding_rect()).expanded(v(20.0, 20.0)),
        IVec2::ZERO,
    );
    let mut edges = HashSet::new();
    for line in &lines {
        let key = |p: DVec2| (p.x as i32, p.y as i32);
        assert!(
            edges.insert((
                key(line.from).min(key(line.to)),
                key(line.from).max(key(line.to))
            )),
            "duplicate {line:?}"
        );
    }
    let outline_edges: usize = g.region_outline(r(0, 0, 2, 2).points())[0].len();
    // 4 hexagons × 6 edges, minus the 5 shared ones counted twice.
    assert_eq!(edges.len(), 4 * 6 - 5);
    assert_eq!(outline_edges, 4 * 6 - 2 * 5);
}

/// Edges of a cell outline as sorted integer point pairs.
fn cell_edges(g: &MapGeometry, tile: IVec2) -> Vec<((i64, i64), (i64, i64))> {
    let key = |p: DVec2| ((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64);
    let polygon = g.tile_polygon(tile);
    (0..polygon.len())
        .map(|i| {
            let (a, b) = (key(polygon[i]), key(polygon[(i + 1) % polygon.len()]));
            (a.min(b), a.max(b))
        })
        .collect()
}

#[test]
fn hexagonal_grid_draws_every_edge_of_the_map_once() {
    let mut rng = rng(3);
    for g in all_geometries().into_iter().filter(|g| g.is_hex_grid()) {
        let g = MapGeometry {
            width: 7,
            height: 6,
            ..g
        };
        let map = r(0, 0, g.width, g.height);
        let mut map_edges = HashSet::new();
        for tile in map.points() {
            map_edges.extend(cell_edges(&g, tile));
        }
        let bounds = DRect::from(g.map_bounding_rect());
        for _ in 0..30 {
            let min = bounds.min + (bounds.size() + 100.0) * v(rng.unit(), rng.unit()) - 80.0;
            let visible =
                DRect::from_min_size(min, v(rng.range(20, 400) as f64, rng.range(20, 400) as f64));
            let lines = g.grid_lines(visible, IVec2::ZERO);
            let mut drawn = HashSet::new();
            for line in &lines {
                let key = |p: DVec2| ((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64);
                let edge = (
                    key(line.from).min(key(line.to)),
                    key(line.from).max(key(line.to)),
                );
                assert!(
                    map_edges.contains(&edge),
                    "{g:?} {visible:?}: {line:?} is not an edge of the map"
                );
                assert!(
                    drawn.insert(edge),
                    "{g:?} {visible:?}: {line:?} drawn twice"
                );
            }
            // Edges with their midpoint well inside the visible area are all drawn.
            for &(a, b) in &map_edges {
                let mid = v((a.0 + b.0) as f64 / 4.0, (a.1 + b.1) as f64 / 4.0);
                if visible.expanded(v(-1.0, -1.0)).contains(mid) {
                    assert!(
                        drawn.contains(&(a, b)),
                        "{g:?} {visible:?}: edge {a:?}-{b:?} missing"
                    );
                }
            }
        }
    }
}

#[test]
fn snapping() {
    let g = geometry(Orientation::Orthogonal, (16, 16), (8, 8));
    assert_eq!(g.snap_to_grid(v(23.0, 9.0), 1), v(16.0, 16.0));
    assert_eq!(g.snap_to_grid(v(23.0, 9.0), 4), v(24.0, 8.0));
    let iso = geometry(Orientation::Isometric, (64, 32), (8, 8));
    assert_eq!(
        iso.snap_to_grid(v(40.0, 20.0), 1),
        v(32.0, 32.0),
        "tile-height squares"
    );
    assert_eq!(iso.snap_to_grid(v(40.0, 20.0), 2), v(48.0, 16.0));
    let hex = hexagonal((14, 12), 6, false, false);
    assert_eq!(
        hex.snap_to_grid(v(6.0, 1.0), 1),
        v(7.0, 0.0),
        "nearest corner"
    );
    assert_eq!(hex.snap_to_grid(v(6.0, 5.0), 1), v(7.0, 0.0));
    assert_eq!(
        hex.snap_to_grid(v(6.0, 5.0), 2),
        v(7.0, 6.0),
        "the center when subdivided"
    );
}

#[test]
fn object_alignment_defaults() {
    let ts = Tileset::default();
    let aligned = Tileset {
        object_alignment: Alignment::Center,
        ..Tileset::default()
    };
    for g in all_geometries() {
        let tile_default = if g.orientation == Orientation::Isometric {
            Alignment::Bottom
        } else {
            Alignment::BottomLeft
        };
        assert_eq!(g.object_alignment(Some(&ts)), tile_default);
        assert_eq!(g.object_alignment(Some(&aligned)), Alignment::Center);
        assert_eq!(g.object_alignment(None), Alignment::TopLeft);
    }
    let iso = geometry(Orientation::Isometric, (64, 32), (4, 4));
    let r = iso.upright_object_rect(v(32.0, 32.0), v(64.0, 64.0), Alignment::Bottom);
    assert_eq!(
        r,
        DRect::from_min_size(
            iso.tile_to_screen(v(1.0, 1.0)) - v(32.0, 64.0),
            v(64.0, 64.0)
        )
    );
}

// --- Hexagonal neighbors, cube coordinates and stamps -------------------------------------------

fn cube_distance(a: Hex, b: Hex) -> i32 {
    ((a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs()) / 2
}

#[test]
fn cube_coordinates() {
    for g in all_geometries()
        .into_iter()
        .filter(|g| g.orientation == Orientation::Hexagonal)
    {
        let (axis, index) = (g.stagger_axis, g.stagger_index);
        for y in -5..5 {
            for x in -5..5 {
                let hex = Hex::from_offset(t(x, y), axis, index);
                assert_eq!(hex.x + hex.y + hex.z, 0);
                assert_eq!(hex.to_offset(axis, index), t(x, y));
                let mut r = hex;
                for _ in 0..6 {
                    r = r.rotated_cw();
                }
                assert_eq!(r, hex);
                assert_eq!(hex.rotated_cw().rotated_ccw(), hex);
                // Neighbors are one step away in cube coordinates and also share an edge on screen.
                let neighbors: Vec<IVec2> = g.neighbors(t(x, y)).collect();
                assert_eq!(neighbors.len(), 6);
                for n in neighbors {
                    assert_eq!(
                        cube_distance(hex, Hex::from_offset(n, axis, index)),
                        1,
                        "{g:?} ({x}, {y}) -> {n}"
                    );
                    let shared = g.region_outline([t(x, y), n]);
                    assert_eq!(shared.len(), 1);
                    assert_eq!(shared[0].len(), 10, "two hexagons sharing an edge");
                }
            }
        }
    }
    // Rotating clockwise turns "east" into "south-east" on screen (pointy-top hexagons).
    let east = Hex::from_offset(t(1, 0), StaggerAxis::Y, StaggerIndex::Odd);
    let g = hexagonal((14, 12), 6, false, false);
    assert_eq!(
        east.rotated_cw()
            .to_offset(StaggerAxis::Y, StaggerIndex::Odd),
        g.bottom_right(t(0, 0))
    );
}

#[test]
fn infinite_map_bounds_are_chunk_based() {
    let mut map = Map {
        infinite: true,
        orientation: Orientation::Staggered,
        tile_width: 64,
        tile_height: 32,
        width: 25,
        height: 50,
        ..Map::default()
    };
    assert_eq!(map.tile_bounding_rect(), r(0, 0, 1, 1));
    let mut layer = TileLayer::infinite(UVec2::ZERO);
    let cell = Cell::new(TilesetSlot::new(0).unwrap(), TileId(0));
    layer.set_cell(t(3, 20), cell).unwrap();
    layer.set_cell(t(-1, 2), cell).unwrap();
    map.layers
        .push(Layer::new(LayerId(1), "l", LayerKind::Tile(layer)));
    assert_eq!(map.tile_bounding_rect(), r(-16, 0, 32, 32));
    assert_eq!(
        map.bounding_rect(),
        map.geometry().bounding_rect(r(-16, 0, 32, 32))
    );
    assert_eq!(
        map.bounding_rect(),
        r(-16 * 64, 0, 32 * 64 + 32, 32 * 16 + 16)
    );
}
