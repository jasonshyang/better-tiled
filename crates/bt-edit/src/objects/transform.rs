//! Flipping and turning objects: Tiled's `MapObject::flip` with `flipInScreenCoordinates` and
//! `flipInPixelCoordinates` (`reference/tiled/src/libtiled/mapobject.cpp`, with
//! `alignmentOffset` and `flipAlignment` from `libtiled/tiled.cpp`), `FlipMapObjects`
//! (`tiled/flipmapobjects.cpp`), `MapDocument::flipSelectedObjects` and
//! `rotateSelectedObjects` (`tiled/mapdocument.cpp`), and the rotate handles of
//! `ObjectSelectionTool::updateRotatingItems` (`tiled/objectselectiontool.cpp`).

use bt_core::{
    Alignment, Map, MapGeometry, MapObject, ObjectId, ObjectLayer, ObjectOverrides, Shape, rotated,
};
use glam::DVec2;

use crate::axis::Axis;
use crate::error::EditError;
use crate::report::ObjectsReport;

use super::bounds::{drawn_bounds, union_of};
use super::edit::rewrite_objects_in;
use super::find::{objects_by_id, objects_in, on_layers_holding};
use super::space::ObjectSpace;

/// What objects turn around.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Pivot {
    /// One point for all, in pixel space: positions turn around it too, and rotations are kept
    /// within [0, 360) (Select Objects' rotate handle).
    Point(DVec2),
    /// Each object's own position: only rotations change, kept within [-180, 180] (Tiled's
    /// Rotate Left and Rotate Right).
    OwnPosition,
}

/// Flips objects of `map` along `axis` around `origin`, as Tiled's `MapObject::flip` does:
/// shapes are mirrored in pixel space (positions, and a polygon's or polyline's points), tile
/// and text objects on screen so their alignment point stays consistent, tile objects' images
/// flipped too; rotations are negated. `origin` is pixel space and the same for every layer,
/// whatever its offset, as in Tiled. Template instances mark their tile, rotation and shape as
/// their own. Reports the objects that changed, in the order given.
pub fn flip_objects(
    map: &mut Map,
    ids: &[ObjectId],
    axis: Axis,
    origin: DVec2,
) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, space, held| {
        flip_objects_in(objects, space, held, axis, origin)
    })
}

/// Flips objects of a list, as [`flip_objects`] does.
pub fn flip_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    axis: Axis,
    origin: DVec2,
) -> Result<ObjectsReport, EditError> {
    rewrite_objects_in(objects, ids, |object| flipped(space, object, axis, origin))
}

/// The point Tiled flips a selection of `map` around: the centre of the rectangle holding what
/// the objects cover on screen, in pixel space. `None` for no ids.
pub fn flip_origin(map: &Map, ids: &[ObjectId]) -> Result<Option<DVec2>, EditError> {
    let space = ObjectSpace::of_map(map);
    Ok(centre_of(&space, objects_by_id(map, ids)?))
}

