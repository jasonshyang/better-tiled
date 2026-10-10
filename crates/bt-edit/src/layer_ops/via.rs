//! Layer via Copy and Layer via Cut: the selected tiles or objects into new layers (Tiled's
//! `MapDocumentActionHandler::layerVia`, `reference/tiled/src/tiled/mapdocumentactionhandler.cpp`,
//! for the current layer; here for every layer given).

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{Cell, Layer, LayerId, LayerKind, Map, MapObject, ObjectId, ObjectLayer};

use super::tree::{LayerLocation, check_layers, in_layer_order, insert_layer, layer_order, locate};
use crate::error::EditError;
use crate::ids::{IdRemap, take_layer_id, take_object_id};
use crate::region::TileRegion;
use crate::report::LayersReport;
use crate::tile_layers::blank_tile_layer;

/// What Layer via Copy and Layer via Cut take into new layers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayerViaSource<'a> {
    /// The cells of `region` on each of `layers`, which are tile layers.
    Tiles {
        /// The tile layers.
        layers: &'a [LayerId],
        /// The cells.
        region: &'a TileRegion,
    },
    /// These objects, on whichever layers hold them.
    Objects(&'a [ObjectId]),
}

/// Copy or cut.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Via {
    Copy,
    Cut,
}

impl Via {
    /// Tiled's name for the layers made.
    fn layer_name(self) -> &'static str {
        match self {
            Via::Copy => "Layer via Copy",
            Via::Cut => "Layer via Cut",
        }
    }
}

/// Layer via Copy: for each tile layer given, a new tile layer "Layer via Copy" just above it
/// holding its tiles in the region; for objects, a new object layer above each layer holding
/// some, with copies of them (new ids, references among the copies pointing at the copies) and
/// the layer's draw order and colour. New layers have the map's size and default attributes, as
/// in Tiled. Locks are the editor's to weigh. Reports the layers made; nothing for an empty
/// region or no objects.
pub fn layer_via_copy(
    map: &mut Map,
    source: LayerViaSource<'_>,
) -> Result<LayersReport, EditError> {
    layer_via(map, source, Via::Copy)
}

/// Layer via Cut: [`layer_via_copy`], and the tiles are erased from their layers, the objects
/// taken from theirs (keeping their ids). Reports the layers made, and the layers cut from as
/// changed.
pub fn layer_via_cut(map: &mut Map, source: LayerViaSource<'_>) -> Result<LayersReport, EditError> {
    layer_via(map, source, Via::Cut)
}

fn layer_via(
    map: &mut Map,
    source: LayerViaSource<'_>,
    via: Via,
) -> Result<LayersReport, EditError> {
    match source {
        LayerViaSource::Tiles { layers, region } => tiles_via(map, layers, region, via),
        LayerViaSource::Objects(ids) => objects_via(map, ids, via),
    }
}

fn tiles_via(
    map: &mut Map,
    layers: &[LayerId],
    region: &TileRegion,
    via: Via,
) -> Result<LayersReport, EditError> {
    check_layers(map, layers)?;
    if let Some(other) = layers
        .iter()
        .find(|id| map.layer(**id).is_some_and(|l| l.as_tile_layer().is_none()))
    {
        return Err(EditError::NotATileLayer(*other));
    }
    let mut report = LayersReport::default();
    if region.is_empty() {
        return Ok(report);
    }
    for source in in_layer_order(map, layers) {
        let mut tiles = blank_tile_layer(map);
        let Some(cells) = map.layer(source).and_then(Layer::as_tile_layer) else {
            return Err(EditError::NotATileLayer(source));
        };
        let taken: Vec<_> = cells
            .cells()
            .filter(|placed| region.contains(placed.at))
            .collect();
        for placed in &taken {
            tiles.set_cell(placed.at, placed.cell)?;
        }
        if via == Via::Cut && !taken.is_empty() {
            if let Some(cells) = map.layer_mut(source).and_then(Layer::as_tile_layer_mut) {
                for placed in &taken {
                    cells.set_cell(placed.at, Cell::EMPTY)?;
                }
                cells.prune_empty_chunks();
            }
            report.changed.push(source);
        }
        let made = add_above(map, source, via, LayerKind::Tile(tiles))?;
        report.created.push(made);
    }
    Ok(report)
}

fn objects_via(map: &mut Map, ids: &[ObjectId], via: Via) -> Result<LayersReport, EditError> {
    if let Some(missing) = ids.iter().find(|id| map.object(**id).is_none()) {
        return Err(EditError::NoSuchObject(*missing));
    }
    let wanted: HashSet<ObjectId> = ids.iter().copied().collect();
    // The objects of each layer holding some, in layer order and in list order.
    let mut taken: Vec<SourcedObjects> = Vec::new();
    for source in layer_order(map) {
        let Some(layer) = map.layer(source).and_then(Layer::as_object_layer) else {
            continue;
        };
        let objects: Vec<MapObject> = layer
            .objects
            .iter()
            .filter(|o| wanted.contains(&o.id))
            .cloned()
            .collect();
        if !objects.is_empty() {
            taken.push(SourcedObjects { source, objects });
        }
    }
    if via == Via::Copy {
        // Every copy has its id before any reference is pointed at one.
        let mut remap = IdRemap::new();
        for object in taken.iter_mut().flat_map(|t| t.objects.iter_mut()) {
            let id = take_object_id(map)?;
            remap.record(object.id, id);
            object.id = id;
        }
        for object in taken.iter_mut().flat_map(|t| t.objects.iter_mut()) {
            remap.rewire_properties(&mut object.properties);
        }
    }
    let mut report = LayersReport::default();
    for SourcedObjects { source, objects } in taken {
        let Some(from) = map.layer_mut(source).and_then(Layer::as_object_layer_mut) else {
            return Err(EditError::NotAnObjectLayer(source));
        };
        let layer = ObjectLayer {
            color: from.color,
            draw_order: from.draw_order,
            objects: Arc::new(objects),
        };
        if via == Via::Cut {
            Arc::make_mut(&mut from.objects).retain(|o| !wanted.contains(&o.id));
            report.changed.push(source);
        }
        let made = add_above(map, source, via, LayerKind::Object(layer))?;
        report.created.push(made);
    }
    Ok(report)
}

