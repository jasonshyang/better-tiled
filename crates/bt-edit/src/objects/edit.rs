//! Adding, duplicating, removing and moving objects, and moving them to another layer (Tiled's
//! `MapDocument::duplicateObjects`, `removeObjects` and `moveObjectsToGroup` and the commands
//! behind them: `reference/tiled/src/tiled/mapdocument.cpp`, `addremovemapobject.cpp`,
//! `movemapobjecttogroup.cpp`, `objectreferenceshelper.cpp`).

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bt_core::{LayerId, Map, MapObject, ObjectId, ObjectLayer};
use glam::DVec2;

use crate::duplicate_name::name_of_duplicate;
use crate::error::EditError;
use crate::ids::{IdRemap, NewIds};
use crate::report::ObjectsReport;

use super::find::{
    distinct, ids_by_layer, in_given_order, indices_in, object_layer, object_layer_mut,
    objects_by_id, objects_in, on_layers_holding,
};

/// Adds `object` to the object layer `layer` of `map`, at `index` in its list (on top when
/// `None`). An object with id 0 takes the map's next id; one with an id of its own keeps it,
/// unless another object of the map has it. Returns the object's id.
pub fn add_object(
    map: &mut Map,
    layer: LayerId,
    object: MapObject,
    index: Option<usize>,
) -> Result<ObjectId, EditError> {
    object_layer(map, layer)?;
    if object.id != ObjectId(0) && map.object(object.id).is_some() {
        return Err(EditError::ObjectIdInUse(object.id));
    }
    let mut ids = NewIds::from_map(map);
    let id = add_object_in(object_layer_mut(map, layer)?, object, index, &mut ids)?;
    ids.store_in(map);
    Ok(id)
}

/// Adds `object` to a list at `index` (on top when `None`). An object with id 0 takes the next
/// of `ids`; one with an id of its own keeps it, unless the list holds it already, and `ids`
/// moves past it. Returns the object's id.
pub fn add_object_in(
    objects: &mut ObjectLayer,
    mut object: MapObject,
    index: Option<usize>,
    ids: &mut NewIds,
) -> Result<ObjectId, EditError> {
    let len = objects.objects.len();
    let at = match index {
        None => len,
        Some(index) if index <= len => index,
        Some(index) => return Err(EditError::IndexOutOfRange { index, len }),
    };
    if object.id == ObjectId(0) {
        object.id = ids.take()?;
    } else if objects.objects.iter().any(|o| o.id == object.id) {
        return Err(EditError::ObjectIdInUse(object.id));
    } else {
        ids.skip_past(object.id)?;
    }
    let id = object.id;
    Arc::make_mut(&mut objects.objects).insert(at, object);
    Ok(id)
}

/// Duplicates objects of `map` (Tiled's Duplicate): each copy goes right after its original in
/// the same layer, moved by `offset` (Tiled leaves copies on top of their originals), with the
/// map's next ids in the order the ids are given, and with "Copy" in its name when
/// `copy_suffix` holds. References from one copy to an object copied with it point at that
/// copy, across layers too. Reports the copies' ids in the order of their originals.
pub fn duplicate_objects(
    map: &mut Map,
    ids: &[ObjectId],
    offset: DVec2,
    copy_suffix: bool,
) -> Result<ObjectsReport, EditError> {
    let groups = ids_by_layer(map, ids)?;
    let originals = objects_by_id(map, ids)?
        .into_iter()
        .map(|o| duplicated(o, copy_suffix))
        .collect();
    let mut new_ids = NewIds::from_map(map);
    let copies = renumbered(originals, offset, &mut new_ids)?;
    let changed = copies.iter().map(|c| c.object.id).collect();
    let mut by_original: HashMap<ObjectId, Renumbered> =
        copies.into_iter().map(|c| (c.original, c)).collect();
    for group in &groups {
        let mine = group
            .ids
            .iter()
            .filter_map(|id| by_original.remove(id))
            .collect();
        insert_after_originals(object_layer_mut(map, group.layer)?, mine);
    }
    new_ids.store_in(map);
    Ok(ObjectsReport { changed })
}