/// The point Tiled flips a selection of a list around.
pub fn flip_origin_in(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<Option<DVec2>, EditError> {
    Ok(centre_of(space, objects_in(objects, ids)?))
}

/// The centre of the screen rectangle holding `objects`, in pixel space.
fn centre_of(space: &ObjectSpace, objects: Vec<&MapObject>) -> Option<DVec2> {
    let bounds = union_of(objects.into_iter().map(|o| drawn_bounds(space, o)))?;
    Some(
        space
            .geometry()
            .screen_to_pixel((bounds.min + bounds.max) * 0.5),
    )
}

/// `object` flipped along `axis` around `origin`.
fn flipped(space: &ObjectSpace, object: &MapObject, axis: Axis, origin: DVec2) -> MapObject {
    let mut flipped = object.clone();
    let alignment = space.alignment_of(object);
    if object.tile.is_some() || matches!(object.shape, Shape::Text(_)) {
        flip_upright(space, &mut flipped, alignment, axis, origin);
    } else {
        flip_shape(&mut flipped, alignment, axis, origin);
    }
    flipped.rotation = if object.rotation == 0.0 {
        0.0
    } else {
        -object.rotation
    };
    if flipped.template.is_some() {
        // Tiled's `propertiesChangedByFlip`; the position is always an instance's own.
        flipped.overrides |=
            ObjectOverrides::TILE | ObjectOverrides::ROTATION | ObjectOverrides::SHAPE;
    }
    flipped
}

/// Tiled's `flipInScreenCoordinates`: the box's corner opposite the alignment point is
/// mirrored on screen, and the object placed so that corner is where its flipped alignment
/// puts it; a tile's image is mirrored with it.
fn flip_upright(
    space: &ObjectSpace,
    object: &mut MapObject,
    alignment: Alignment,
    axis: Axis,
    origin: DVec2,
) {
    let geometry = space.geometry();
    let size = space
        .tile_box(object)
        .map_or(object.size, |image| image.size());
    let screen_origin = geometry.pixel_to_screen(origin);
    let top_left = geometry.pixel_to_screen(object.pos)
        + rotated(-alignment.origin_fraction() * size, object.rotation);
    let mut flipped_offset = -flip_alignment(alignment, axis).origin_fraction() * size;
    let mut new_top_left = top_left;
    match axis {
        Axis::Horizontal => {
            new_top_left.x += 2.0 * (screen_origin.x - top_left.x);
            flipped_offset.x = -flipped_offset.x;
        }
        Axis::Vertical => {
            new_top_left.y += 2.0 * (screen_origin.y - top_left.y);
            flipped_offset.y = -flipped_offset.y;
        }
    }
    if let Some(cell) = object.tile {
        let flip = match axis {
            Axis::Horizontal => cell.flip().flipped_horizontally(),
            Axis::Vertical => cell.flip().flipped_vertically(),
        };
        object.tile = Some(cell.with_flip(flip));
    }
    let new_screen = new_top_left - rotated(flipped_offset, -object.rotation);
    object.pos = geometry.screen_to_pixel(new_screen);
}

/// Tiled's `flipInPixelCoordinates`: a polygon's position and points are mirrored; any other
/// shape's position becomes the mirror of the corner its flipped alignment names (turned with
/// it). Tiled leaves turned shapes on isometric maps to this pixel-space rule too.
fn flip_shape(object: &mut MapObject, alignment: Alignment, axis: Axis, origin: DVec2) {
    match &mut object.shape {
        Shape::Polygon(points) | Shape::Polyline(points) if !points.is_empty() => {
            for point in points.iter_mut() {
                *point = axis.mirrored(*point, DVec2::ZERO);
            }
            object.pos = axis.mirrored(object.pos, origin);
        }
        _ => {
            let corner = flip_alignment(alignment, axis).origin_fraction() * object.size;
            object.pos = axis.mirrored(object.pos + rotated(corner, object.rotation), origin);
        }
    }
}

/// Tiled's `flipAlignment`: the alignment of the mirrored box.
fn flip_alignment(alignment: Alignment, axis: Axis) -> Alignment {
    match (axis, alignment) {
        (Axis::Horizontal, Alignment::TopLeft) => Alignment::TopRight,
        (Axis::Horizontal, Alignment::TopRight) => Alignment::TopLeft,
        (Axis::Horizontal, Alignment::Left) => Alignment::Right,
        (Axis::Horizontal, Alignment::Right) => Alignment::Left,
        (Axis::Horizontal, Alignment::BottomLeft) => Alignment::BottomRight,
        (Axis::Horizontal, Alignment::BottomRight) => Alignment::BottomLeft,
        (Axis::Vertical, Alignment::TopLeft) => Alignment::BottomLeft,
        (Axis::Vertical, Alignment::Top) => Alignment::Bottom,
        (Axis::Vertical, Alignment::TopRight) => Alignment::BottomRight,
        (Axis::Vertical, Alignment::BottomLeft) => Alignment::TopLeft,
        (Axis::Vertical, Alignment::Bottom) => Alignment::Top,
        (Axis::Vertical, Alignment::BottomRight) => Alignment::TopRight,
        (_, unchanged) => unchanged,
    }
}

/// Turns objects of `map` clockwise by `degrees` around `pivot`. Around a point (pixel space,
/// the same for every layer), positions turn on screen, as Tiled's rotate handle turns them,
/// and rotations are kept within [0, 360); around their own positions, rotations are kept
/// within [-180, 180]. Points only move. Template instances whose rotation changed mark it as
/// their own. Reports the objects that changed, in the order given.
pub fn rotate_objects(
    map: &mut Map,
    ids: &[ObjectId],
    degrees: f64,
    pivot: Pivot,
) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, space, held| {
        rotate_objects_in(objects, space, held, degrees, pivot)
    })
}

