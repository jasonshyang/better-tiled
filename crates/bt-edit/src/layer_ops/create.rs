//! New and duplicated layers, with Tiled's names for them (`MapDocument::addLayer`,
//! `newLayerName` and `duplicateLayers`, `reference/tiled/src/tiled/mapdocument.cpp`;
//! `Editor::nameOfDuplicate`, `tiled/editor.cpp`; ids as `ObjectReferencesHelper` gives them,
//! `tiled/objectreferenceshelper.cpp`).

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{ImageLayer, Layer, LayerId, LayerKind, Map, ObjectLayer};

use super::tree::{LayerLocation, check_layers, holds_or_is, in_layer_order, insert_layer, locate};
use crate::duplicate_name::name_of_duplicate;
use crate::error::EditError;
use crate::ids::{IdRemap, take_layer_id, take_object_id};
use crate::report::LayersReport;
use crate::tile_layers::blank_tile_layer;

/// The kinds of layer a map holds, for making new ones.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LayerType {
    /// Cells.
    Tile,
    /// Objects.
    Object,
    /// An image.
    Image,
    /// Other layers.
    Group,
}

impl LayerType {
    /// The kind of `layer`.
    pub fn of(layer: &Layer) -> LayerType {
        match layer.kind {
            LayerKind::Tile(_) => LayerType::Tile,
            LayerKind::Object(_) => LayerType::Object,
            LayerKind::Image(_) => LayerType::Image,
            LayerKind::Group(_) => LayerType::Group,
        }
    }

    /// The name new layers of the kind are numbered after.
    fn base_name(self) -> &'static str {
        match self {
            LayerType::Tile => "Tile Layer",
            LayerType::Object => "Object Layer",
            LayerType::Image => "Image Layer",
            LayerType::Group => "Group Layer",
        }
    }

    /// An empty layer of the kind for `map`: a tile layer of the map's size and bounds.
    fn empty(self, map: &Map) -> LayerKind {
        match self {
            LayerType::Tile => LayerKind::Tile(blank_tile_layer(map)),
            LayerType::Object => LayerKind::Object(ObjectLayer::default()),
            LayerType::Image => LayerKind::Image(ImageLayer::default()),
            LayerType::Group => LayerKind::Group(Vec::new()),
        }
    }
}

/// New Layer: an empty layer of `kind` right above `above`, in its group; with `None`, at the
/// bottom of the map, where Tiled puts it when no layer is current. It is named after its kind
/// and numbered after the layers of that kind ("Tile Layer 3" when there are two), skipping
/// names in use, as in Tiled; a tile layer takes the map's size and bounds. Returns its id.
pub fn new_layer(
    map: &mut Map,
    kind: LayerType,
    above: Option<LayerId>,
) -> Result<LayerId, EditError> {
    let at = match above {
        Some(id) => {
            let below = locate(map, id)?;
            LayerLocation {
                parent: below.parent,
                index: below.index + 1,
            }
        }
        None => LayerLocation {
            parent: None,
            index: 0,
        },
    };
    let name = numbered_name(map, kind.base_name(), |layer| LayerType::of(layer) == kind);
    let id = take_layer_id(map)?;
    let layer = Layer::new(id, name, kind.empty(map));
    insert_layer(map, at, layer)?;
    Ok(id)
}

/// `base` numbered after the layers `counted` picks: one more than their number, or the first
/// number after that whose name no layer has (Tiled's `newLayerName`).
pub(crate) fn numbered_name(map: &Map, base: &str, counted: impl Fn(&Layer) -> bool) -> String {
    let mut names = HashSet::new();
    let mut number: u64 = 0;
    map.for_each_layer(|layer, _| {
        if counted(layer) {
            number += 1;
            names.insert(layer.name.as_str());
        }
    });
    loop {
        number += 1;
        let name = format!("{base} {number}");
        if !names.contains(name.as_str()) {
            return name;
        }
    }
}

/// A layer copied by [`duplicate_layers`], before it is put in the tree.
struct Duplicate {
    original: LayerId,
    copy: Layer,
}