/// Duplicates objects of a list, as [`duplicate_objects`] does, with ids from `new_ids`.
pub fn duplicate_objects_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
    offset: DVec2,
    copy_suffix: bool,
    new_ids: &mut NewIds,
) -> Result<ObjectsReport, EditError> {
    let originals = objects_in(objects, ids)?
        .into_iter()
        .map(|o| duplicated(o, copy_suffix))
        .collect();
    let copies = renumbered(originals, offset, new_ids)?;
    let changed = copies.iter().map(|c| c.object.id).collect();
    insert_after_originals(objects, copies);
    Ok(ObjectsReport { changed })
}

/// A copy of `object`, named as a duplicate when `copy_suffix` holds.
fn duplicated(object: &MapObject, copy_suffix: bool) -> MapObject {
    let mut copy = object.clone();
    if copy_suffix {
        copy.name = name_of_duplicate(&copy.name);
    }
    copy
}

/// A copy with its new id, and the id of the object it copies.
pub(crate) struct Renumbered {
    /// The id of the object copied.
    pub(crate) original: ObjectId,
    /// The copy.
    pub(crate) object: MapObject,
}

/// Gives copies new ids from `ids`, in order, and moves them by `offset`; references from one
/// copy to an object copied with it then point at that copy (Tiled's `ObjectReferencesHelper`).
/// Nothing is taken from `ids` unless every copy gets one.
pub(crate) fn renumbered(
    copies: Vec<MapObject>,
    offset: DVec2,
    ids: &mut NewIds,
) -> Result<Vec<Renumbered>, EditError> {
    let mut counter = *ids;
    let mut remap = IdRemap::new();
    let mut out = Vec::with_capacity(copies.len());
    for mut object in copies {
        let id = counter.take()?;
        remap.record(object.id, id);
        let original = object.id;
        object.id = id;
        object.pos += offset;
        out.push(Renumbered { original, object });
    }
    for copy in &mut out {
        remap.rewire_properties(&mut copy.object.properties);
    }
    *ids = counter;
    Ok(out)
}

/// A copy and where it goes.
struct Placed {
    at: usize,
    object: MapObject,
}

/// Inserts each copy right after its original (on top when the original is not in the list),
/// as Tiled's `AddMapObjects` does: by decreasing place, so earlier inserts move no later one.
fn insert_after_originals(objects: &mut ObjectLayer, copies: Vec<Renumbered>) {
    if copies.is_empty() {
        return;
    }
    let mut places = HashMap::new();
    for (index, object) in objects.objects.iter().enumerate() {
        places.entry(object.id).or_insert(index);
    }
    let len = objects.objects.len();
    let mut placed: Vec<Placed> = copies
        .into_iter()
        .map(|copy| Placed {
            at: places.get(&copy.original).map_or(len, |index| index + 1),
            object: copy.object,
        })
        .collect();
    placed.sort_by_key(|p| Reverse(p.at));
    let list = Arc::make_mut(&mut objects.objects);
    for p in placed {
        let at = p.at.min(list.len());
        list.insert(at, p.object);
    }
}

/// Removes objects from `map`. Reports their ids in the order given.
pub fn remove_objects(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, _, held| {
        remove_objects_in(objects, held)
    })
}

/// Removes objects from a list. Reports their ids in the order given.
pub fn remove_objects_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let gone: HashSet<usize> = indices_in(objects, ids)?.into_iter().collect();
    if gone.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let mut index = 0;
    Arc::make_mut(&mut objects.objects).retain(|_| {
        let keep = !gone.contains(&index);
        index += 1;
        keep
    });
    Ok(ObjectsReport {
        changed: distinct(ids),
    })
}

/// Moves objects of `map` by `delta` pixels. Reports their ids in the order given; a zero
/// delta changes nothing.
pub fn move_objects(
    map: &mut Map,
    ids: &[ObjectId],
    delta: DVec2,
) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, _, held| {
        move_objects_in(objects, held, delta)
    })
}

/// Moves objects of a list by `delta` pixels.
pub fn move_objects_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
    delta: DVec2,
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    if delta == DVec2::ZERO || indices.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let list = Arc::make_mut(&mut objects.objects);
    for index in indices {
        if let Some(object) = list.get_mut(index) {
            object.pos += delta;
        }
    }
    Ok(ObjectsReport {
        changed: distinct(ids),
    })
}