/// Turns objects of a list, as [`rotate_objects`] does.
pub fn rotate_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    degrees: f64,
    pivot: Pivot,
) -> Result<ObjectsReport, EditError> {
    rewrite_objects_in(objects, ids, |object| turned(space, object, degrees, pivot))
}

/// `object` turned by `degrees` around `pivot`.
fn turned(space: &ObjectSpace, object: &MapObject, degrees: f64, pivot: Pivot) -> MapObject {
    let mut turned = object.clone();
    // Tiled's `MapObject::canRotate`.
    let rotates = !matches!(object.shape, Shape::Point);
    match pivot {
        Pivot::Point(origin) => {
            if object.pos != origin {
                let geometry = space.geometry();
                let centre = geometry.pixel_to_screen(origin);
                let arm = geometry.pixel_to_screen(object.pos) - centre;
                turned.pos = geometry.screen_to_pixel(centre + rotated(arm, degrees));
            }
            if rotates {
                turned.rotation = within_full_turn(object.rotation + degrees);
            }
        }
        Pivot::OwnPosition => {
            if rotates {
                turned.rotation = within_half_turns(object.rotation + degrees);
            }
        }
    }
    if turned.template.is_some() && turned.rotation != object.rotation {
        turned.overrides |= ObjectOverrides::ROTATION;
    }
    turned
}

/// The steps a rotate handle snaps to, in degrees (Tiled's 15°).
const ROTATION_SNAP: f64 = 15.0;

/// How far a drag of a rotate handle turns objects around `centre`: the angle from the direction
/// of where the drag started (`from`) to the direction of where it is now (`to`), seen from the
/// centre on screen, clockwise in degrees from -180 to 180, all three points pixel space
/// (Tiled's `updateRotatingItems`). With `snap` the angle is rounded to whole steps of 15°
/// (Tiled's Ctrl). The objects then turn by it with [`rotate_objects`] around
/// [`Pivot::Point`] of the centre.
pub fn drag_angle(
    geometry: &MapGeometry,
    centre: DVec2,
    from: DVec2,
    to: DVec2,
    snap: bool,
) -> f64 {
    let centre = geometry.pixel_to_screen(centre);
    let start = centre - geometry.pixel_to_screen(from);
    let now = centre - geometry.pixel_to_screen(to);
    let turn = (now.y.atan2(now.x) - start.y.atan2(start.x)).to_degrees();
    let degrees = (turn + 180.0).rem_euclid(360.0) - 180.0;
    if snap {
        ((degrees + ROTATION_SNAP / 2.0) / ROTATION_SNAP).floor() * ROTATION_SNAP
    } else {
        degrees
    }
}

/// A rotation brought within [0, 360) (Tiled's `normalizeRotation`).
fn within_full_turn(degrees: f64) -> f64 {
    let turned = degrees.rem_euclid(360.0);
    // A tiny negative angle rounds up to a full turn.
    if turned >= 360.0 || turned == 0.0 {
        0.0
    } else {
        turned
    }
}

