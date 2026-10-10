//! The rectangles objects cover: a shape's, or a tile object's image placed by its alignment and
//! tile offset, turned with the object (Tiled's `MapRenderer::boundingRect` and
//! `MapObject::screenBounds`, `reference/tiled/src/libtiled/orthogonalrenderer.cpp`,
//! `isometricrenderer.cpp`, `mapobject.cpp`, without the room they leave for line widths).

use bt_core::{Map, MapObject, ObjectId, ObjectLayer};
use bt_utils::DRect;

use crate::error::EditError;

use super::find::{objects_by_id, objects_in};
use super::space::ObjectSpace;

/// The pixel rectangle an object covers as drawn: its shape, or a tile object's image placed by
/// its alignment (the tileset's, else the map's) and the tileset's tile offset, turned around
/// its position. On isometric maps it holds what the screen shows, projected back into pixel
/// space. Points, and rectangles, ellipses and capsules without a size, have bounds of no size
/// at their position; hit tests give the latter Tiled's marker.
pub fn object_bounds(space: &ObjectSpace, object: &MapObject) -> DRect {
    space.pixel_outline(object).bounds()
}

/// The rectangle holding the bounds of the objects of `map` with `ids`; `None` for no ids.
pub fn objects_bounds(map: &Map, ids: &[ObjectId]) -> Result<Option<DRect>, EditError> {
    let space = ObjectSpace::of_map(map);
    Ok(union_of(
        objects_by_id(map, ids)?
            .into_iter()
            .map(|o| object_bounds(&space, o)),
    ))
}