/// An object as it will be, at its place in the list.
struct Rewrite {
    index: usize,
    object: MapObject,
}

/// Replaces objects of a list by what `rewrite` makes of them, all worked out before the list
/// changes. Reports those that came out different, in the order given; when none did, the
/// list stays shared.
pub(crate) fn rewrite_objects_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
    rewrite: impl Fn(&MapObject) -> MapObject,
) -> Result<ObjectsReport, EditError> {
    let rewrites: Vec<Rewrite> = indices_in(objects, ids)?
        .into_iter()
        .filter_map(|index| {
            let object = objects.objects.get(index)?;
            let new = rewrite(object);
            (new != *object).then_some(Rewrite { index, object: new })
        })
        .collect();
    if rewrites.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let list = Arc::make_mut(&mut objects.objects);
    let mut changed = Vec::with_capacity(rewrites.len());
    for rewrite in rewrites {
        if let Some(slot) = list.get_mut(rewrite.index) {
            changed.push(rewrite.object.id);
            *slot = rewrite.object;
        }
    }
    Ok(ObjectsReport { changed })
}

/// An object to move, and how far.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ObjectMove {
    /// The object.
    pub(crate) id: ObjectId,
    /// How far, in pixels.
    pub(crate) delta: DVec2,
}

/// Moves each object of a list by its own delta. Reports those that moved.
pub(crate) fn move_each_in(
    objects: &mut ObjectLayer,
    moves: &[ObjectMove],
) -> Result<ObjectsReport, EditError> {
    let moving: Vec<ObjectMove> = moves
        .iter()
        .copied()
        .filter(|m| m.delta != DVec2::ZERO)
        .collect();
    let ids: Vec<ObjectId> = moving.iter().map(|m| m.id).collect();
    let indices = indices_in(objects, &ids)?;
    if indices.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let deltas: HashMap<ObjectId, DVec2> = moving.iter().map(|m| (m.id, m.delta)).collect();
    let list = Arc::make_mut(&mut objects.objects);
    for index in indices {
        if let Some(object) = list.get_mut(index)
            && let Some(delta) = deltas.get(&object.id)
        {
            object.pos += *delta;
        }
    }
    Ok(ObjectsReport {
        changed: distinct(&ids),
    })
}

/// Moves each object of `map` by its own delta. Reports those that moved, in the order given.
pub(crate) fn move_each(map: &mut Map, moves: &[ObjectMove]) -> Result<ObjectsReport, EditError> {
    let ids: Vec<ObjectId> = moves.iter().map(|m| m.id).collect();
    on_layers_holding(map, &ids, |objects, _, held| {
        let held: HashSet<ObjectId> = held.iter().copied().collect();
        let mine: Vec<ObjectMove> = moves
            .iter()
            .copied()
            .filter(|m| held.contains(&m.id))
            .collect();
        move_each_in(objects, &mine)
    })
}

/// Moves objects to the object layer `layer` (Tiled's Move Objects to Layer): they leave their
/// layers and go on top of `layer` in map order, keeping their ids, positions and everything
/// else. Objects already on `layer` stay where they are. Reports the objects moved, in the
/// order given.
pub fn move_objects_to_layer(
    map: &mut Map,
    ids: &[ObjectId],
    layer: LayerId,
) -> Result<ObjectsReport, EditError> {
    object_layer(map, layer)?;
    let groups = ids_by_layer(map, ids)?;
    let mut moving = Vec::new();
    for group in groups.iter().filter(|g| g.layer != layer) {
        let leaving: HashSet<ObjectId> = group.ids.iter().copied().collect();
        let objects = object_layer_mut(map, group.layer)?;
        moving.extend(
            Arc::make_mut(&mut objects.objects).extract_if(.., |o| leaving.contains(&o.id)),
        );
    }
    if moving.is_empty() {
        return Ok(ObjectsReport::default());
    }
    let changed = moving.iter().map(|o: &MapObject| o.id).collect();
    Arc::make_mut(&mut object_layer_mut(map, layer)?.objects).extend(moving);
    Ok(ObjectsReport {
        changed: in_given_order(changed, ids),
    })
}

