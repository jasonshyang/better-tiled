//! Where layers are in the tree, the order Tiled visits them in, and taking layers out of the
//! tree and putting them back (Tiled's `Layer::siblingIndex`, `isParentOrSelf` and
//! `LayerIterator`, `reference/tiled/src/libtiled/layer.cpp`, and `LayerModel::takeLayerAt` and
//! `insertLayer`, `tiled/layermodel.cpp`).

use std::collections::HashSet;

use bt_core::{Layer, LayerId, LayerKind, Map};

use crate::error::EditError;

/// Where a layer is in the tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LayerLocation {
    /// The group holding it; `None` for the map's top level.
    pub parent: Option<LayerId>,
    /// Its place among the group's layers, counted from the bottom.
    pub index: usize,
}

/// A layer taken out of the tree, and where it was.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TakenLayer {
    /// The layer, with its children.
    pub(crate) layer: Layer,
    /// Where it was.
    pub(crate) from: LayerLocation,
}

/// Where the layer with `id` is.
pub fn locate(map: &Map, id: LayerId) -> Result<LayerLocation, EditError> {
    fn find(layers: &[Layer], parent: Option<LayerId>, id: LayerId) -> Option<LayerLocation> {
        layers.iter().enumerate().find_map(|(index, layer)| {
            if layer.id == id {
                Some(LayerLocation { parent, index })
            } else {
                find(layer.children(), Some(layer.id), id)
            }
        })
    }
    find(&map.layers, None, id).ok_or(EditError::NoSuchLayer(id))
}

/// The layers of the group `parent`, or the map's top-level layers for `None`, bottom first.
pub fn children(map: &Map, parent: Option<LayerId>) -> Result<&[Layer], EditError> {
    let Some(id) = parent else {
        return Ok(&map.layers);
    };
    match &map.layer(id).ok_or(EditError::NoSuchLayer(id))?.kind {
        LayerKind::Group(layers) => Ok(layers),
        LayerKind::Tile(_) | LayerKind::Object(_) | LayerKind::Image(_) => {
            Err(EditError::NotAGroup(id))
        }
    }
}

/// The layers of the group `parent`, or the map's top-level layers for `None`, to change.
pub(crate) fn children_mut(
    map: &mut Map,
    parent: Option<LayerId>,
) -> Result<&mut Vec<Layer>, EditError> {
    let Some(id) = parent else {
        return Ok(&mut map.layers);
    };
    let layer = map.layer_mut(id).ok_or(EditError::NoSuchLayer(id))?;
    layer.children_mut().ok_or(EditError::NotAGroup(id))
}

/// Every layer in Tiled's order (its `LayerIterator`, the order of its "global index"): bottom
/// to top, a group's layers before the group itself. The order layers are drawn in, with each
/// group where its drawing ends.
pub fn layer_order(map: &Map) -> Vec<LayerId> {
    fn walk(layers: &[Layer], order: &mut Vec<LayerId>) {
        for layer in layers {
            walk(layer.children(), order);
            order.push(layer.id);
        }
    }
    let mut order = Vec::new();
    walk(&map.layers, &mut order);
    order
}

/// The layers of `ids` in [`layer_order`], each once.
pub(crate) fn in_layer_order(map: &Map, ids: &[LayerId]) -> Vec<LayerId> {
    let wanted: HashSet<LayerId> = ids.iter().copied().collect();
    layer_order(map)
        .into_iter()
        .filter(|id| wanted.contains(id))
        .collect()
}

/// `ids` without repeats, in the order given.
pub(crate) fn distinct(ids: &[LayerId]) -> Vec<LayerId> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// Whether the layer `outer` is the layer `id` or a group holding it (Tiled's
/// `isParentOrSelf`, asked of `id`).
pub(crate) fn holds_or_is(map: &Map, outer: LayerId, id: LayerId) -> bool {
    map.layer_path(id).is_some_and(|path| path.contains(&outer))
}

/// Whether the layer is a group.
pub(crate) fn is_group(map: &Map, id: LayerId) -> bool {
    map.layer(id)
        .is_some_and(|layer| matches!(layer.kind, LayerKind::Group(_)))
}

/// Fails with the first id the map has no layer for.
pub(crate) fn check_layers(map: &Map, ids: &[LayerId]) -> Result<(), EditError> {
    match ids.iter().find(|id| map.layer(**id).is_none()) {
        Some(missing) => Err(EditError::NoSuchLayer(*missing)),
        None => Ok(()),
    }
}

/// Takes the layer with `id` out of the tree, with its children.
pub(crate) fn take_layer(map: &mut Map, id: LayerId) -> Result<TakenLayer, EditError> {
    let from = locate(map, id)?;
    let siblings = children_mut(map, from.parent)?;
    if from.index >= siblings.len() {
        return Err(EditError::NoSuchLayer(id));
    }
    Ok(TakenLayer {
        layer: siblings.remove(from.index),
        from,
    })
}

