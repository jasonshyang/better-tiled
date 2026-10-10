//! Finding objects by id: across a map's object layers (Tiled's `Map::findObjectById`,
//! `reference/tiled/src/libtiled/map.cpp`), or in one list; and the object layers a set of ids
//! lives in, which the map-level operations run on layer by layer.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bt_core::{Layer, LayerId, Map, MapObject, ObjectId, ObjectLayer};

use crate::error::EditError;
use crate::report::ObjectsReport;

use super::space::ObjectSpace;

/// An object found in a map.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Found<'a> {
    /// The object layer holding it.
    pub layer: LayerId,
    /// Its place in the layer's list, from the bottom.
    pub index: usize,
    /// The object.
    pub object: &'a MapObject,
}

/// The object with `id`, on any object layer of `map`.
pub fn find_object(map: &Map, id: ObjectId) -> Result<Found<'_>, EditError> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_some() {
            return;
        }
        if let Some(objects) = layer.as_object_layer()
            && let Some((index, object)) =
                objects.objects.iter().enumerate().find(|(_, o)| o.id == id)
        {
            found = Some(Found {
                layer: layer.id,
                index,
                object,
            });
        }
    });
    found.ok_or(EditError::NoSuchObject(id))
}

/// The object with `id` in `map`, to change. Its layer's list is copied first when a snapshot
/// shares it.
pub fn object_mut(map: &mut Map, id: ObjectId) -> Result<&mut MapObject, EditError> {
    let found = find_object(map, id)?;
    let index = found.index;
    let objects = object_layer_mut(map, found.layer)?;
    Arc::make_mut(&mut objects.objects)
        .get_mut(index)
        .ok_or(EditError::NoSuchObject(id))
}

/// The place of the object with `id` in a list of objects, from the bottom.
pub fn object_index_in(objects: &ObjectLayer, id: ObjectId) -> Result<usize, EditError> {
    objects
        .objects
        .iter()
        .position(|o| o.id == id)
        .ok_or(EditError::NoSuchObject(id))
}

/// The object with `id` in a list of objects, to change; the list is copied first when a
/// snapshot shares it.
pub fn object_mut_in(objects: &mut ObjectLayer, id: ObjectId) -> Result<&mut MapObject, EditError> {
    let index = object_index_in(objects, id)?;
    Arc::make_mut(&mut objects.objects)
        .get_mut(index)
        .ok_or(EditError::NoSuchObject(id))
}

/// The object layer with id `layer`.
pub(crate) fn object_layer(map: &Map, layer: LayerId) -> Result<&ObjectLayer, EditError> {
    map.layer(layer)
        .ok_or(EditError::NoSuchLayer(layer))?
        .as_object_layer()
        .ok_or(EditError::NotAnObjectLayer(layer))
}

/// The object layer with id `layer`, to change.
pub(crate) fn object_layer_mut(
    map: &mut Map,
    layer: LayerId,
) -> Result<&mut ObjectLayer, EditError> {
    object_layer_among(&mut map.layers, layer)
}

/// The object layer with id `layer` anywhere in a tree of layers, to change: what operations
/// use when they hold the map's tilesets at the same time.
pub(crate) fn object_layer_among(
    layers: &mut [Layer],
    layer: LayerId,
) -> Result<&mut ObjectLayer, EditError> {
    fn find(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
        for layer in layers {
            if layer.id == id {
                return Some(layer);
            }
            if let Some(children) = layer.children_mut()
                && let Some(found) = find(children, id)
            {
                return Some(found);
            }
        }
        None
    }
    find(layers, layer)
        .ok_or(EditError::NoSuchLayer(layer))?
        .as_object_layer_mut()
        .ok_or(EditError::NotAnObjectLayer(layer))
}

