//! Objects the rules place keep their ids from one result to the next when they are the same
//! objects (`08-automap.md`, §10), so a diff of the map does not call them changed: a run numbers
//! what it places after the painted map's next id, which moves whenever painted objects come and
//! go.

use std::collections::{HashMap, HashSet};

use bt_core::{LayerId, LayerKind, Map, MapObject, ObjectId};

use crate::engine::rewire;

/// Where an object is: the layer and its position, exactly, to find the same object again.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Spot {
    layer: LayerId,
    x: u64,
    y: u64,
}

impl Spot {
    fn of(layer: LayerId, object: &MapObject) -> Spot {
        Spot {
            layer,
            x: object.pos.x.to_bits(),
            y: object.pos.y.to_bits(),
        }
    }
}

/// An object the rules placed in an earlier result.
struct Placed {
    id: ObjectId,
    /// The object with its id left out, to compare.
    object: MapObject,
    taken: bool,
}

/// Gives the objects the rules placed in `result` (those `painted` lacks) the ids they had in
/// `previous`, a result made from `previous_painted`, where an object of the same layer is the
/// same apart from its id and that id is free; the others take ids after every id in use.
/// References to the objects follow them.
pub(crate) fn keep_object_ids(
    result: &mut Map,
    painted: &Map,
    previous: &Map,
    previous_painted: &Map,
) {
    let painted_ids = object_ids(painted);
    let previous_painted_ids = object_ids(previous_painted);
    let mut earlier: HashMap<Spot, Vec<Placed>> = HashMap::new();
    for_each_object(previous, |layer, object| {
        if !previous_painted_ids.contains(&object.id) && !painted_ids.contains(&object.id) {
            earlier
                .entry(Spot::of(layer, object))
                .or_default()
                .push(Placed {
                    id: object.id,
                    object: without_id(object),
                    taken: false,
                });
        }
    });
    let mut renamed: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut unmatched = Vec::new();
    for_each_object(result, |layer, object| {
        if painted_ids.contains(&object.id) {
            return;
        }
        let same = earlier
            .get_mut(&Spot::of(layer, object))
            .and_then(|candidates| {
                let plain = without_id(object);
                candidates
                    .iter_mut()
                    .find(|c| !c.taken && c.object == plain)
            });
        match same {
            Some(placed) => {
                placed.taken = true;
                renamed.insert(object.id, placed.id);
            }
            None => unmatched.push(object.id),
        }
    });
    let mut used: HashSet<ObjectId> = painted_ids;
    used.extend(renamed.values().copied());
    let mut next = used
        .iter()
        .map(|id| id.0.saturating_add(1))
        .max()
        .unwrap_or(1)
        .max(result.next_object_id);
    for id in unmatched {
        // An object kept by number elsewhere may hold this id: it moves after everything used.
        let fresh = if used.contains(&id) {
            let fresh = ObjectId(next);
            next = next.saturating_add(1);
            fresh
        } else {
            id
        };
        used.insert(fresh);
        renamed.insert(id, fresh);
    }
    if renamed.iter().all(|(from, to)| from == to) {
        return;
    }
    rename_objects(result, &renamed);
    let highest = used.iter().map(|id| id.0).max().unwrap_or(0);
    result.next_object_id = result.next_object_id.max(highest.saturating_add(1));
}

/// Every object's id in `map`.
fn object_ids(map: &Map) -> HashSet<ObjectId> {
    let mut ids = HashSet::new();
    for_each_object(map, |_, object| {
        ids.insert(object.id);
    });
    ids
}

fn for_each_object(map: &Map, mut visit: impl FnMut(LayerId, &MapObject)) {
    map.for_each_layer(|layer, _| {
        if let LayerKind::Object(objects) = &layer.kind {
            for object in objects.objects.iter() {
                visit(layer.id, object);
            }
        }
    });
}

fn without_id(object: &MapObject) -> MapObject {
    MapObject {
        id: ObjectId(0),
        ..object.clone()
    }
}

/// Renames objects as `renamed` says, and the references to them.
fn rename_objects(map: &mut Map, renamed: &HashMap<ObjectId, ObjectId>) {
    map.for_each_layer_mut(|layer| {
        if let LayerKind::Object(objects) = &mut layer.kind {
            for object in std::sync::Arc::make_mut(&mut objects.objects) {
                if let Some(new) = renamed.get(&object.id) {
                    object.id = *new;
                }
                rewire(&mut object.properties, renamed);
            }
        }
    });
}
