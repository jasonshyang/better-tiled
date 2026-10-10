//! Converting objects: rectangles to polygons, and tile objects back to their tile's size
//! (Tiled's `AbstractObjectTool::convertRectanglesToPolygons` and `resetTileSize`,
//! `reference/tiled/src/tiled/abstractobjecttool.cpp`).

use bt_core::{Map, MapObject, ObjectId, ObjectLayer, ObjectOverrides, Shape};
use glam::DVec2;

use crate::error::EditError;
use crate::report::ObjectsReport;

use super::edit::rewrite_objects_in;
use super::find::{objects_in, on_layers_holding};
use super::space::ObjectSpace;

/// Convert to Polygon: the rectangles among the objects of `map` (not tile objects) become
/// polygons through their corners, keeping their size as Tiled does; other objects are left
/// alone. Template instances mark their shape as their own. Reports the objects converted, in
/// the order given.
pub fn rect_to_polygon(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, _, held| {
        rect_to_polygon_in(objects, held)
    })
}

/// Convert to Polygon on a list.
pub fn rect_to_polygon_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    rewrite_objects_in(objects, ids, |object| {
        let mut converted = object.clone();
        if object.tile.is_none() && object.shape == Shape::Rectangle {
            let size = object.size;
            converted.shape = Shape::Polygon(vec![
                DVec2::ZERO,
                DVec2::new(size.x, 0.0),
                size,
                DVec2::new(0.0, size.y),
            ]);
            if converted.template.is_some() {
                converted.overrides |= ObjectOverrides::SHAPE;
            }
        }
        converted
    })
}

/// Reset Tile Size: the tile objects among the objects of `map` take their tile's size; other
/// objects are left alone. Template instances mark their size as their own. A tile the map's
/// tilesets do not have is an error. Reports the objects resized, in the order given.
pub fn reset_tile_object_size(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, space, held| {
        reset_tile_object_size_in(objects, space, held)
    })
}

/// Reset Tile Size on a list, its tiles from `space`'s tilesets.
pub fn reset_tile_object_size_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    for object in objects_in(objects, ids)? {
        tile_size_of(space, object)?;
    }
    rewrite_objects_in(objects, ids, |object| {
        let mut resized = object.clone();
        if let Ok(Some(size)) = tile_size_of(space, object)
            && size != object.size
        {
            resized.size = size;
            if resized.template.is_some() {
                resized.overrides |= ObjectOverrides::SIZE;
            }
        }
        resized
    })
}

/// The size of a tile object's tile; `None` for other objects.
fn tile_size_of(space: &ObjectSpace, object: &MapObject) -> Result<Option<DVec2>, EditError> {
    let Some(cell) = object.tile else {
        return Ok(None);
    };
    let slot = cell.slot().ok_or(EditError::EmptyCell)?;
    let tileset = space
        .tileset_of(cell)
        .ok_or(EditError::NoSuchTileset(slot))?;
    if !tileset.has_tile(cell.tile()) {
        return Err(EditError::NoSuchTile {
            slot,
            tile: cell.tile(),
        });
    }
    Ok(Some(tileset.tile_pixel_size(cell.tile()).as_dvec2()))
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, FileRef, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_valid, rect_object};

    fn tile(slot: usize, id: u32) -> Cell {
        Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id))
    }

    fn sample() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .tileset(
                Atlas::builder()
                    .tile_size(16, 24)
                    .build_embedded()
                    .unwrap()
                    .tileset,
            )
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 20.0),
                    MapObject {
                        shape: Shape::Ellipse,
                        ..rect_object(0.0, 0.0, 10.0, 20.0)
                    },
                    MapObject {
                        tile: Some(tile(0, 2)),
                        ..rect_object(0.0, 0.0, 32.0, 32.0)
                    },
                    MapObject {
                        template: Some(FileRef::new("box.tx", "/box.tx")),
                        ..rect_object(0.0, 0.0, 4.0, 4.0)
                    },
                ],
            )
            .build()
            .unwrap()
    }

    #[test]
    fn rectangles_become_polygons_through_their_corners() {
        let mut map = sample();
        let all = [ObjectId(4), ObjectId(1), ObjectId(2), ObjectId(3)];
        let report = rect_to_polygon(&mut map, &all).unwrap();
        assert_eq!(report.changed, [ObjectId(4), ObjectId(1)]);
        let converted = find_object(&map, ObjectId(1)).unwrap().object;
        assert_eq!(
            converted.shape,
            Shape::Polygon(vec![
                DVec2::ZERO,
                DVec2::new(10.0, 0.0),
                DVec2::new(10.0, 20.0),
                DVec2::new(0.0, 20.0)
            ])
        );
        assert_eq!(
            converted.size,
            DVec2::new(10.0, 20.0),
            "Tiled keeps the size"
        );
        assert_eq!(
            find_object(&map, ObjectId(4)).unwrap().object.overrides,
            ObjectOverrides::SHAPE
        );
        assert!(rect_to_polygon(&mut map, &all).unwrap().changed.is_empty());
        assert_valid(&map);
    }

    #[test]
    fn tile_objects_take_their_tiles_size_again() {
        let mut map = sample();
        let report = reset_tile_object_size(&mut map, &[ObjectId(1), ObjectId(3)]).unwrap();
        assert_eq!(report.changed, [ObjectId(3)]);
        assert_eq!(
            find_object(&map, ObjectId(3)).unwrap().object.size,
            DVec2::new(16.0, 24.0)
        );
        assert!(
            reset_tile_object_size(&mut map, &[ObjectId(3)])
                .unwrap()
                .changed
                .is_empty()
        );
        crate::objects::find::object_mut(&mut map, ObjectId(3))
            .unwrap()
            .tile = Some(tile(1, 0));
        assert_eq!(
            reset_tile_object_size(&mut map, &[ObjectId(3)]),
            Err(EditError::NoSuchTileset(TilesetSlot::new(1).unwrap()))
        );
        crate::objects::find::object_mut(&mut map, ObjectId(3))
            .unwrap()
            .tile = Some(tile(0, 99));
        assert_eq!(
            reset_tile_object_size(&mut map, &[ObjectId(3)]),
            Err(EditError::NoSuchTile {
                slot: TilesetSlot::new(0).unwrap(),
                tile: TileId(99)
            })
        );
    }
}
