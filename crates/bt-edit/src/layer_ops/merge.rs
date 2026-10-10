//! Merge Layer Down (Tiled's `MapDocument::mergeLayersDown`,
//! `reference/tiled/src/tiled/mapdocument.cpp`, with the rules of `Layer::canMergeDown`,
//! `TileLayer::mergedWith` and `ObjectGroup::mergedWith`, `libtiled/layer.cpp`,
//! `libtiled/tilelayer.cpp`, `libtiled/objectgroup.cpp`).

use std::sync::Arc;

use bt_core::{Layer, LayerId, LayerKind, Map, TileBounds};

use super::tree::{LayerLocation, check_layers, children, children_mut, distinct, locate};
use crate::error::EditError;
use crate::report::LayersReport;
use crate::tile_layers::rebounded_tile_layer;

/// Merge Layer Down: each layer that can goes into the layer just below it in its group, in the
/// order given. Tile goes over tile, the upper layer's tiles overwriting where it has some;
/// objects go over objects, appended with their ids. The merged layer is the lower one, with its
/// own name, id and attributes; the upper layer's are dropped. Image layers and groups do not
/// merge, nor does a layer onto another kind: those are skipped, as in Tiled. Reports the layers
/// merged into as changed and the merged ones as removed.
pub fn merge_layers_down(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let mut report = LayersReport::default();
    for id in distinct(ids) {
        // Merged into a layer listed before it, which removed it.
        if map.layer(id).is_none() {
            continue;
        }
        let Some(pair) = pair_below(map, id)?.filter(MergePair::can_merge) else {
            continue;
        };
        let at = pair.at;
        let merged = merged_layer(pair.lower, pair.upper)?;
        let lower = merged.id;
        let siblings = children_mut(map, at.parent)?;
        siblings.remove(at.index);
        if let Some(slot) = at.index.checked_sub(1).and_then(|i| siblings.get_mut(i)) {
            *slot = merged;
        }
        report.changed.push(lower);
        report.removed.push(id);
    }
    report.changed = distinct(&report.changed);
    Ok(report)
}

/// Whether the layer has a layer of its own kind just below it in its group, tile layers and
/// object layers only (Tiled's `Layer::canMergeDown`).
pub fn can_merge_down(map: &Map, id: LayerId) -> Result<bool, EditError> {
    Ok(pair_below(map, id)?.is_some_and(|pair| pair.can_merge()))
}

/// A layer and the layer just below it in its group.
struct MergePair<'m> {
    upper: &'m Layer,
    lower: &'m Layer,
    /// Where the upper layer is.
    at: LayerLocation,
}

impl MergePair<'_> {
    /// Whether the two are both tile layers or both object layers.
    fn can_merge(&self) -> bool {
        matches!(
            (&self.lower.kind, &self.upper.kind),
            (LayerKind::Tile(_), LayerKind::Tile(_)) | (LayerKind::Object(_), LayerKind::Object(_))
        )
    }
}

/// The layer with `id` and the one below it; `None` for the bottom layer of its group.
fn pair_below(map: &Map, id: LayerId) -> Result<Option<MergePair<'_>>, EditError> {
    let at = locate(map, id)?;
    let siblings = children(map, at.parent)?;
    let Some(upper) = siblings.get(at.index) else {
        return Ok(None);
    };
    let Some(lower) = at.index.checked_sub(1).and_then(|i| siblings.get(i)) else {
        return Ok(None);
    };
    Ok(Some(MergePair { upper, lower, at }))
}

