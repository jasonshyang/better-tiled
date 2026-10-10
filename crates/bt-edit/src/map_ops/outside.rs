//! Which objects a resized map leaves outside. An object is outside when its rectangle on screen,
//! turned with it around its position, does not touch the screen rectangle of the map's new
//! area (Tiled's `visibleIn` in `MapDocument::resizeMap`,
//! `reference/tiled/src/tiled/mapdocument.cpp`). The rectangle is the one Tiled's renderers give
//! (`boundingRect(const MapObject *)`, `libtiled/orthogonalrenderer.cpp` and
//! `isometricrenderer.cpp`) without the margins they add for the pen: tile and text objects
//! upright at their projected position, other shapes projected corner by corner, a point where
//! it is.

use bt_core::{Alignment, Map, MapGeometry, MapObject, ObjectId, Shape, rotated};
use bt_utils::{DRect, IRect};
use glam::DVec2;

/// The objects of every object layer that would be outside the cells `area`, measured on the
/// map as it is.
pub(crate) fn objects_outside(map: &Map, area: IRect) -> Vec<ObjectId> {
    let geometry = map.geometry();
    let visible = DRect::from(geometry.bounding_rect(area));
    let mut outside = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            outside.extend(
                objects
                    .objects
                    .iter()
                    .filter(|object| !touches(visible, screen_bounds(map, &geometry, object)))
                    .map(|object| object.id),
            );
        }
    });
    outside
}

/// Whether two rectangles overlap or touch, an empty one included (Tiled's `intersects`).
fn touches(a: DRect, b: DRect) -> bool {
    a.max.x >= b.min.x && a.max.y >= b.min.y && a.min.x <= b.max.x && a.min.y <= b.max.y
}

/// The object's rectangle on screen, turned with it around its position.
fn screen_bounds(map: &Map, geometry: &MapGeometry, object: &MapObject) -> DRect {
    let origin = geometry.pixel_to_screen(object.pos);
    let upright = match (&object.tile, &object.shape) {
        (Some(cell), _) => {
            let tileset = map.tileset_of(*cell);
            let alignment = geometry.object_alignment(tileset);
            let rect = geometry.upright_object_rect(object.pos, object.size, alignment);
            // The tile offset, scaled as the image is.
            let shift = tileset.map_or(DVec2::ZERO, |tileset| {
                let tile_size = tileset.tile_pixel_size(cell.tile()).as_dvec2();
                let offset = tileset.tile_offset.as_dvec2();
                if tile_size.cmpgt(DVec2::ZERO).all() {
                    offset * object.size / tile_size
                } else {
                    offset
                }
            });
            rect.translated(shift)
        }
        (None, Shape::Text(_)) => {
            geometry.upright_object_rect(object.pos, object.size, Alignment::TopLeft)
        }
        (None, Shape::Point) => DRect::from_min_size(origin, DVec2::ZERO),
        (None, Shape::Polygon(points) | Shape::Polyline(points)) if !points.is_empty() => {
            projected(geometry, points.iter().map(|p| object.pos + *p), origin)
        }
        (None, _) => {
            let corners = [
                DVec2::ZERO,
                DVec2::new(object.size.x, 0.0),
                object.size,
                DVec2::new(0.0, object.size.y),
            ];
            projected(geometry, corners.map(|c| object.pos + c), origin)
        }
    };
    turned(upright, origin, object.rotation)
}

/// The screen rectangle of pixel positions; `at` when there are none.
fn projected(geometry: &MapGeometry, pixels: impl IntoIterator<Item = DVec2>, at: DVec2) -> DRect {
    DRect::from_points(pixels.into_iter().map(|p| geometry.pixel_to_screen(p)))
        .unwrap_or(DRect { min: at, max: at })
}

/// The rectangle holding `rect` turned `degrees` clockwise around `origin` (Qt's
/// `QTransform::rotate` and `mapRect`).
fn turned(rect: DRect, origin: DVec2, degrees: f64) -> DRect {
    if degrees == 0.0 {
        return rect;
    }
    let corners = [
        rect.min,
        DVec2::new(rect.max.x, rect.min.y),
        rect.max,
        DVec2::new(rect.min.x, rect.max.y),
    ];
    let turned = corners.map(|corner| origin + rotated(corner - origin, degrees));
    DRect::from_points(turned).unwrap_or(rect)
}