/// Duplicate Layers: copies of the layers, groups with everything in them, each with new layer
/// and object ids (taken in Tiled's order) and with object references among the copied objects
/// pointing at the copies. The copies of layers in one group go together above the topmost of
/// them, in their order, as in Tiled. With `copy_suffix` (the editor's "duplicate adds Copy")
/// each copy's name gets Tiled's "Copy" suffix; a group's layers keep their names. Reports the
/// copies made, in the order they went in.
pub fn duplicate_layers(
    map: &mut Map,
    ids: &[LayerId],
    copy_suffix: bool,
) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    // From the top down, a group before its layers, so a copied group takes them with it.
    let mut originals: Vec<LayerId> = in_layer_order(map, ids);
    originals.reverse();
    let mut remap = IdRemap::new();
    let mut duplicates: Vec<Duplicate> = Vec::new();
    for original in originals {
        if duplicates
            .iter()
            .any(|done| holds_or_is(map, done.original, original))
        {
            continue;
        }
        let Some(layer) = map.layer(original) else {
            return Err(EditError::NoSuchLayer(original));
        };
        let mut copy = layer.clone();
        give_new_ids(map, &mut copy, &mut remap)?;
        if copy_suffix {
            copy.name = name_of_duplicate(&copy.name);
        }
        duplicates.push(Duplicate { original, copy });
    }
    for duplicate in &mut duplicates {
        rewire_objects(&mut duplicate.copy, &remap);
    }

    let mut report = LayersReport::default();
    let mut previous: Option<LayerLocation> = None;
    for Duplicate { original, copy } in duplicates {
        let at = locate(map, original)?;
        // Copies of layers of one group go where the previous copy went, pushing it up.
        let at = match previous {
            Some(last) if last.parent == at.parent => last,
            _ => LayerLocation {
                parent: at.parent,
                index: at.index + 1,
            },
        };
        report.created.push(copy.id);
        insert_layer(map, at, copy)?;
        previous = Some(at);
    }
    Ok(report)
}

/// Gives `layer`, the layers in it and their objects new ids from `map`, recording each
/// object's copy in `remap`: the layer first, then its objects or its layers in order.
fn give_new_ids(map: &mut Map, layer: &mut Layer, remap: &mut IdRemap) -> Result<(), EditError> {
    layer.id = take_layer_id(map)?;
    match &mut layer.kind {
        LayerKind::Object(objects) => {
            for object in Arc::make_mut(&mut objects.objects) {
                let id = take_object_id(map)?;
                remap.record(object.id, id);
                object.id = id;
            }
        }
        LayerKind::Group(layers) => {
            for child in layers {
                give_new_ids(map, child, remap)?;
            }
        }
        LayerKind::Tile(_) | LayerKind::Image(_) => {}
    }
    Ok(())
}

