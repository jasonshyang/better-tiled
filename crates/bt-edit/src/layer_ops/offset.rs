//! The Offset Layers tool: dragging layers' drawing offsets (Tiled's `LayerOffsetTool`,
//! `reference/tiled/src/tiled/layeroffsettool.cpp`).

use std::collections::HashSet;

use bt_core::{LayerId, Map};
use glam::DVec2;

use super::tree::{check_layers, holds_or_is, is_group, layer_order};
use crate::error::EditError;
use crate::report::LayersReport;

/// The Offset Layers tool: each layer's offset becomes its offset plus `delta` (pixels), passed
/// through `snap` (the identity for no snapping; the editor snaps to the grid as Tiled does,
/// treating the offset as a position). Locked layers stay, a layer in a locked group too; a
/// layer inside a group that moves does not move again. Apply it to the map as it was when the
/// drag started, with the whole drag's delta. Reports the layers whose offset changed.
pub fn offset_layers(
    map: &mut Map,
    ids: &[LayerId],
    delta: DVec2,
    snap: impl Fn(DVec2) -> DVec2,
) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let wanted: HashSet<LayerId> = ids.iter().copied().collect();
    let mut moving: Vec<LayerId> = Vec::new();
    for id in layer_order(map) {
        let locked = map.effective_layer(id).is_none_or(|layer| layer.locked);
        if !wanted.contains(&id) || locked {
            continue;
        }
        // A group comes after its layers: those it holds move with it.
        if is_group(map, id) {
            moving.retain(|inner| !holds_or_is(map, id, *inner));
        }
        moving.push(id);
    }
    let mut report = LayersReport::default();
    for id in moving {
        let Some(layer) = map.layer_mut(id) else {
            continue;
        };
        let offset = snap(layer.offset + delta);
        if offset != layer.offset {
            layer.offset = offset;
            report.changed.push(id);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::nested;

    #[test]
    fn locked_layers_stay_and_nothing_moves_twice() {
        let mut map = nested();
        map.layer_mut(LayerId(4)).unwrap().locked = true;
        let delta = DVec2::new(5.0, -3.0);
        // The floor inside the walls, the crates inside the locked props, and the walls.
        let report = offset_layers(
            &mut map,
            &[LayerId(3), LayerId(5), LayerId(2), LayerId(6)],
            delta,
            |p| p,
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(2), LayerId(6)]);
        assert_eq!(map.layer(LayerId(2)).unwrap().offset, delta);
        assert_eq!(
            map.layer(LayerId(3)).unwrap().offset,
            DVec2::ZERO,
            "moved by its group"
        );
        assert_eq!(
            map.layer(LayerId(5)).unwrap().offset,
            DVec2::ZERO,
            "locked by props"
        );
        assert_eq!(
            offset_layers(&mut map, &[LayerId(7)], delta, |p| p),
            Err(EditError::NoSuchLayer(LayerId(7)))
        );
    }

    #[test]
    fn each_new_offset_is_snapped() {
        let mut map = nested();
        map.layer_mut(LayerId(1)).unwrap().offset = DVec2::new(3.0, 0.0);
        let geometry = map.geometry();
        let report = offset_layers(
            &mut map,
            &[LayerId(1), LayerId(6)],
            DVec2::new(10.0, 7.0),
            |p| geometry.snap_to_grid(p, 1),
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(6)]);
        assert_eq!(map.layer(LayerId(1)).unwrap().offset, DVec2::new(16.0, 0.0));
        assert_eq!(map.layer(LayerId(6)).unwrap().offset, DVec2::new(16.0, 0.0));
        let report = offset_layers(&mut map, &[LayerId(6)], DVec2::new(2.0, 2.0), |p| {
            geometry.snap_to_grid(p, 1)
        })
        .unwrap();
        assert!(report.is_empty(), "snapped back where it was");
    }
}