/// The lower layer with the upper one's content on top. A finite lower layer smaller than the
/// upper one grows to hold it (Tiled unites their rectangles).
fn merged_layer(lower: &Layer, upper: &Layer) -> Result<Layer, EditError> {
    let mut merged = lower.clone();
    match (&mut merged.kind, &upper.kind) {
        (LayerKind::Tile(below), LayerKind::Tile(above)) => {
            if below.bounds() == TileBounds::Finite && !above.size().cmple(below.size()).all() {
                let size = below.size().max(above.size());
                *below = rebounded_tile_layer(below, size, TileBounds::Finite)?;
            }
            for placed in above.cells() {
                below.set_cell(placed.at, placed.cell)?;
            }
        }
        (LayerKind::Object(below), LayerKind::Object(above)) => {
            Arc::make_mut(&mut below.objects).extend(above.objects.iter().cloned());
        }
        _ => {}
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use bt_core::ObjectId;
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::IRect;
    use glam::{DVec2, UVec2};

    use super::*;
    use crate::testutil::{grid_of, group, nested, rect_object, tile_layer, tree, trimmed};

    fn layered() -> Map {
        let atlas = bt_test_data::fixtures::Atlas::builder()
            .build_embedded()
            .unwrap();
        let lower = Layer {
            opacity: 0.5,
            locked: true,
            ..tile_layer("lower", "1 1 .\n. 1 1")
        };
        let upper = Layer {
            offset: DVec2::new(5.0, 5.0),
            ..tile_layer("upper", "2 . 2\n. . 2")
        };
        MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(atlas.tileset)
            .layer(lower)
            .layer(upper)
            .object_layer("below", vec![rect_object(0.0, 0.0, 4.0, 4.0)])
            .object_layer("above", vec![rect_object(8.0, 8.0, 4.0, 4.0)])
            .image_layer("picture", bt_core::ImageLayer::default())
            .layer(group("empty", Vec::new()))
            .build()
            .unwrap()
    }

    #[test]
    fn tiles_overwrite_and_the_lower_layer_keeps_its_attributes() {
        let mut map = layered();
        let report = merge_layers_down(&mut map, &[LayerId(2)]).unwrap();
        assert_eq!(report.changed, [LayerId(1)]);
        assert_eq!(report.removed, [LayerId(2)]);
        assert_eq!(tree(&map), "1:lower 3:below 4:above 5:picture 6:empty[]");
        let lower = map.layer(LayerId(1)).unwrap();
        assert_eq!((lower.opacity, lower.locked), (0.5, true));
        assert_eq!(
            lower.offset,
            DVec2::ZERO,
            "the upper layer's offset is dropped"
        );
        let all = IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
        };
        assert_eq!(
            grid_of(lower.as_tile_layer().unwrap(), all),
            trimmed("2 1 2\n. 1 2")
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn objects_append_with_their_ids() {
        let mut map = layered();
        merge_layers_down(&mut map, &[LayerId(4)]).unwrap();
        let objects = &map
            .layer(LayerId(3))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(
            objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            [ObjectId(1), ObjectId(2)]
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn layers_that_cannot_merge_are_skipped() {
        let mut map = layered();
        // The object layer onto a tile layer, the image layer and the group: nothing.
        let report = merge_layers_down(&mut map, &[LayerId(3), LayerId(5), LayerId(6)]).unwrap();
        assert!(report.is_empty());
        assert_eq!(can_merge_down(&map, LayerId(1)), Ok(false), "nothing below");
        assert_eq!(
            merge_layers_down(&mut map, &[LayerId(9)]),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        // Inside a group, only with the layer below in that group.
        let mut map = nested();
        assert_eq!(can_merge_down(&map, LayerId(3)), Ok(false));
        assert!(
            merge_layers_down(&mut map, &[LayerId(3)])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn chains_merge_into_one() {
        let mut map = layered();
        let extra = tile_layer("extra", ". . 3\n3 . .");
        let at = locate(&map, LayerId(2)).unwrap();
        crate::layer_ops::tree::insert_layer(
            &mut map,
            crate::layer_ops::LayerLocation {
                parent: at.parent,
                index: at.index + 1,
            },
            Layer {
                id: LayerId(7),
                ..extra
            },
        )
        .unwrap();
        map.next_layer_id = 8;
        let report = merge_layers_down(&mut map, &[LayerId(7), LayerId(2)]).unwrap();
        assert_eq!(report.changed, [LayerId(2), LayerId(1)]);
        assert_eq!(report.removed, [LayerId(7), LayerId(2)]);
        let all = IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
        };
        assert_eq!(
            grid_of(map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap(), all),
            trimmed("2 1 3\n3 1 2")
        );
        assert_eq!(map.validate(), []);
    }
}