/// The ids, each once, in the order first given.
pub(crate) fn distinct(ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// The places of `ids` in a list, each id once, in the order given.
pub(crate) fn indices_in(objects: &ObjectLayer, ids: &[ObjectId]) -> Result<Vec<usize>, EditError> {
    let mut places = HashMap::new();
    for (index, object) in objects.objects.iter().enumerate() {
        places.entry(object.id).or_insert(index);
    }
    distinct(ids)
        .into_iter()
        .map(|id| places.get(&id).copied().ok_or(EditError::NoSuchObject(id)))
        .collect()
}

/// The ids an object layer holds out of those asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayerIds {
    /// The layer.
    pub(crate) layer: LayerId,
    /// The ids, each once, in the order given.
    pub(crate) ids: Vec<ObjectId>,
}

/// The object layers holding `ids`, in map order, each with the ids it holds. An id no object
/// layer holds is `NoSuchObject`.
pub(crate) fn ids_by_layer(map: &Map, ids: &[ObjectId]) -> Result<Vec<LayerIds>, EditError> {
    let wanted = distinct(ids);
    let lookup: HashSet<ObjectId> = wanted.iter().copied().collect();
    let mut home: HashMap<ObjectId, LayerId> = HashMap::new();
    let mut layers = Vec::new();
    map.for_each_layer(|layer, _| {
        let Some(objects) = layer.as_object_layer() else {
            return;
        };
        let before = home.len();
        for object in objects.objects.iter() {
            if lookup.contains(&object.id) {
                home.entry(object.id).or_insert(layer.id);
            }
        }
        if home.len() > before {
            layers.push(layer.id);
        }
    });
    if let Some(missing) = wanted.iter().find(|id| !home.contains_key(id)) {
        return Err(EditError::NoSuchObject(*missing));
    }
    Ok(layers
        .into_iter()
        .map(|layer| LayerIds {
            layer,
            ids: wanted
                .iter()
                .copied()
                .filter(|id| home.get(id) == Some(&layer))
                .collect(),
        })
        .collect())
}

/// The objects with `ids` in `map`, each once, in the order given.
pub(crate) fn objects_by_id<'m>(
    map: &'m Map,
    ids: &[ObjectId],
) -> Result<Vec<&'m MapObject>, EditError> {
    let mut by_id = HashMap::new();
    for group in ids_by_layer(map, ids)? {
        let mine: HashSet<ObjectId> = group.ids.iter().copied().collect();
        let objects = object_layer(map, group.layer)?;
        for object in objects.objects.iter() {
            if mine.contains(&object.id) {
                by_id.entry(object.id).or_insert(object);
            }
        }
    }
    distinct(ids)
        .into_iter()
        .map(|id| by_id.get(&id).copied().ok_or(EditError::NoSuchObject(id)))
        .collect()
}

/// The objects with `ids` in a list, each once, in the order given.
pub(crate) fn objects_in<'o>(
    objects: &'o ObjectLayer,
    ids: &[ObjectId],
) -> Result<Vec<&'o MapObject>, EditError> {
    indices_in(objects, ids)?
        .into_iter()
        .map(|index| {
            objects
                .objects
                .get(index)
                .ok_or(EditError::IndexOutOfRange {
                    index,
                    len: objects.objects.len(),
                })
        })
        .collect()
}

/// Runs a list operation on each object layer of `map` holding some of `ids`, with the ids it
/// holds and the map's space; the changes are reported in the order the ids were given. Every
/// id is found before anything runs.
pub(crate) fn on_layers_holding(
    map: &mut Map,
    ids: &[ObjectId],
    mut op: impl FnMut(
        &mut ObjectLayer,
        &ObjectSpace<'_>,
        &[ObjectId],
    ) -> Result<ObjectsReport, EditError>,
) -> Result<ObjectsReport, EditError> {
    let groups = ids_by_layer(map, ids)?;
    let geometry = map.geometry();
    let mut changed = Vec::new();
    for group in &groups {
        let space = ObjectSpace::from_parts(geometry, &map.tilesets);
        let objects = object_layer_among(&mut map.layers, group.layer)?;
        changed.extend(op(objects, &space, &group.ids)?.changed);
    }
    Ok(ObjectsReport {
        changed: in_given_order(changed, ids),
    })
}