/// The rectangle holding the bounds of the objects of a list with `ids`; `None` for no ids.
pub fn objects_bounds_in(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<Option<DRect>, EditError> {
    Ok(union_of(
        objects_in(objects, ids)?
            .into_iter()
            .map(|o| object_bounds(space, o)),
    ))
}

/// The screen rectangle an object covers as drawn.
pub(crate) fn drawn_bounds(space: &ObjectSpace, object: &MapObject) -> DRect {
    space.drawn_outline(object).bounds()
}

/// The smallest rectangle holding every rectangle, those of no size (points) included;
/// `None` for none.
pub(crate) fn union_of(rects: impl IntoIterator<Item = DRect>) -> Option<DRect> {
    rects.into_iter().reduce(|a, b| DRect {
        min: a.min.min(b.min),
        max: a.max.max(b.max),
    })
}

#[cfg(test)]
mod tests {
    use bt_core::{Alignment, Cell, Orientation, Shape, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::{DVec2, IVec2, UVec2};

    use super::*;
    use crate::testutil::{assert_near, rect_object};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> DRect {
        DRect::from_min_size(v(x, y), v(w, h))
    }

    fn assert_rect_near(actual: DRect, expected: DRect) {
        assert_near(actual.min, expected.min);
        assert_near(actual.max, expected.max);
    }

    fn tile_object(x: f64, y: f64, w: f64, h: f64) -> MapObject {
        MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(1))),
            ..rect_object(x, y, w, h)
        }
    }

    fn map_with(atlas: Atlas, orientation: Orientation) -> Map {
        MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
            .orientation(orientation)
            .tileset(atlas.tileset)
            .build()
            .unwrap()
    }

    #[test]
    fn shapes_cover_their_box_turned_around_their_position() {
        let map = map_with(
            Atlas::builder().build_embedded().unwrap(),
            Orientation::Orthogonal,
        );
        let space = ObjectSpace::of_map(&map);
        let box_ = rect_object(10.0, 20.0, 30.0, 10.0);
        assert_eq!(object_bounds(&space, &box_), rect(10.0, 20.0, 30.0, 10.0));
        let turned = MapObject {
            rotation: 90.0,
            ..box_.clone()
        };
        assert_eq!(object_bounds(&space, &turned), rect(0.0, 20.0, 10.0, 30.0));
        let ellipse = MapObject {
            shape: Shape::Ellipse,
            rotation: 45.0,
            size: v(20.0, 20.0),
            ..box_.clone()
        };
        let circle = object_bounds(&space, &ellipse);
        assert_rect_near(
            circle,
            DRect::from_min_size(
                // The centre, (10, 10) from the position, turns to straight below it.
                v(10.0, 20.0) + v(0.0, 10.0 * 2f64.sqrt()) - v(10.0, 10.0),
                v(20.0, 20.0),
            ),
        );
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(10.0, -5.0), v(4.0, 8.0)]),
            ..box_.clone()
        };
        assert_eq!(
            object_bounds(&space, &polygon),
            rect(10.0, 15.0, 10.0, 13.0)
        );
        let point = MapObject {
            shape: Shape::Point,
            ..box_.clone()
        };
        assert_eq!(object_bounds(&space, &point), rect(10.0, 20.0, 0.0, 0.0));
        let marker = MapObject {
            size: DVec2::ZERO,
            ..box_
        };
        assert_eq!(
            object_bounds(&space, &marker),
            rect(10.0, 20.0, 0.0, 0.0),
            "no size, no marker"
        );
    }

    #[test]
    fn tile_objects_cover_their_image_by_alignment_and_offset() {
        let atlas = Atlas::builder()
            .tile_size(16, 24)
            .tile_offset(IVec2::new(4, -2))
            .build_embedded()
            .unwrap();
        let map = map_with(atlas.clone(), Orientation::Orthogonal);
        let space = ObjectSpace::of_map(&map);
        // Bottom-left: the image hangs up from the position, moved by the offset; at twice the
        // tile's size the offset doubles.
        let tile = tile_object(100.0, 100.0, 32.0, 48.0);
        assert_eq!(object_bounds(&space, &tile), rect(108.0, 48.0, 32.0, 48.0));
        let sizeless = tile_object(100.0, 100.0, 0.0, 0.0);
        assert_eq!(
            object_bounds(&space, &sizeless),
            rect(104.0, 74.0, 16.0, 24.0),
            "the tile's own size when the object has none"
        );
        let turned = MapObject {
            rotation: -90.0,
            ..tile.clone()
        };
        // (8, -52)..(40, -4) around the position, turned a quarter to the left.
        assert_eq!(object_bounds(&space, &turned), rect(48.0, 60.0, 48.0, 32.0));
        let mut centered = atlas.tileset.as_ref().clone();
        centered.object_alignment = Alignment::Center;
        let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
            .tileset(std::sync::Arc::new(centered))
            .build()
            .unwrap();
        let space = ObjectSpace::of_map(&map);
        assert_eq!(object_bounds(&space, &tile), rect(92.0, 72.0, 32.0, 48.0));
    }

    #[test]
    fn isometric_tile_objects_stand_upright_at_their_projected_position() {
        let atlas = Atlas::builder().tile_size(32, 32).build_embedded().unwrap();
        let map = map_with(atlas, Orientation::Isometric);
        let space = ObjectSpace::of_map(&map);
        let geometry = map.geometry();
        let tile = tile_object(32.0, 32.0, 32.0, 32.0);
        let foot = geometry.pixel_to_screen(tile.pos);
        // Bottom-centre on isometric maps.
        assert_eq!(
            drawn_bounds(&space, &tile),
            DRect::from_min_size(foot - v(16.0, 32.0), v(32.0, 32.0))
        );
        // In pixel space the upright image is a diamond around the position.
        let bounds = object_bounds(&space, &tile);
        let corners = [v(-16.0, -32.0), v(16.0, -32.0), v(16.0, 0.0), v(-16.0, 0.0)]
            .map(|c| geometry.screen_to_pixel(foot + c));
        assert_rect_near(bounds, DRect::from_points(corners).unwrap());
    }

    #[test]
    fn several_objects_join_their_bounds() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(50.0, -5.0, 0.0, 0.0)
                    },
                ],
            )
            .build()
            .unwrap();
        assert_eq!(
            objects_bounds(&map, &[bt_core::ObjectId(1), bt_core::ObjectId(2)]),
            Ok(Some(rect(0.0, -5.0, 50.0, 15.0))),
            "a point counts though it has no size"
        );
        assert_eq!(objects_bounds(&map, &[]), Ok(None));
        assert_eq!(
            objects_bounds(&map, &[bt_core::ObjectId(3)]),
            Err(EditError::NoSuchObject(bt_core::ObjectId(3)))
        );
    }
}
