//! Showing, hiding, locking and unlocking layers, and the "other layers" Show/Hide Other Layers
//! and Lock/Unlock Other Layers act on (Tiled's `LayerModel::toggleLayers`, `toggleLockLayers`
//! and `collectAllSiblings`, `reference/tiled/src/tiled/layermodel.cpp`).

use std::collections::HashSet;

use bt_core::{Layer, LayerId, Map};

use super::tree::{check_layers, children, distinct, layer_order, locate};
use crate::error::EditError;
use crate::report::LayersReport;

/// Show/Hide Layers: shows the layers when every one is hidden, hides them otherwise. Show/Hide
/// Other Layers is this over [`other_layers`]. Reports the layers that changed.
pub fn toggle_visible(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let ids = distinct(ids);
    let show = ids
        .iter()
        .all(|id| map.layer(*id).is_some_and(|layer| !layer.visible));
    Ok(set_flag(map, &ids, show, |layer| &mut layer.visible))
}

/// Lock/Unlock Layers: locks the layers when one of them is unlocked, unlocks them otherwise.
/// Lock/Unlock Other Layers is this over [`other_layers`]. Reports the layers that changed.
pub fn toggle_locked(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let ids = distinct(ids);
    let lock = ids
        .iter()
        .any(|id| map.layer(*id).is_some_and(|layer| !layer.locked));
    Ok(set_flag(map, &ids, lock, |layer| &mut layer.locked))
}

/// Sets one flag of each layer to `value`; the layers that changed.
fn set_flag(
    map: &mut Map,
    ids: &[LayerId],
    value: bool,
    flag: impl Fn(&mut Layer) -> &mut bool,
) -> LayersReport {
    let mut report = LayersReport::default();
    for &id in ids {
        if let Some(layer) = map.layer_mut(id) {
            let field = flag(layer);
            if *field != value {
                *field = value;
                report.changed.push(id);
            }
        }
    }
    report
}

/// The layers beside the given ones: the other layers of their groups and of every group they
/// are in, up to the map's top level, without the layers given and the groups holding them
/// (Tiled's `collectAllSiblings`). In Tiled's layer order.
pub fn other_layers(map: &Map, ids: &[LayerId]) -> Result<Vec<LayerId>, EditError> {
    check_layers(map, ids)?;
    let mut collected: HashSet<LayerId> = HashSet::new();
    let mut todo = distinct(ids);
    while !todo.is_empty() {
        let id = todo.remove(0);
        let at = locate(map, id)?;
        for sibling in children(map, at.parent)? {
            collected.insert(sibling.id);
            todo.retain(|other| *other != sibling.id);
        }
        if let Some(parent) = at.parent
            && !collected.contains(&parent)
            && !todo.contains(&parent)
        {
            todo.push(parent);
        }
    }
    // Not the layers given, nor the groups they are in.
    for &id in ids {
        let path = map.layer_path(id).unwrap_or_default();
        for layer in path.iter().rev() {
            if !collected.remove(layer) {
                break;
            }
        }
    }
    Ok(layer_order(map)
        .into_iter()
        .filter(|id| collected.contains(id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::nested;

    #[test]
    fn showing_needs_every_layer_hidden_and_locking_one_unlocked() {
        let mut map = nested();
        map.layer_mut(LayerId(1)).unwrap().visible = false;
        let report = toggle_visible(&mut map, &[LayerId(1), LayerId(3)]).unwrap();
        assert_eq!(
            report.changed,
            [LayerId(3)],
            "one was shown: both are hidden"
        );
        let report = toggle_visible(&mut map, &[LayerId(1), LayerId(3)]).unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(3)]);
        assert!(map.layer(LayerId(3)).unwrap().visible);
        map.layer_mut(LayerId(1)).unwrap().locked = true;
        let report = toggle_locked(&mut map, &[LayerId(1), LayerId(3)]).unwrap();
        assert_eq!(
            report.changed,
            [LayerId(3)],
            "one was unlocked: both are locked"
        );
        let report = toggle_locked(&mut map, &[LayerId(1), LayerId(3)]).unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(3)]);
        assert_eq!(
            toggle_locked(&mut map, &[LayerId(8)]),
            Err(EditError::NoSuchLayer(LayerId(8)))
        );
    }

    #[test]
    fn other_layers_are_the_siblings_up_the_tree() {
        let map = nested();
        // The crates: nothing beside them in props; the floor beside props in walls; the ground
        // and the sky at the top. Not props or walls, which hold them.
        assert_eq!(
            other_layers(&map, &[LayerId(5)]),
            Ok(vec![LayerId(1), LayerId(3), LayerId(6)])
        );
        assert_eq!(
            other_layers(&map, &[LayerId(2)]),
            Ok(vec![LayerId(1), LayerId(6)])
        );
        assert_eq!(
            other_layers(&map, &[LayerId(3), LayerId(6)]),
            Ok(vec![LayerId(1), LayerId(4)])
        );
        let mut map = nested();
        let others = other_layers(&map, &[LayerId(5)]).unwrap();
        let report = toggle_visible(&mut map, &others).unwrap();
        assert_eq!(report.changed, others, "Hide Other Layers");
        assert!(map.layer(LayerId(5)).unwrap().visible);
    }
}