/// Points the object references of the objects in `layer` (and in its layers) at the copies
/// `remap` knows.
fn rewire_objects(layer: &mut Layer, remap: &IdRemap) {
    match &mut layer.kind {
        LayerKind::Object(objects) => {
            for object in Arc::make_mut(&mut objects.objects) {
                remap.rewire_properties(&mut object.properties);
            }
        }
        LayerKind::Group(layers) => {
            for child in layers {
                rewire_objects(child, remap);
            }
        }
        LayerKind::Tile(_) | LayerKind::Image(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{ObjectId, Value};

    use super::*;
    use crate::testutil::{nested, tree};

    #[test]
    fn new_layers_are_numbered_after_their_kind() {
        let mut map = nested();
        let tile = new_layer(&mut map, LayerType::Tile, Some(LayerId(3))).unwrap();
        assert_eq!(tile, LayerId(7));
        // Two tile layers already: the third is "Tile Layer 3", above the floor in its group.
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 7:Tile Layer 3 4:props[5:crates]] 6:sky"
        );
        let layer = map.layer(tile).unwrap().as_tile_layer().unwrap();
        assert_eq!(layer.size(), glam::UVec2::new(4, 3));
        assert!(!layer.is_infinite());
        new_layer(&mut map, LayerType::Tile, Some(LayerId(6))).unwrap();
        assert_eq!(map.layer(LayerId(8)).unwrap().name, "Tile Layer 4");
        // A name an object layer has is skipped; other kinds' names are not looked at.
        map.layer_mut(LayerId(5)).unwrap().name = "Object Layer 2".into();
        map.layer_mut(LayerId(1)).unwrap().name = "Object Layer 3".into();
        new_layer(&mut map, LayerType::Object, None).unwrap();
        assert_eq!(map.layer(LayerId(9)).unwrap().name, "Object Layer 3");
        assert_eq!(
            map.layers[0].id,
            LayerId(9),
            "without a layer, at the bottom"
        );
        new_layer(&mut map, LayerType::Group, Some(LayerId(5))).unwrap();
        assert_eq!(map.layer(LayerId(10)).unwrap().name, "Group Layer 3");
        new_layer(&mut map, LayerType::Image, Some(LayerId(10))).unwrap();
        assert_eq!(map.layer(LayerId(11)).unwrap().name, "Image Layer 2");
        assert_eq!(map.validate(), []);
        assert_eq!(
            new_layer(&mut map, LayerType::Tile, Some(LayerId(99))),
            Err(EditError::NoSuchLayer(LayerId(99)))
        );
    }

    #[test]
    fn names_of_duplicates_follow_tileds_pattern() {
        assert_eq!(name_of_duplicate("Walls"), "Walls Copy");
        assert_eq!(name_of_duplicate("Walls Copy"), "Walls Copy 2");
        assert_eq!(name_of_duplicate("Walls Copy 2"), "Walls Copy 3");
        assert_eq!(name_of_duplicate("Walls Copy  9"), "Walls Copy 10");
        assert_eq!(
            name_of_duplicate("WallsCopy"),
            "Walls Copy 2",
            "no space needed"
        );
        assert_eq!(name_of_duplicate("Copycat"), "Copycat Copy");
        assert_eq!(name_of_duplicate("Copy"), "Copy 2");
        assert_eq!(name_of_duplicate(""), "");
    }

    #[test]
    fn duplicates_take_new_ids_throughout_and_rewire_their_references() {
        let mut map = nested();
        let report = duplicate_layers(&mut map, &[LayerId(2)], true).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates]] \
             7:walls Copy[8:floor 9:props[10:crates]] 6:sky",
            "the group's layers keep their names"
        );
        let copies = &map
            .layer(LayerId(10))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(
            copies.iter().map(|o| o.id).collect::<Vec<_>>(),
            [ObjectId(3), ObjectId(4)]
        );
        assert_eq!(
            copies[0].properties["next"].value,
            Value::Object(ObjectId(4)),
            "the copy points at the copy"
        );
        let originals = &map
            .layer(LayerId(5))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(
            originals[0].properties["next"].value,
            Value::Object(ObjectId(2))
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn duplicates_of_one_group_go_together_above_the_topmost() {
        let mut map = nested();
        // The group and a layer inside it: the group's copy holds the layer already.
        let report =
            duplicate_layers(&mut map, &[LayerId(3), LayerId(4), LayerId(2)], false).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        let mut map = nested();
        let report = duplicate_layers(&mut map, &[LayerId(1), LayerId(6)], false).unwrap();
        assert_eq!(report.created, [LayerId(7), LayerId(8)]);
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates]] 6:sky 8:ground 7:sky",
            "copies above the topmost, in the originals' order"
        );
        let mut map = nested();
        duplicate_layers(&mut map, &[LayerId(3), LayerId(5)], false).unwrap();
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 8:floor 4:props[5:crates 7:crates]] 6:sky",
            "each copy stays in its group, above its original"
        );
        assert_eq!(map.validate(), []);
        assert_eq!(
            duplicate_layers(&mut map, &[LayerId(1), LayerId(42)], false),
            Err(EditError::NoSuchLayer(LayerId(42)))
        );
    }
}