/// A rotation brought within [-180, 180] by whole turns, as Tiled's Rotate Left and Right keep
/// it; ±180 stays as it is.
fn within_half_turns(degrees: f64) -> f64 {
    let turned = if degrees > 180.0 {
        degrees - 360.0 * ((degrees - 180.0) / 360.0).ceil()
    } else if degrees < -180.0 {
        degrees + 360.0 * ((-180.0 - degrees) / 360.0).ceil()
    } else {
        degrees
    };
    if turned == 0.0 { 0.0 } else { turned }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, FileRef, Flip, Orientation, TextData, TileId, TilesetSlot};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::rng::Rng;
    use glam::UVec2;

    use super::*;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_near, assert_valid, random_object, rect_object};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    /// A map of 32 × 16 cells in `orientation`, with the fixture atlas of 16-pixel tiles whose
    /// objects align as given.
    fn space_map(orientation: Orientation, alignment: Alignment) -> Map {
        let mut tileset = Atlas::builder()
            .build_embedded()
            .unwrap()
            .tileset
            .as_ref()
            .clone();
        tileset.object_alignment = alignment;
        MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
            .orientation(orientation)
            .tileset(Arc::new(tileset))
            .build()
            .unwrap()
    }

    fn flip_one(map: &Map, object: &MapObject, axis: Axis, origin: DVec2) -> MapObject {
        flipped(&ObjectSpace::of_map(map), object, axis, origin)
    }

    #[test]
    fn shapes_flip_as_tileds_do() {
        let map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        let box_ = rect_object(10.0, 20.0, 30.0, 10.0);
        let flipped = flip_one(&map, &box_, Axis::Horizontal, v(50.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(60.0, 20.0), 0.0));
        // Turned a quarter clockwise it hangs down from (10, 20); its mirror hangs from (90, 50)
        // turned a quarter back.
        let turned = MapObject {
            rotation: 90.0,
            ..box_.clone()
        };
        let flipped = flip_one(&map, &turned, Axis::Horizontal, v(50.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(90.0, 50.0), -90.0));
        let flipped = flip_one(&map, &turned, Axis::Vertical, v(0.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(0.0, -20.0), -90.0));
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, 5.0)]),
            rotation: 30.0,
            ..rect_object(10.0, 10.0, 0.0, 0.0)
        };
        let flipped = flip_one(&map, &polygon, Axis::Vertical, v(0.0, 0.0));
        assert_eq!(flipped.pos, v(10.0, -10.0));
        assert_eq!(
            flipped.shape,
            Shape::Polygon(vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, -5.0)])
        );
        assert_eq!(flipped.rotation, -30.0);
        let point = MapObject {
            shape: Shape::Point,
            ..rect_object(5.0, 5.0, 0.0, 0.0)
        };
        let flipped = flip_one(&map, &point, Axis::Horizontal, v(10.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(15.0, 5.0), 0.0));
        assert!(flipped.rotation.is_sign_positive(), "no negative zero");
    }

    #[test]
    fn tile_objects_keep_their_alignment_point_consistent() {
        let map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        // Bottom-left: around its own middle the object stays and its image flips.
        let object = MapObject {
            tile: Some(tile(3)),
            ..rect_object(0.0, 16.0, 16.0, 16.0)
        };
        let flipped = flip_one(&map, &object, Axis::Horizontal, v(8.0, 0.0));
        assert_eq!(flipped.pos, v(0.0, 16.0));
        assert_eq!(flipped.tile, Some(tile(3).with_flip(Flip::H)));
        let flipped = flip_one(&map, &object, Axis::Horizontal, v(0.0, 0.0));
        assert_eq!(flipped.pos, v(-16.0, 16.0));
        // Turned a quarter, its image lies right of and below the position; the mirror around
        // x = 50 lies at 84..100, held from (100, 32) turned back.
        let turned = MapObject {
            rotation: 90.0,
            ..object.clone()
        };
        let flipped = flip_one(&map, &turned, Axis::Horizontal, v(50.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(100.0, 32.0), -90.0));
        let flipped = flip_one(&map, &object, Axis::Vertical, v(0.0, 100.0));
        assert_eq!(flipped.pos, v(0.0, 200.0));
        assert_eq!(flipped.tile, Some(tile(3).with_flip(Flip::V)));
        // Centred, turned: the centre mirrors to the mirrored centre.
        let centred = space_map(Orientation::Orthogonal, Alignment::Center);
        let wide = MapObject {
            tile: Some(tile(0)),
            rotation: 90.0,
            ..rect_object(100.0, 100.0, 20.0, 10.0)
        };
        let flipped = flip_one(&centred, &wide, Axis::Horizontal, v(0.0, 0.0));
        assert_eq!((flipped.pos, flipped.rotation), (v(-100.0, 100.0), -90.0));
        let top_right = space_map(Orientation::Orthogonal, Alignment::TopRight);
        let flipped = flip_one(&top_right, &object, Axis::Horizontal, v(0.0, 0.0));
        assert_eq!(
            flipped.pos,
            v(16.0, 16.0),
            "its box was the 16 pixels left of the position"
        );
    }

    #[test]
    fn upright_objects_mirror_what_the_screen_shows() {
        let fuzz = Fuzz::from_env(0xF11F, 100);
        for orientation in [Orientation::Orthogonal, Orientation::Isometric] {
            for alignment in [
                Alignment::Unspecified,
                Alignment::TopLeft,
                Alignment::Right,
                Alignment::BottomRight,
            ] {
                let map = space_map(orientation, alignment);
                let space = ObjectSpace::of_map(&map);
                for case in 0..fuzz.cases {
                    let mut rng = Rng::new(fuzz.case_seed(case));
                    let mut object = random_object(&mut rng);
                    if object.tile.is_none() {
                        object.shape = Shape::Text(TextData::default());
                    }
                    let origin = v(rng.next_f64() * 200.0, rng.next_f64() * 200.0);
                    let axis = if rng.below(2) == Some(0) {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    };
                    let screen_origin = map.geometry().pixel_to_screen(origin);
                    let mirrored: Vec<DVec2> = space
                        .drawn_outline(&object)
                        .points()
                        .iter()
                        .map(|&p| axis.mirrored(p, screen_origin))
                        .collect();
                    let flipped = flipped(&space, &object, axis, origin);
                    let drawn = space.drawn_outline(&flipped);
                    for p in drawn.points() {
                        assert!(
                            mirrored.iter().any(|m| (*m - *p).length() < 1e-6),
                            "case {case} ({}) on {orientation:?}, {alignment:?}: {p} of {flipped:?} is no corner of the mirrored {object:?}",
                            fuzz.reproduce()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shapes_mirror_in_pixel_space() {
        let fuzz = Fuzz::from_env(0x005A_F11F, 200);
        let map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        let space = ObjectSpace::of_map(&map);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let mut object = random_object(&mut rng);
            object.tile = None;
            if matches!(object.shape, Shape::Text(_)) {
                object.shape = Shape::Rectangle;
            }
            let origin = v(rng.next_f64() * 200.0, rng.next_f64() * 200.0);
            for axis in [Axis::Horizontal, Axis::Vertical] {
                let mirrored: Vec<DVec2> = space
                    .pixel_outline(&object)
                    .points()
                    .iter()
                    .map(|&p| axis.mirrored(p, origin))
                    .collect();
                let flipped = flipped(&space, &object, axis, origin);
                let drawn = space.pixel_outline(&flipped);
                assert_eq!(drawn.points().len(), mirrored.len());
                for p in drawn.points() {
                    assert!(
                        mirrored.iter().any(|m| (*m - *p).length() < 1e-6),
                        "case {case} ({}): {object:?} flipped {axis:?} around {origin}",
                        fuzz.reproduce()
                    );
                }
            }
        }
    }

    #[test]
    fn flipping_twice_gives_the_object_back() {
        let fuzz = Fuzz::from_env(0x0007_F11F, 300);
        for orientation in [
            Orientation::Orthogonal,
            Orientation::Isometric,
            Orientation::Oblique,
        ] {
            let mut map = space_map(orientation, Alignment::Unspecified);
            map.skew = glam::IVec2::new(6, 0);
            let space = ObjectSpace::of_map(&map);
            for case in 0..fuzz.cases {
                let mut rng = Rng::new(fuzz.case_seed(case));
                let object = random_object(&mut rng);
                let origin = v(rng.next_f64() * 300.0, rng.next_f64() * 300.0);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let back = flipped(
                        &space,
                        &flipped(&space, &object, axis, origin),
                        axis,
                        origin,
                    );
                    assert_near(back.pos, object.pos);
                    assert_eq!(
                        (back.rotation, &back.shape, back.tile, back.size),
                        (object.rotation, &object.shape, object.tile, object.size),
                        "case {case} ({}) on {orientation:?}",
                        fuzz.reproduce()
                    );
                }
            }
        }
    }

    #[test]
    fn template_instances_mark_what_a_flip_changes() {
        let mut map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        let instance = MapObject {
            template: Some(FileRef::new("tree.tx", "/maps/tree.tx")),
            tile: Some(tile(1)),
            ..rect_object(0.0, 16.0, 16.0, 16.0)
        };
        map.layers.push(bt_core::Layer::new(
            bt_core::LayerId(1),
            "things",
            bt_core::LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![
                    MapObject {
                        id: ObjectId(1),
                        ..instance
                    },
                    MapObject {
                        id: ObjectId(2),
                        ..rect_object(40.0, 0.0, 8.0, 8.0)
                    },
                ]),
                ..ObjectLayer::default()
            }),
        ));
        map.next_layer_id = 2;
        map.next_object_id = 3;
        let report = flip_objects(
            &mut map,
            &[ObjectId(2), ObjectId(1)],
            Axis::Horizontal,
            v(20.0, 0.0),
        )
        .unwrap();
        assert_eq!(report.changed, [ObjectId(2), ObjectId(1)]);
        assert_eq!(
            find_object(&map, ObjectId(1)).unwrap().object.overrides,
            ObjectOverrides::TILE | ObjectOverrides::ROTATION | ObjectOverrides::SHAPE
        );
        assert!(
            find_object(&map, ObjectId(2))
                .unwrap()
                .object
                .overrides
                .is_empty()
        );
        assert_valid(&map);
        rotate_objects(&mut map, &[ObjectId(1)], 0.0, Pivot::OwnPosition).unwrap();
        let rotated = rotate_objects(&mut map, &[ObjectId(2)], 90.0, Pivot::OwnPosition).unwrap();
        assert_eq!(rotated.changed, [ObjectId(2)]);
        assert_valid(&map);
    }

    #[test]
    fn rotations_turn_around_a_point_or_in_place() {
        let mut map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        map.layers.push(bt_core::Layer::new(
            bt_core::LayerId(1),
            "things",
            bt_core::LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![
                    MapObject {
                        id: ObjectId(1),
                        rotation: 300.0,
                        ..rect_object(10.0, 0.0, 4.0, 4.0)
                    },
                    MapObject {
                        id: ObjectId(2),
                        shape: Shape::Point,
                        ..rect_object(0.0, 10.0, 0.0, 0.0)
                    },
                    MapObject {
                        id: ObjectId(3),
                        template: Some(FileRef::new("a.tx", "/a.tx")),
                        ..rect_object(0.0, 0.0, 4.0, 4.0)
                    },
                ]),
                ..ObjectLayer::default()
            }),
        ));
        map.next_layer_id = 2;
        map.next_object_id = 4;
        let all = [ObjectId(1), ObjectId(2), ObjectId(3)];
        let report = rotate_objects(&mut map, &all, 90.0, Pivot::Point(v(0.0, 0.0))).unwrap();
        assert_eq!(report.changed, all);
        let object = |map: &Map, id| find_object(map, ObjectId(id)).unwrap().object.clone();
        assert_eq!(
            (object(&map, 1).pos, object(&map, 1).rotation),
            (v(0.0, 10.0), 30.0)
        );
        assert_eq!(
            (object(&map, 2).pos, object(&map, 2).rotation),
            (v(-10.0, 0.0), 0.0),
            "points only move"
        );
        assert_eq!(
            (object(&map, 3).pos, object(&map, 3).rotation),
            (v(0.0, 0.0), 90.0)
        );
        assert_eq!(object(&map, 3).overrides, ObjectOverrides::ROTATION);
        let report = rotate_objects(&mut map, &all, 90.0, Pivot::OwnPosition).unwrap();
        assert_eq!(report.changed, [ObjectId(1), ObjectId(3)]);
        let rotations = |map: &Map| (object(map, 1).rotation, object(map, 3).rotation);
        assert_eq!(rotations(&map), (120.0, 180.0));
        rotate_objects(&mut map, &all, 90.0, Pivot::OwnPosition).unwrap();
        assert_eq!(rotations(&map), (-150.0, -90.0));
        rotate_objects(&mut map, &all, -90.0, Pivot::Point(v(0.0, 0.0))).unwrap();
        assert_eq!(rotations(&map), (120.0, 180.0));
        assert_eq!(object(&map, 1).pos, v(10.0, 0.0));
        assert_valid(&map);
        assert_eq!(
            (within_half_turns(-270.0), within_half_turns(540.0)),
            (90.0, 180.0)
        );
        assert_eq!(
            (within_half_turns(-180.0), within_half_turns(541.0)),
            (-180.0, -179.0)
        );
        assert_eq!(
            (within_full_turn(-30.0), within_full_turn(-1e-20)),
            (330.0, 0.0)
        );
    }

    #[test]
    fn isometric_rotations_turn_on_screen() {
        let map = space_map(Orientation::Isometric, Alignment::Unspecified);
        let space = ObjectSpace::of_map(&map);
        let geometry = map.geometry();
        let object = rect_object(64.0, 0.0, 8.0, 8.0);
        let pivot = v(32.0, 32.0);
        let turned = turned(&space, &object, 90.0, Pivot::Point(pivot));
        let arm = geometry.pixel_to_screen(object.pos) - geometry.pixel_to_screen(pivot);
        assert_near(
            geometry.pixel_to_screen(turned.pos) - geometry.pixel_to_screen(pivot),
            rotated(arm, 90.0),
        );
        assert_eq!(turned.rotation, 90.0);
    }

    #[test]
    fn rotate_handles_turn_by_the_angle_dragged_around_the_centre() {
        let map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        let geometry = map.geometry();
        let centre = v(100.0, 100.0);
        let angle = |from, to, snap| drag_angle(&geometry, centre, from, to, snap);
        // From the right of the centre to below it is a quarter turn clockwise on screen.
        assert!((angle(v(110.0, 100.0), v(100.0, 110.0), false) - 90.0).abs() < 1e-9);
        assert!((angle(v(110.0, 100.0), v(100.0, 90.0), false) + 90.0).abs() < 1e-9);
        // Ctrl snaps to steps of 15°: 16.7° goes to 15°, 6.8° to nothing.
        assert_eq!(angle(v(110.0, 100.0), v(110.0, 103.0), true), 15.0);
        assert_eq!(angle(v(110.0, 100.0), v(110.0, 101.2), true), 0.0);
        assert_eq!(angle(v(110.0, 100.0), v(105.0, 108.66), true), 60.0);
        assert_eq!(angle(v(110.0, 100.0), v(110.0, 100.0), false), 0.0);
        // On an isometric map the angle is the one the screen shows: from the centre's right
        // corner of a cell to its bottom corner is a quarter turn of the diamond's diagonals.
        let iso = space_map(Orientation::Isometric, Alignment::Unspecified);
        let geometry = iso.geometry();
        let centre = v(32.0, 32.0);
        let screen = geometry.pixel_to_screen(centre);
        let right = geometry.screen_to_pixel(screen + v(20.0, 0.0));
        let below = geometry.screen_to_pixel(screen + v(0.0, 20.0));
        assert!((drag_angle(&geometry, centre, right, below, false) - 90.0).abs() < 1e-9);
        // The objects then turn around the centre by it.
        let mut map = space_map(Orientation::Orthogonal, Alignment::Unspecified);
        map.layers.push(bt_core::Layer::new(
            bt_core::LayerId(1),
            "things",
            bt_core::LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![MapObject {
                    id: ObjectId(1),
                    ..rect_object(110.0, 100.0, 4.0, 4.0)
                }]),
                ..ObjectLayer::default()
            }),
        ));
        map.next_layer_id = 2;
        map.next_object_id = 2;
        let degrees = drag_angle(
            &map.geometry(),
            v(100.0, 100.0),
            v(110.0, 100.0),
            v(100.0, 112.0),
            true,
        );
        rotate_objects(
            &mut map,
            &[ObjectId(1)],
            degrees,
            Pivot::Point(v(100.0, 100.0)),
        )
        .unwrap();
        let turned = find_object(&map, ObjectId(1)).unwrap().object;
        assert_near(turned.pos, v(100.0, 110.0));
        assert_eq!(turned.rotation, 90.0);
    }

    #[test]
    fn selections_flip_around_the_middle_of_what_they_cover() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    MapObject {
                        rotation: 90.0,
                        ..rect_object(40.0, 0.0, 20.0, 10.0)
                    },
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(100.0, 100.0, 0.0, 0.0)
                    },
                ],
            )
            .build()
            .unwrap();
        assert_eq!(
            flip_origin(&map, &[ObjectId(1), ObjectId(2)]),
            Ok(Some(v(20.0, 10.0))),
            "0..40 across, the turned box reaching 20 down"
        );
        assert_eq!(flip_origin(&map, &[ObjectId(3)]), Ok(Some(v(100.0, 100.0))));
        assert_eq!(flip_origin(&map, &[]), Ok(None));
        let iso = MapBuilder::new(UVec2::new(64, 32), UVec2::splat(8))
            .orientation(Orientation::Isometric)
            .object_layer("things", vec![rect_object(32.0, 32.0, 32.0, 32.0)])
            .build()
            .unwrap();
        assert_near(
            flip_origin(&iso, &[ObjectId(1)]).unwrap().unwrap(),
            v(48.0, 48.0),
        );
    }

    #[test]
    fn collision_shapes_flip_turn_and_align_in_their_tiles_space() {
        let tileset = Atlas::builder().tile_size(32, 32).build_embedded().unwrap();
        let space = ObjectSpace::of_tile(&tileset.tileset, TileId(0));
        let mut group = bt_core::CollisionGroup {
            id: bt_core::LayerId(0),
            layer: ObjectLayer {
                draw_order: bt_core::DrawOrder::Index,
                objects: Arc::new(vec![
                    MapObject {
                        id: ObjectId(1),
                        ..rect_object(0.0, 0.0, 8.0, 16.0)
                    },
                    MapObject {
                        id: ObjectId(2),
                        shape: Shape::Polygon(vec![v(0.0, 0.0), v(12.0, 0.0), v(0.0, 12.0)]),
                        ..rect_object(20.0, 4.0, 0.0, 0.0)
                    },
                ]),
                ..ObjectLayer::default()
            },
        };
        let both = [ObjectId(1), ObjectId(2)];
        let origin = flip_origin_in(&group.layer, &space, &both)
            .unwrap()
            .unwrap();
        assert_eq!(origin, v(16.0, 8.0));
        flip_objects_in(&mut group.layer, &space, &both, Axis::Horizontal, origin).unwrap();
        let shape =
            |group: &bt_core::CollisionGroup, index: usize| group.layer.objects[index].clone();
        assert_eq!(shape(&group, 0).pos, v(24.0, 0.0));
        assert_eq!(shape(&group, 1).pos, v(12.0, 4.0));
        assert_eq!(
            shape(&group, 1).shape,
            Shape::Polygon(vec![v(0.0, 0.0), v(-12.0, 0.0), v(0.0, 12.0)])
        );
        rotate_objects_in(
            &mut group.layer,
            &space,
            &both,
            90.0,
            Pivot::Point(v(16.0, 16.0)),
        )
        .unwrap();
        assert_eq!(
            (shape(&group, 0).pos, shape(&group, 0).rotation),
            (v(32.0, 24.0), 90.0)
        );
        assert_eq!(shape(&group, 1).pos, v(28.0, 12.0));
        let tile = bt_utils::DRect::from_min_size(DVec2::ZERO, v(32.0, 32.0));
        crate::objects::align::align_objects_in(
            &mut group.layer,
            &space,
            &[ObjectId(1)],
            crate::objects::align::Align::Left,
            tile,
        )
        .unwrap();
        assert_eq!(
            shape(&group, 0).pos,
            v(16.0, 24.0),
            "its turned box was 16..32"
        );
        assert_eq!(
            flip_objects_in(
                &mut group.layer,
                &space,
                &[ObjectId(3)],
                Axis::Vertical,
                origin
            ),
            Err(EditError::NoSuchObject(ObjectId(3)))
        );
    }
}
