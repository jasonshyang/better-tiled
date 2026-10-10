//! Which objects a point or a rubber band picks, as Tiled's object tools pick them
//! (`reference/tiled/src/tiled/abstractobjecttool.cpp` `mapObjectsAt` and `topMostMapObjectAt`,
//! `objectselectiontool.cpp` `objectsAboutToBeSelected`, and the renderers' `interactionShape`):
//! inside filled shapes or near their outline, near lines and points, topmost first, hidden
//! objects never. Tile objects are picked by their image's box, not its opaque pixels (Tiled's
//! "precise tile object selection" needs the image, which this crate does not have).

use bt_core::{MapObject, ObjectId, ObjectLayer, Outline};
use glam::DVec2;

use super::order::render_order;
use super::space::ObjectSpace;

/// What a rubber band selects.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum RectMode {
    /// Objects it touches (Tiled's default).
    #[default]
    Touching,
    /// Objects it holds whole.
    Enclosed,
}

/// Whether `point` (pixel space) picks `object`: inside a filled shape (rectangles, ellipses,
/// capsules, polygons, tile and text boxes), or within `tolerance` of a shape's outline, of a
/// polyline or of a point. Rectangles, ellipses and capsules without a size are picked through
/// Tiled's marker, a 20-pixel square or circle around their position. `tolerance` is measured
/// on screen, in the map's pixels at a zoom of one, so it means the same on every orientation.
pub fn object_contains(
    space: &ObjectSpace,
    object: &MapObject,
    point: DVec2,
    tolerance: f64,
) -> bool {
    space
        .hit_outline(object)
        .hits(space.geometry().pixel_to_screen(point), tolerance)
}

/// The shown objects of a list `point` picks (see [`object_contains`]), topmost first.
pub fn objects_at(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    point: DVec2,
    tolerance: f64,
) -> Vec<ObjectId> {
    let screen = space.geometry().pixel_to_screen(point);
    render_order(objects, space.geometry())
        .into_iter()
        .rev()
        .filter(|o| o.visible && space.hit_outline(o).hits(screen, tolerance))
        .map(|o| o.id)
        .collect()
}