/// Objects taken from one layer.
struct SourcedObjects {
    source: LayerId,
    objects: Vec<MapObject>,
}

/// Adds a layer named for `via` holding `kind` just above `source`; returns its id.
fn add_above(
    map: &mut Map,
    source: LayerId,
    via: Via,
    kind: LayerKind,
) -> Result<LayerId, EditError> {
    let below = locate(map, source)?;
    let id = take_layer_id(map)?;
    let at = LayerLocation {
        parent: below.parent,
        index: below.index + 1,
    };
    insert_layer(map, at, Layer::new(id, via.layer_name(), kind))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use bt_core::Value;
    use bt_utils::IRect;

    use super::*;
    use crate::testutil::{drawn, grid_of, nested, tree, trimmed};

    fn cells(map: &Map, id: LayerId) -> String {
        let all = IRect {
            x: 0,
            y: 0,
            w: 4,
            h: 3,
        };
        grid_of(map.layer(id).unwrap().as_tile_layer().unwrap(), all)
    }

    fn region() -> TileRegion {
        drawn(
            "
            ##..
            ....
            ..#.
            ",
        )
    }

    #[test]
    fn tiles_via_copy_go_above_each_layer() {
        let mut map = nested();
        let region = region();
        let source = LayerViaSource::Tiles {
            layers: &[LayerId(3), LayerId(1)],
            region: &region,
        };
        let report = layer_via_copy(&mut map, source).unwrap();
        assert_eq!(report.created, [LayerId(7), LayerId(8)]);
        assert!(report.changed.is_empty());
        assert_eq!(
            tree(&map),
            "1:ground 7:Layer via Copy 2:walls[3:floor 8:Layer via Copy 4:props[5:crates]] 6:sky"
        );
        assert_eq!(
            cells(&map, LayerId(7)),
            trimmed("2 2 . .\n. . . .\n. . 3 .")
        );
        assert_eq!(
            cells(&map, LayerId(8)),
            trimmed("1 1 . .\n. . . .\n. . 1 .")
        );
        assert_eq!(
            cells(&map, LayerId(1)),
            trimmed("2 2 . .\n. . . .\n. . 3 3")
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn tiles_via_cut_leave_the_rest() {
        let mut map = nested();
        let region = region();
        let source = LayerViaSource::Tiles {
            layers: &[LayerId(1)],
            region: &region,
        };
        let report = layer_via_cut(&mut map, source).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        assert_eq!(report.changed, [LayerId(1)]);
        assert_eq!(map.layer(LayerId(7)).unwrap().name, "Layer via Cut");
        assert_eq!(
            cells(&map, LayerId(1)),
            trimmed(". . . .\n. . . .\n. . . 3")
        );
        assert_eq!(
            cells(&map, LayerId(7)),
            trimmed("2 2 . .\n. . . .\n. . 3 .")
        );
        let nothing = TileRegion::new();
        let source = LayerViaSource::Tiles {
            layers: &[LayerId(1)],
            region: &nothing,
        };
        assert!(layer_via_cut(&mut map, source).unwrap().is_empty());
        let source = LayerViaSource::Tiles {
            layers: &[LayerId(5)],
            region: &nothing,
        };
        assert_eq!(
            layer_via_copy(&mut map, source),
            Err(EditError::NotATileLayer(LayerId(5)))
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn objects_via_copy_get_new_ids_and_their_references_follow() {
        let mut map = nested();
        let objects = [ObjectId(2), ObjectId(1)];
        let report = layer_via_copy(&mut map, LayerViaSource::Objects(&objects)).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates 7:Layer via Copy]] 6:sky"
        );
        let copies = &map
            .layer(LayerId(7))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(
            copies.iter().map(|o| o.id).collect::<Vec<_>>(),
            [ObjectId(3), ObjectId(4)],
            "in the layer's order"
        );
        assert_eq!(
            copies[0].properties["next"].value,
            Value::Object(ObjectId(4))
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn objects_via_cut_keep_their_ids() {
        let mut map = nested();
        let report = layer_via_cut(&mut map, LayerViaSource::Objects(&[ObjectId(2)])).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        assert_eq!(report.changed, [LayerId(5)]);
        let moved = &map
            .layer(LayerId(7))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(
            moved.iter().map(|o| o.id).collect::<Vec<_>>(),
            [ObjectId(2)]
        );
        let left = &map
            .layer(LayerId(5))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(left.iter().map(|o| o.id).collect::<Vec<_>>(), [ObjectId(1)]);
        assert_eq!(
            layer_via_cut(&mut map, LayerViaSource::Objects(&[ObjectId(9)])),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
        assert!(
            layer_via_cut(&mut map, LayerViaSource::Objects(&[]))
                .unwrap()
                .is_empty()
        );
        assert_eq!(map.validate(), []);
    }
}