#[cfg(test)]
mod tests {
    use bt_core::{Property, Value};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_valid, ids_of, rect_object};

    fn named(name: &str, x: f64) -> MapObject {
        MapObject {
            name: name.into(),
            ..rect_object(x, 0.0, 8.0, 8.0)
        }
    }

    /// Two object layers around a tile layer: “a” (1, 2, 3) and “b” (4).
    fn sample() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "a",
                vec![named("one", 0.0), named("two", 10.0), named("three", 20.0)],
            )
            .tile_layer("ground", "")
            .object_layer("b", vec![named("four", 30.0)])
            .build()
            .unwrap()
    }

    fn layer_ids(map: &Map, layer: u32) -> Vec<u32> {
        ids_of(
            map.layer(LayerId(layer))
                .unwrap()
                .as_object_layer()
                .unwrap(),
        )
    }

    #[test]
    fn objects_are_added_with_new_or_their_own_ids() {
        let mut map = sample();
        let id = add_object(&mut map, LayerId(1), named("new", 0.0), Some(0)).unwrap();
        assert_eq!(id, ObjectId(5));
        assert_eq!(layer_ids(&map, 1), [5, 1, 2, 3]);
        let own = MapObject {
            id: ObjectId(40),
            ..named("own", 0.0)
        };
        assert_eq!(
            add_object(&mut map, LayerId(3), own, None),
            Ok(ObjectId(40))
        );
        assert_eq!(map.next_object_id, 41, "the counter passes kept ids");
        assert_valid(&map);
        let taken = MapObject {
            id: ObjectId(2),
            ..named("taken", 0.0)
        };
        assert_eq!(
            add_object(&mut map, LayerId(3), taken, None),
            Err(EditError::ObjectIdInUse(ObjectId(2))),
            "an id another layer holds"
        );
        assert_eq!(
            add_object(&mut map, LayerId(2), named("x", 0.0), None),
            Err(EditError::NotAnObjectLayer(LayerId(2)))
        );
        assert_eq!(
            add_object(&mut map, LayerId(1), named("x", 0.0), Some(9)),
            Err(EditError::IndexOutOfRange { index: 9, len: 4 })
        );
        assert_valid(&map);
    }

    #[test]
    fn duplicates_follow_their_originals_and_their_references() {
        let mut map = sample();
        let mut linked = named("door", 0.0);
        linked
            .properties
            .insert("to".into(), Property::new(Value::Object(ObjectId(4))));
        linked
            .properties
            .insert("stays".into(), Property::new(Value::Object(ObjectId(2))));
        *object_mut_by_id(&mut map, ObjectId(1)) = MapObject {
            id: ObjectId(1),
            ..linked
        };
        let report = duplicate_objects(
            &mut map,
            &[ObjectId(4), ObjectId(1), ObjectId(1)],
            DVec2::new(16.0, 0.0),
            true,
        )
        .unwrap();
        assert_eq!(
            report.changed,
            [ObjectId(5), ObjectId(6)],
            "ids in the order given"
        );
        assert_eq!(layer_ids(&map, 1), [1, 6, 2, 3]);
        assert_eq!(layer_ids(&map, 3), [4, 5]);
        let copy = find_object(&map, ObjectId(6)).unwrap().object;
        assert_eq!(copy.name, "door Copy");
        assert_eq!(copy.pos, DVec2::new(16.0, 0.0));
        assert_eq!(
            copy.properties["to"].value,
            Value::Object(ObjectId(5)),
            "across layers to the copy"
        );
        assert_eq!(copy.properties["stays"].value, Value::Object(ObjectId(2)));
        assert_eq!(map.next_object_id, 7);
        assert_valid(&map);
        let again = duplicate_objects(&mut map, &[ObjectId(6)], DVec2::ZERO, true).unwrap();
        assert_eq!(
            find_object(&map, again.changed[0]).unwrap().object.name,
            "door Copy 2"
        );
        let plain = duplicate_objects(&mut map, &[ObjectId(2)], DVec2::ZERO, false).unwrap();
        assert_eq!(
            find_object(&map, plain.changed[0]).unwrap().object.name,
            "two"
        );
        assert_eq!(
            duplicate_objects(&mut map, &[ObjectId(99)], DVec2::ZERO, false),
            Err(EditError::NoSuchObject(ObjectId(99)))
        );
        assert_valid(&map);
    }

    fn object_mut_by_id(map: &mut Map, id: ObjectId) -> &mut MapObject {
        crate::objects::find::object_mut(map, id).unwrap()
    }

    #[test]
    fn duplicating_without_ids_to_give_changes_nothing() {
        let mut map = sample();
        map.next_object_id = u32::MAX - 1;
        let before = map.clone();
        assert_eq!(
            duplicate_objects(&mut map, &[ObjectId(1), ObjectId(2)], DVec2::ZERO, false),
            Err(EditError::OutOfIds)
        );
        assert_eq!(map, before);
    }

    #[test]
    fn objects_are_removed_and_moved() {
        let mut map = sample();
        let shared = map.clone();
        assert_eq!(
            move_objects(&mut map, &[ObjectId(4), ObjectId(2)], DVec2::new(1.0, -2.0))
                .unwrap()
                .changed,
            [ObjectId(4), ObjectId(2)]
        );
        assert_eq!(
            find_object(&map, ObjectId(2)).unwrap().object.pos,
            DVec2::new(11.0, -2.0)
        );
        assert_eq!(
            find_object(&shared, ObjectId(2)).unwrap().object.pos,
            DVec2::new(10.0, 0.0)
        );
        let untouched = map.clone();
        assert!(
            move_objects(&mut map, &[ObjectId(1)], DVec2::ZERO)
                .unwrap()
                .changed
                .is_empty()
        );
        assert!(Arc::ptr_eq(
            &untouched.layers[0].as_object_layer().unwrap().objects,
            &map.layers[0].as_object_layer().unwrap().objects
        ));
        let removed = remove_objects(&mut map, &[ObjectId(3), ObjectId(4), ObjectId(1)]).unwrap();
        assert_eq!(removed.changed, [ObjectId(3), ObjectId(4), ObjectId(1)]);
        assert_eq!(layer_ids(&map, 1), [2]);
        assert!(layer_ids(&map, 3).is_empty());
        assert_eq!(
            remove_objects(&mut map, &[ObjectId(2), ObjectId(3)]),
            Err(EditError::NoSuchObject(ObjectId(3)))
        );
        assert_eq!(layer_ids(&map, 1), [2], "nothing goes when one is missing");
        assert_valid(&map);
    }

    #[test]
    fn objects_move_to_the_top_of_another_layer_in_map_order() {
        let mut map = sample();
        let report = move_objects_to_layer(
            &mut map,
            &[ObjectId(3), ObjectId(4), ObjectId(1)],
            LayerId(3),
        )
        .unwrap();
        assert_eq!(
            report.changed,
            [ObjectId(3), ObjectId(1)],
            "object 4 is there already"
        );
        assert_eq!(layer_ids(&map, 1), [2]);
        assert_eq!(layer_ids(&map, 3), [4, 1, 3]);
        assert_eq!(
            move_objects_to_layer(&mut map, &[ObjectId(2)], LayerId(2)),
            Err(EditError::NotAnObjectLayer(LayerId(2)))
        );
        assert_eq!(
            move_objects_to_layer(&mut map, &[ObjectId(2)], LayerId(7)),
            Err(EditError::NoSuchLayer(LayerId(7)))
        );
        assert_valid(&map);
    }

    #[test]
    fn a_tiles_collision_shapes_take_ids_of_their_own() {
        let mut shapes = ObjectLayer {
            objects: Arc::new(vec![
                MapObject {
                    id: ObjectId(3),
                    ..rect_object(0.0, 0.0, 4.0, 4.0)
                },
                MapObject {
                    id: ObjectId(7),
                    ..rect_object(4.0, 4.0, 4.0, 4.0)
                },
            ]),
            ..ObjectLayer::default()
        };
        let mut ids = NewIds::after_highest(&shapes);
        let added = add_object_in(&mut shapes, rect_object(1.0, 1.0, 2.0, 2.0), None, &mut ids);
        assert_eq!(added, Ok(ObjectId(8)));
        let mut ids = NewIds::after_highest(&shapes);
        let copies =
            duplicate_objects_in(&mut shapes, &[ObjectId(3)], DVec2::ONE, true, &mut ids).unwrap();
        assert_eq!(copies.changed, [ObjectId(9)]);
        assert_eq!(ids_of(&shapes), [3, 9, 7, 8]);
        move_objects_in(&mut shapes, &[ObjectId(9)], DVec2::new(2.0, 0.0)).unwrap();
        assert_eq!(shapes.objects[1].pos, DVec2::new(3.0, 1.0));
        remove_objects_in(&mut shapes, &[ObjectId(7)]).unwrap();
        assert_eq!(ids_of(&shapes), [3, 9, 8]);
        assert_eq!(
            remove_objects_in(&mut shapes, &[ObjectId(7)]),
            Err(EditError::NoSuchObject(ObjectId(7)))
        );
    }

    /// Every object id of `map`, in map order.
    fn all_ids(map: &Map) -> Vec<ObjectId> {
        let mut ids = Vec::new();
        map.for_each_layer(|layer, _| {
            if let Some(objects) = layer.as_object_layer() {
                ids.extend(objects.objects.iter().map(|o| o.id));
            }
        });
        ids
    }

    #[test]
    fn random_edits_keep_the_map_valid_and_failures_change_nothing() {
        use bt_test_data::Fuzz;
        use bt_test_data::fixtures::Atlas;
        use bt_utils::rng::Rng;

        use crate::axis::Axis;
        use crate::objects::{align, convert, order, transform};
        use crate::testutil::random_object;

        let fuzz = Fuzz::from_env(0x0ED1_7ED5, 150);
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut failures = 0;
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let listed = bt_core::Layer::new(
                LayerId(0),
                "listed",
                bt_core::LayerKind::Object(ObjectLayer {
                    draw_order: bt_core::DrawOrder::Index,
                    objects: Arc::new((0..3).map(|_| random_object(&mut rng)).collect()),
                    ..ObjectLayer::default()
                }),
            );
            let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
                .tileset(std::sync::Arc::clone(&atlas.tileset))
                .object_layer("drawn", (0..4).map(|_| random_object(&mut rng)).collect())
                .tile_layer("ground", "")
                .layer(listed)
                .build()
                .unwrap();
            assert_valid(&map);
            for step in 0..12 {
                let all = all_ids(&map);
                let some: Vec<ObjectId> = all
                    .iter()
                    .copied()
                    .filter(|_| rng.below(3) == Some(0))
                    .collect();
                let layer = LayerId([1, 3][rng.below(2).unwrap() as usize]);
                let delta = DVec2::new(rng.next_f64() * 40.0 - 20.0, rng.next_f64() * 40.0 - 20.0);
                let before = map.clone();
                let result = match rng.below(13).unwrap() {
                    0 => duplicate_objects(&mut map, &some, delta, rng.below(2) == Some(0)),
                    1 => remove_objects(&mut map, &some),
                    2 => move_objects_to_layer(&mut map, &some, layer),
                    3 => {
                        let mut object = random_object(&mut rng);
                        object.template = None;
                        object.id = ObjectId(rng.below(12).unwrap() as u32);
                        add_object(&mut map, layer, object, None).map(|_| ObjectsReport::default())
                    }
                    4 => transform::rotate_objects(
                        &mut map,
                        &some,
                        90.0,
                        transform::Pivot::OwnPosition,
                    ),
                    5 => transform::flip_objects(&mut map, &some, Axis::Vertical, delta),
                    6 => transform::rotate_objects(
                        &mut map,
                        &some,
                        rng.next_f64() * 360.0,
                        transform::Pivot::Point(delta),
                    ),
                    7 => order::raise_objects(&mut map, &some),
                    8 => order::lower_objects_to_bottom(&mut map, &some),
                    9 => order::move_objects_up(&mut map, &some),
                    10 => convert::rect_to_polygon(&mut map, &some),
                    11 => align::distribute_objects(&mut map, &some, Axis::Horizontal),
                    _ => move_objects(&mut map, &some, delta),
                };
                let context = format!("case {case} ({}), step {step}", fuzz.reproduce());
                if result.is_err() {
                    failures += 1;
                    assert_eq!(map, before, "a failure changed the map: {context}");
                }
                assert!(map.validate().is_empty(), "{:?}: {context}", map.validate());
            }
        }
        assert!(failures > 0, "some steps should be refused");
    }
}