#[cfg(test)]
mod tests {
    use bt_core::{Orientation, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::testutil::rect_object;

    fn close(a: DRect, b: DRect) -> bool {
        a.min.distance(b.min) < 1e-9 && a.max.distance(b.max) < 1e-9
    }

    #[test]
    fn objects_are_bounded_as_tileds_renderers_draw_them() {
        let atlas = Atlas::builder()
            .tile_offset(glam::IVec2::new(2, -4))
            .build_embedded()
            .unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tileset(atlas.tileset)
            .build()
            .unwrap();
        let geometry = map.geometry();
        let rect = |x, y, w, h| DRect::from_min_size(DVec2::new(x, y), DVec2::new(w, h));
        assert!(close(
            screen_bounds(&map, &geometry, &rect_object(4.0, 8.0, 10.0, 6.0)),
            rect(4.0, 8.0, 10.0, 6.0)
        ));
        // A tile object stands on its position, bottom-left, shifted by the tile offset scaled
        // to its size (twice the tile's here).
        let tile = MapObject {
            tile: Some(bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(1))),
            ..rect_object(10.0, 40.0, 32.0, 32.0)
        };
        assert!(close(
            screen_bounds(&map, &geometry, &tile),
            rect(14.0, 0.0, 32.0, 32.0)
        ));
        // Turned a quarter clockwise around its top-left corner, a rectangle hangs to the left.
        let turned = MapObject {
            rotation: 90.0,
            ..rect_object(20.0, 20.0, 10.0, 4.0)
        };
        assert!(close(
            screen_bounds(&map, &geometry, &turned),
            rect(16.0, 20.0, 4.0, 10.0)
        ));
        let polygon = MapObject {
            shape: Shape::Polygon(vec![
                DVec2::ZERO,
                DVec2::new(-8.0, 3.0),
                DVec2::new(5.0, 9.0),
            ]),
            ..rect_object(30.0, 30.0, 0.0, 0.0)
        };
        assert!(close(
            screen_bounds(&map, &geometry, &polygon),
            rect(22.0, 30.0, 13.0, 9.0)
        ));
    }

    #[test]
    fn isometric_shapes_are_projected_and_tile_objects_stand_upright() {
        let atlas = Atlas::builder().tile_size(32, 16).build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(4))
            .orientation(Orientation::Isometric)
            .tileset(atlas.tileset)
            .build()
            .unwrap();
        let geometry = map.geometry();
        // The square of cell (0, 0) in pixels is its diamond on screen.
        let square = screen_bounds(&map, &geometry, &rect_object(0.0, 0.0, 16.0, 16.0));
        assert!(close(
            square,
            DRect::from_min_size(DVec2::new(48.0, 0.0), DVec2::new(32.0, 16.0))
        ));
        // A tile object stands bottom-centre on its projected position.
        let tile = MapObject {
            tile: Some(bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            ..rect_object(16.0, 16.0, 32.0, 16.0)
        };
        assert!(close(
            screen_bounds(&map, &geometry, &tile),
            DRect::from_min_size(DVec2::new(48.0, 0.0), DVec2::new(32.0, 16.0))
        ));
    }

    #[test]
    fn objects_touching_the_area_are_inside() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .object_layer(
                "things",
                vec![
                    rect_object(32.0, 0.0, 8.0, 8.0),
                    rect_object(33.0, 0.0, 8.0, 8.0),
                    MapObject {
                        rotation: 180.0,
                        ..rect_object(40.0, 20.0, 10.0, 4.0)
                    },
                ],
            )
            .build()
            .unwrap();
        let area = IRect {
            x: 0,
            y: 0,
            w: 2,
            h: 2,
        };
        // The first touches the area's right edge; the third is turned back into it.
        assert_eq!(objects_outside(&map, area), [ObjectId(2)]);
    }
}