/// The shown objects of a list a rubber band selects, topmost first. The band is a quad in pixel
/// space, its corners in order around it: a rectangle on screen is one on orthogonal maps and a
/// parallelogram on isometric and oblique ones. A point is selected when the band holds it.
pub fn objects_in_quad(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    quad: [DVec2; 4],
    mode: RectMode,
) -> Vec<ObjectId> {
    let band: Vec<DVec2> = quad
        .iter()
        .map(|&corner| space.geometry().pixel_to_screen(corner))
        .collect();
    let area = Outline::Area(band.clone());
    render_order(objects, space.geometry())
        .into_iter()
        .rev()
        .filter(|o| o.visible)
        .filter(|o| {
            let outline = space.hit_outline(o);
            match mode {
                RectMode::Touching => outline.touches(&area),
                RectMode::Enclosed => outline.lies_within(&band),
            }
        })
        .map(|o| o.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, DrawOrder, Map, Orientation, Shape, TextData, TileId, TilesetSlot};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::rng::Rng;
    use glam::UVec2;

    use super::*;
    use crate::objects::bounds::object_bounds;
    use crate::testutil::rect_object;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn flat_map() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::splat(16))
            .tileset(Atlas::builder().build_embedded().unwrap().tileset)
            .build()
            .unwrap()
    }

    fn with_shape(shape: Shape, object: MapObject) -> MapObject {
        MapObject { shape, ..object }
    }

    #[test]
    fn every_shape_is_hit_inside_or_near() {
        let map = flat_map();
        let space = ObjectSpace::of_map(&map);
        let hits = |o: &MapObject, x, y, tolerance| object_contains(&space, o, v(x, y), tolerance);
        let rect = rect_object(10.0, 10.0, 20.0, 10.0);
        assert!(hits(&rect, 20.0, 15.0, 0.0) && hits(&rect, 30.0, 20.0, 0.0));
        assert!(!hits(&rect, 32.0, 15.0, 0.0) && hits(&rect, 32.0, 15.0, 2.0));
        let ellipse = with_shape(Shape::Ellipse, rect.clone());
        assert!(hits(&ellipse, 20.0, 15.0, 0.0));
        assert!(
            !hits(&ellipse, 11.0, 11.0, 0.0),
            "the box's corner is outside"
        );
        let capsule = with_shape(Shape::Capsule, rect.clone());
        assert!(hits(&capsule, 20.0, 10.5, 0.0), "the straight side");
        assert!(!hits(&capsule, 10.5, 10.5, 0.0), "the rounded end");
        let polygon = with_shape(
            Shape::Polygon(vec![v(0.0, 0.0), v(20.0, 0.0), v(0.0, 20.0)]),
            rect.clone(),
        );
        assert!(hits(&polygon, 14.0, 14.0, 0.0) && !hits(&polygon, 25.0, 25.0, 0.0));
        assert!(hits(&polygon, 21.0, 20.0, 1.0), "near the long side");
        let polyline = with_shape(
            Shape::Polyline(vec![v(0.0, 0.0), v(20.0, 0.0), v(0.0, 20.0)]),
            rect.clone(),
        );
        assert!(!hits(&polyline, 14.0, 14.0, 0.0), "a line holds nothing");
        assert!(hits(&polyline, 20.0, 10.5, 1.0) && hits(&polyline, 21.0, 20.0, 1.0));
        let point = with_shape(Shape::Point, rect.clone());
        assert!(hits(&point, 12.0, 10.0, 3.0) && !hits(&point, 14.0, 10.0, 3.0));
        let text = with_shape(Shape::Text(TextData::default()), rect.clone());
        assert!(hits(&text, 29.0, 19.0, 0.0));
        let tile = MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            ..rect_object(10.0, 30.0, 16.0, 16.0)
        };
        assert!(
            hits(&tile, 12.0, 20.0, 0.0),
            "the image above a bottom-left position"
        );
        assert!(!hits(&tile, 12.0, 32.0, 0.0));
    }

    #[test]
    fn turned_objects_are_hit_where_they_are_drawn() {
        let map = flat_map();
        let space = ObjectSpace::of_map(&map);
        // A 40×10 bar turned a quarter clockwise around its top-left hangs down and left.
        let bar = MapObject {
            rotation: 90.0,
            ..rect_object(100.0, 100.0, 40.0, 10.0)
        };
        assert!(object_contains(&space, &bar, v(95.0, 130.0), 0.0));
        assert!(!object_contains(&space, &bar, v(120.0, 105.0), 0.0));
        let leaning = MapObject {
            rotation: 45.0,
            ..bar.clone()
        };
        let along =
            v(100.0, 100.0) + DVec2::from_angle(std::f64::consts::FRAC_PI_4).rotate(v(30.0, 5.0));
        assert!(object_contains(&space, &leaning, along, 0.0));
        assert!(!object_contains(&space, &leaning, v(130.0, 100.0), 0.0));
        let ellipse = MapObject {
            shape: Shape::Ellipse,
            ..bar
        };
        assert!(object_contains(&space, &ellipse, v(95.0, 120.0), 0.0));
        assert!(!object_contains(&space, &ellipse, v(91.0, 101.0), 0.0));
    }

    #[test]
    fn sizeless_shapes_are_hit_through_tileds_marker() {
        let map = flat_map();
        let space = ObjectSpace::of_map(&map);
        let marker = rect_object(50.0, 50.0, 0.0, 0.0);
        assert!(object_contains(&space, &marker, v(59.0, 59.0), 0.0));
        assert!(!object_contains(&space, &marker, v(61.0, 50.0), 0.0));
        let turned = MapObject {
            rotation: 45.0,
            ..marker.clone()
        };
        assert!(
            !object_contains(&space, &turned, v(59.0, 59.0), 0.0),
            "the turned square's corner is elsewhere"
        );
        assert!(object_contains(&space, &turned, v(63.0, 50.0), 0.0));
        let circle = with_shape(Shape::Capsule, marker);
        assert!(object_contains(&space, &circle, v(50.0, 59.5), 0.0));
        assert!(!object_contains(&space, &circle, v(58.0, 58.0), 0.0));
    }

    #[test]
    fn the_topmost_shown_object_comes_first() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(16))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 20.0, 30.0, 30.0),
                    rect_object(10.0, 0.0, 30.0, 30.0),
                    MapObject {
                        visible: false,
                        ..rect_object(0.0, 0.0, 50.0, 50.0)
                    },
                    rect_object(15.0, 10.0, 5.0, 5.0),
                ],
            )
            .build()
            .unwrap();
        let space = ObjectSpace::of_map(&map);
        let layer = map.layers[0].as_object_layer().unwrap();
        // Drawn top-down: 2 (y 0), 4 (y 10), 1 (y 20); 3 is hidden.
        assert_eq!(
            objects_at(layer, &space, v(17.0, 25.0), 0.0),
            [ObjectId(1), ObjectId(2)]
        );
        assert_eq!(
            objects_at(layer, &space, v(17.0, 12.0), 0.0),
            [ObjectId(4), ObjectId(2)]
        );
        let listed = ObjectLayer {
            draw_order: DrawOrder::Index,
            ..layer.clone()
        };
        assert_eq!(
            objects_at(&listed, &space, v(17.0, 25.0), 0.0),
            [ObjectId(2), ObjectId(1)],
            "in list order the later one is on top"
        );
    }

    #[test]
    fn rubber_bands_touch_or_enclose() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(16))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    rect_object(20.0, 0.0, 10.0, 10.0),
                    MapObject {
                        shape: Shape::Polyline(vec![v(0.0, 0.0), v(40.0, 0.0)]),
                        ..rect_object(0.0, 30.0, 0.0, 0.0)
                    },
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(25.0, 20.0, 0.0, 0.0)
                    },
                ],
            )
            .build()
            .unwrap();
        let space = ObjectSpace::of_map(&map);
        let layer = map.layers[0].as_object_layer().unwrap();
        let band =
            |x0: f64, y0: f64, x1: f64, y1: f64| [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
        let mut touched = objects_in_quad(
            layer,
            &space,
            band(5.0, -5.0, 26.0, 35.0),
            RectMode::Touching,
        );
        touched.sort();
        assert_eq!(
            touched,
            [ObjectId(1), ObjectId(2), ObjectId(3), ObjectId(4)]
        );
        assert_eq!(
            objects_in_quad(
                layer,
                &space,
                band(-1.0, -1.0, 32.0, 25.0),
                RectMode::Enclosed
            ),
            [ObjectId(4), ObjectId(2), ObjectId(1)],
            "the polyline pokes out; topmost first"
        );
        assert!(
            objects_in_quad(
                layer,
                &space,
                band(12.0, 12.0, 18.0, 18.0),
                RectMode::Touching
            )
            .is_empty(),
            "between them"
        );
    }

    #[test]
    fn screen_rectangles_are_quads_on_isometric_maps() {
        let map = MapBuilder::new(UVec2::new(64, 32), UVec2::splat(8))
            .orientation(Orientation::Isometric)
            .object_layer(
                "things",
                vec![
                    // The square of pixels of cell (1, 1), drawn as its diamond.
                    rect_object(32.0, 32.0, 32.0, 32.0),
                    // A point at the diamond's right corner, and one far below.
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(64.0, 32.0, 0.0, 0.0)
                    },
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(200.0, 200.0, 0.0, 0.0)
                    },
                ],
            )
            .build()
            .unwrap();
        let geometry = map.geometry();
        let space = ObjectSpace::of_map(&map);
        let layer = map.layers[0].as_object_layer().unwrap();
        // A screen rectangle around the diamond and the corner point.
        let diamond = [v(1.0, 1.0), v(2.0, 1.0), v(2.0, 2.0), v(1.0, 2.0)]
            .map(|t| geometry.tile_to_screen(t));
        let min = diamond.iter().fold(v(f64::MAX, f64::MAX), |a, &b| a.min(b)) - v(1.0, 1.0);
        let max = diamond.iter().fold(v(f64::MIN, f64::MIN), |a, &b| a.max(b)) + v(1.0, 1.0);
        let quad = [
            v(min.x, min.y),
            v(max.x, min.y),
            v(max.x, max.y),
            v(min.x, max.y),
        ]
        .map(|corner| geometry.screen_to_pixel(corner));
        let mut enclosed = objects_in_quad(layer, &space, quad, RectMode::Enclosed);
        enclosed.sort();
        assert_eq!(enclosed, [ObjectId(1), ObjectId(2)]);
        // A small screen rectangle in the corner of that box misses the diamond.
        let corner = [
            v(min.x, min.y),
            v(min.x + 10.0, min.y),
            v(min.x + 10.0, min.y + 5.0),
            v(min.x, min.y + 5.0),
        ]
        .map(|c| geometry.screen_to_pixel(c));
        assert!(objects_in_quad(layer, &space, corner, RectMode::Touching).is_empty());
        let middle = (diamond[0] + diamond[2]) * 0.5;
        assert_eq!(
            objects_at(layer, &space, geometry.screen_to_pixel(middle), 0.0),
            [ObjectId(1)]
        );
    }

    #[test]
    fn collision_shapes_are_hit_in_their_tiles_space() {
        // On a tileset with an isometric grid, a tile's shapes are drawn on one cell's diamond.
        let mut tileset = Atlas::builder()
            .tile_size(64, 32)
            .build_embedded()
            .unwrap()
            .tileset
            .as_ref()
            .clone();
        tileset.grid = Some(bt_core::TilesetGrid {
            orientation: bt_core::GridOrientation::Isometric,
            size: UVec2::new(64, 32),
        });
        let space = ObjectSpace::of_tile(&tileset, TileId(0));
        let shapes = ObjectLayer {
            draw_order: DrawOrder::Index,
            objects: Arc::new(vec![
                MapObject {
                    id: ObjectId(1),
                    ..rect_object(0.0, 0.0, 32.0, 32.0)
                },
                MapObject {
                    id: ObjectId(2),
                    shape: Shape::Ellipse,
                    ..rect_object(8.0, 8.0, 16.0, 16.0)
                },
            ]),
            ..ObjectLayer::default()
        };
        // Pixel (16, 16) is drawn at the middle of the diamond, (32, 16) on screen.
        assert_eq!(
            space.geometry().pixel_to_screen(v(16.0, 16.0)),
            v(32.0, 16.0)
        );
        assert_eq!(
            objects_at(&shapes, &space, v(16.0, 16.0), 0.0),
            [ObjectId(2), ObjectId(1)]
        );
        let corner = space.geometry().screen_to_pixel(v(2.0, 2.0));
        assert!(
            objects_at(&shapes, &space, corner, 0.0).is_empty(),
            "the screen's corner is outside the diamond"
        );
        let band = [v(-1.0, -1.0), v(33.0, -1.0), v(33.0, 33.0), v(-1.0, 33.0)];
        assert_eq!(
            objects_in_quad(&shapes, &space, band, RectMode::Enclosed),
            [ObjectId(2), ObjectId(1)]
        );
    }

    /// The corners of a rectangle, clockwise.
    fn corners(r: bt_utils::DRect) -> [DVec2; 4] {
        [r.min, v(r.max.x, r.min.y), r.max, v(r.min.x, r.max.y)]
    }

    #[test]
    fn what_is_hit_lies_within_the_bounds() {
        let fuzz = Fuzz::from_env(0x00B1_EC75, 200);
        let mut hit = 0;
        let atlas = Atlas::builder()
            .tile_size(16, 24)
            .tile_offset(glam::IVec2::new(3, -5))
            .build_embedded()
            .unwrap();
        for orientation in [
            Orientation::Orthogonal,
            Orientation::Isometric,
            Orientation::Oblique,
        ] {
            let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
                .orientation(orientation)
                .skew(glam::IVec2::new(8, 0))
                .tileset(Arc::clone(&atlas.tileset))
                .build()
                .unwrap();
            let space = ObjectSpace::of_map(&map);
            for case in 0..fuzz.cases {
                let mut rng = Rng::new(fuzz.case_seed(case));
                let object = crate::testutil::random_object(&mut rng);
                if object.tile.is_none()
                    && object.size == DVec2::ZERO
                    && matches!(
                        object.shape,
                        Shape::Rectangle | Shape::Ellipse | Shape::Capsule
                    )
                {
                    continue;
                }
                let bounds = object_bounds(&space, &object).expanded(DVec2::splat(1e-6));
                let context = || {
                    format!(
                        "case {case} ({}) on {orientation:?}: {object:?}",
                        fuzz.reproduce()
                    )
                };
                for _ in 0..20 {
                    let p = object.pos
                        + v(rng.next_f64() * 100.0 - 50.0, rng.next_f64() * 100.0 - 50.0);
                    if object_contains(&space, &object, p, 0.0) {
                        hit += 1;
                        assert!(
                            bounds.min.x <= p.x
                                && p.x <= bounds.max.x
                                && bounds.min.y <= p.y
                                && p.y <= bounds.max.y,
                            "{p} is hit but not in {bounds:?}: {}",
                            context()
                        );
                    }
                }
                // A band around the bounds holds the object; a band holding it touches it.
                let layer = ObjectLayer {
                    objects: Arc::new(vec![MapObject {
                        id: ObjectId(1),
                        visible: true,
                        ..object.clone()
                    }]),
                    ..ObjectLayer::default()
                };
                assert_eq!(
                    objects_in_quad(&layer, &space, corners(bounds), RectMode::Enclosed),
                    [ObjectId(1)],
                    "{}",
                    context()
                );
                let band = bt_utils::DRect::from_points([
                    object.pos + v(rng.next_f64() * 160.0 - 80.0, rng.next_f64() * 160.0 - 80.0),
                    object.pos + v(rng.next_f64() * 160.0 - 80.0, rng.next_f64() * 160.0 - 80.0),
                ])
                .unwrap();
                if !objects_in_quad(&layer, &space, corners(band), RectMode::Enclosed).is_empty() {
                    assert_eq!(
                        objects_in_quad(&layer, &space, corners(band), RectMode::Touching),
                        [ObjectId(1)],
                        "{}",
                        context()
                    );
                }
            }
        }
        assert!(hit > 300, "only {hit} points hit anything");
    }
}