/// Puts `layer` into the tree at `at`; a place past the top of the group is refused.
pub(crate) fn insert_layer(
    map: &mut Map,
    at: LayerLocation,
    layer: Layer,
) -> Result<(), EditError> {
    let siblings = children_mut(map, at.parent)?;
    if at.index > siblings.len() {
        return Err(EditError::NoSuchPlace {
            parent: at.parent,
            index: at.index,
        });
    }
    siblings.insert(at.index, layer);
    Ok(())
}

/// A layer in [`tree_shape`]: its id and the group holding it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShapeEntry {
    id: LayerId,
    parent: Option<LayerId>,
}

/// The tree's shape: every layer with its group, parents before their children. Two maps with
/// the same shape hold the same layers in the same places.
pub(crate) fn tree_shape(map: &Map) -> Vec<ShapeEntry> {
    fn walk(layers: &[Layer], parent: Option<LayerId>, shape: &mut Vec<ShapeEntry>) {
        for layer in layers {
            shape.push(ShapeEntry {
                id: layer.id,
                parent,
            });
            walk(layer.children(), Some(layer.id), shape);
        }
    }
    let mut shape = Vec::new();
    walk(&map.layers, None, &mut shape);
    shape
}

/// Moves layers into `parent` at `index`, one after the other in [`layer_order`], so they keep
/// the order they are drawn in (Tiled's `ReparentLayers`). `index` counts the group's layers
/// before the move; the caller has checked that no layer goes into itself.
pub(crate) fn reparent_layers(
    map: &mut Map,
    ids: &[LayerId],
    parent: Option<LayerId>,
    index: usize,
) -> Result<Vec<LayerId>, EditError> {
    let moving = in_layer_order(map, ids);
    let mut index = index;
    for &id in &moving {
        let taken = take_layer(map, id)?;
        // Taking a layer from below the place moves the place down.
        if taken.from.parent == parent && taken.from.index < index {
            index -= 1;
        }
        insert_layer(map, LayerLocation { parent, index }, taken.layer)?;
        index += 1;
    }
    Ok(moving)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{nested, tree};

    #[test]
    fn layers_are_located_in_their_groups() {
        let map = nested();
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates]] 6:sky"
        );
        assert_eq!(
            locate(&map, LayerId(5)),
            Ok(LayerLocation {
                parent: Some(LayerId(4)),
                index: 0
            })
        );
        assert_eq!(
            locate(&map, LayerId(6)),
            Ok(LayerLocation {
                parent: None,
                index: 2
            })
        );
        assert_eq!(
            locate(&map, LayerId(9)),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        assert_eq!(children(&map, Some(LayerId(2))).map(<[Layer]>::len), Ok(2));
        assert_eq!(
            children(&map, Some(LayerId(1))).map(<[Layer]>::len),
            Err(EditError::NotAGroup(LayerId(1)))
        );
    }

    #[test]
    fn tileds_order_puts_a_groups_layers_before_it() {
        let map = nested();
        assert_eq!(
            layer_order(&map),
            [1, 3, 5, 4, 2, 6].map(LayerId).to_vec(),
            "LayerIterator: children first, bottom to top"
        );
        assert!(holds_or_is(&map, LayerId(2), LayerId(5)));
        assert!(holds_or_is(&map, LayerId(5), LayerId(5)));
        assert!(!holds_or_is(&map, LayerId(5), LayerId(2)));
    }

    #[test]
    fn reparenting_keeps_the_drawing_order() {
        let mut map = nested();
        // The sky (top) and the crates (inside props) go to the bottom of the map, crates first.
        let moved = reparent_layers(&mut map, &[LayerId(6), LayerId(5)], None, 0).unwrap();
        assert_eq!(moved, [LayerId(5), LayerId(6)]);
        assert_eq!(
            tree(&map),
            "5:crates 6:sky 1:ground 2:walls[3:floor 4:props[]]"
        );
        // Moving a layer up within its own group counts places before the move.
        reparent_layers(&mut map, &[LayerId(5)], None, 3).unwrap();
        assert_eq!(
            tree(&map),
            "6:sky 1:ground 5:crates 2:walls[3:floor 4:props[]]"
        );
        assert!(matches!(
            insert_layer(
                &mut map,
                LayerLocation {
                    parent: None,
                    index: 9
                },
                Layer::new(LayerId(7), "x", LayerKind::Group(Vec::new()))
            ),
            Err(EditError::NoSuchPlace {
                parent: None,
                index: 9
            })
        ));
    }
}