/// `changed` in the order the ids were given in `ids`.
pub(crate) fn in_given_order(mut changed: Vec<ObjectId>, ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut place = HashMap::new();
    for (index, id) in ids.iter().enumerate() {
        place.entry(*id).or_insert(index);
    }
    changed.sort_by_key(|id| place.get(id).copied().unwrap_or(usize::MAX));
    changed
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use glam::{DVec2, UVec2};

    use super::*;
    use crate::testutil::rect_object;

    fn sample() -> Map {
        let group = Layer::new(
            LayerId(0),
            "group",
            bt_core::LayerKind::Group(vec![Layer::new(
                LayerId(0),
                "inner",
                bt_core::LayerKind::Object(ObjectLayer {
                    objects: Arc::new(vec![rect_object(1.0, 1.0, 4.0, 4.0)]),
                    ..ObjectLayer::default()
                }),
            )]),
        );
        MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .tile_layer("ground", "")
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 8.0, 8.0),
                    rect_object(8.0, 0.0, 8.0, 8.0),
                ],
            )
            .layer(group)
            .build()
            .unwrap()
    }

    #[test]
    fn objects_are_found_on_any_layer() {
        let mut map = sample();
        let found = find_object(&map, ObjectId(3)).unwrap();
        assert_eq!(
            (found.layer, found.index),
            (LayerId(4), 0),
            "inside the group"
        );
        assert_eq!(found.object.pos, DVec2::new(1.0, 1.0));
        assert_eq!(
            find_object(&map, ObjectId(9)),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
        let shared = map.clone();
        object_mut(&mut map, ObjectId(2)).unwrap().name = "moved".into();
        assert_eq!(find_object(&map, ObjectId(2)).unwrap().object.name, "moved");
        assert_eq!(
            find_object(&shared, ObjectId(2)).unwrap().object.name,
            "",
            "a snapshot keeps its list"
        );
        assert_eq!(
            object_layer_mut(&mut map, LayerId(1)).map(|_| ()),
            Err(EditError::NotAnObjectLayer(LayerId(1)))
        );
        assert_eq!(
            object_layer_mut(&mut map, LayerId(8)).map(|_| ()),
            Err(EditError::NoSuchLayer(LayerId(8)))
        );
    }

    #[test]
    fn ids_sort_into_their_layers() {
        let map = sample();
        let ids = [ObjectId(3), ObjectId(2), ObjectId(3), ObjectId(1)];
        assert_eq!(
            ids_by_layer(&map, &ids).unwrap(),
            [
                LayerIds {
                    layer: LayerId(2),
                    ids: vec![ObjectId(2), ObjectId(1)]
                },
                LayerIds {
                    layer: LayerId(4),
                    ids: vec![ObjectId(3)]
                }
            ]
        );
        assert_eq!(
            ids_by_layer(&map, &[ObjectId(1), ObjectId(7)]),
            Err(EditError::NoSuchObject(ObjectId(7)))
        );
        assert_eq!(
            objects_by_id(&map, &ids)
                .unwrap()
                .iter()
                .map(|o| o.id)
                .collect::<Vec<_>>(),
            [ObjectId(3), ObjectId(2), ObjectId(1)]
        );
        let things = object_layer(&map, LayerId(2)).unwrap();
        assert_eq!(object_index_in(things, ObjectId(2)), Ok(1));
        assert_eq!(
            indices_in(things, &[ObjectId(2), ObjectId(1), ObjectId(2)]),
            Ok(vec![1, 0])
        );
        assert_eq!(
            indices_in(things, &[ObjectId(3)]),
            Err(EditError::NoSuchObject(ObjectId(3))),
            "not in this list"
        );
        assert_eq!(
            in_given_order(vec![ObjectId(1), ObjectId(3), ObjectId(2)], &ids),
            [ObjectId(3), ObjectId(2), ObjectId(1)]
        );
    }
}
