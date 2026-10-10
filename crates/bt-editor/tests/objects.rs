//! Editing objects through the editor: picking honours the object-selection preference and
//! locks, drags are one step and snap, inserting selects the new object, and the Objects menu
//! and the clipboard act on unlocked objects.

#![cfg(test)]

use bt_core::{DrawOrder, LayerId, MapObject, ObjectId};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Enabled, Intent, ObjectRow, ObjectStroke,
    ObjectStrokeKind, SelectionMode, Settings, Snapping, ViewRequest,
};
use bt_test_data::fixtures::MapBuilder;
use glam::{DVec2, UVec2};

fn rect(x: f64, y: f64) -> MapObject {
    MapObject {
        pos: DVec2::new(x, y),
        size: DVec2::new(16.0, 16.0),
        ..MapObject::default()
    }
}

/// A 10×10 map of 16-pixel cells: "a" with objects 1 and 2, "b" with object 3, both object
/// layers; "a" current.
fn editor() -> (Editor, DocumentId) {
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
        .object_layer("a", vec![rect(0.0, 0.0), rect(32.0, 0.0)])
        .object_layer("b", vec![rect(0.0, 0.0)])
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    editor.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(1),
    });
    (editor, doc)
}

fn object(editor: &Editor, doc: DocumentId, id: u32) -> MapObject {
    editor
        .map_of(doc)
        .unwrap()
        .object(ObjectId(id))
        .unwrap()
        .clone()
}

fn select(editor: &mut Editor, doc: DocumentId, ids: &[u32]) {
    editor.apply(Intent::SelectObjects {
        doc,
        ids: ids.iter().map(|id| ObjectId(*id)).collect(),
        mode: SelectionMode::Replace,
    });
}

#[test]
fn picking_prefers_the_current_layer_and_skips_locked_ones() {
    let (mut editor, doc) = editor();
    let at = DVec2::new(8.0, 8.0);
    assert_eq!(
        editor.objects_at(doc, at, 2.0),
        [ObjectId(1), ObjectId(3)],
        "the current layer's object first"
    );
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: true,
    });
    assert_eq!(editor.objects_at(doc, at, 2.0), [ObjectId(3)]);
    let band = bt_utils::DRect::from_min_size(DVec2::new(-4.0, -4.0), DVec2::new(60.0, 30.0));
    assert_eq!(editor.objects_in_rect(doc, band), [ObjectId(3)]);
}

#[test]
fn a_drag_moves_from_where_it_started_snaps_and_is_one_step() {
    let (mut editor, doc) = editor();
    let mut settings = Settings::default();
    settings.behaviour.snapping = Snapping::Grid;
    editor.apply(Intent::ChangeSettings(Box::new(settings)));
    select(&mut editor, doc, &[1, 2]);
    editor.apply(Intent::ObjectStroke(ObjectStroke::Begin {
        doc,
        kind: ObjectStrokeKind::Move,
    }));
    for x in [3.0, 9.0, 21.0] {
        editor.apply(Intent::ObjectStroke(ObjectStroke::Move {
            delta: DVec2::new(x, 5.0),
            snap: bt_editor::SnapToggle::Keep,
        }));
    }
    editor.apply(Intent::ObjectStroke(ObjectStroke::End));
    // 21 snaps the anchor to 16 and 5 to 0: both objects move as far.
    assert_eq!(object(&editor, doc, 1).pos, DVec2::new(16.0, 0.0));
    assert_eq!(object(&editor, doc, 2).pos, DVec2::new(48.0, 0.0));
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(object(&editor, doc, 1).pos, DVec2::ZERO, "one step");
}

#[test]
fn inserting_selects_the_new_object_and_needs_an_object_layer() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::InsertObject {
        doc,
        object: Box::new(rect(64.0, 64.0)),
    });
    let selected = editor.document(doc).unwrap().selection().objects.clone();
    assert_eq!(selected, [ObjectId(4)]);
    assert_eq!(object(&editor, doc, 4).pos, DVec2::new(64.0, 64.0));
}

#[test]
fn the_objects_menu_and_clipboard_act_on_unlocked_objects() {
    let (mut editor, doc) = editor();
    select(&mut editor, doc, &[1, 3]);
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(2),
        locked: true,
    });
    let effects = editor.apply(Intent::Run(ActionId::Delete));
    assert!(editor.map_of(doc).unwrap().object(ObjectId(1)).is_none());
    assert!(editor.map_of(doc).unwrap().object(ObjectId(3)).is_some());
    assert!(
        matches!(&effects[..], [Effect::Notify(n)] if n.text == "“b” is locked: its objects were left as they were"),
        "{effects:?}"
    );
    editor.apply(Intent::Run(ActionId::Undo));

    select(&mut editor, doc, &[2]);
    editor.apply(Intent::Run(ActionId::Copy));
    let effects = editor.apply(Intent::Run(ActionId::Paste));
    assert!(
        matches!(effects[..], [Effect::PasteAtPointer(d)] if d == doc),
        "{effects:?}"
    );
    editor.apply(Intent::PasteObjects {
        doc,
        at: Some(DVec2::new(100.0, 100.0)),
    });
    let pasted = editor.document(doc).unwrap().selection().objects.clone();
    assert_eq!(pasted.len(), 1);
    let copy = object(&editor, doc, pasted[0].0);
    assert_eq!(
        copy.pos + copy.size / 2.0,
        DVec2::new(100.0, 100.0),
        "centred on the pointer"
    );

    editor.apply(Intent::Run(ActionId::DuplicateObjects));
    let copies = editor.document(doc).unwrap().selection().objects.clone();
    assert_ne!(copies, pasted, "the copies are selected");

    // Raise and Lower need a layer drawn in list order.
    assert!(matches!(
        editor.enabled(ActionId::RaiseObjects),
        Enabled::Disabled(Some(_))
    ));
    let _ = DrawOrder::Index;

    editor.apply(Intent::Run(ActionId::FlipHorizontal));
    editor.apply(Intent::MoveObjectsToLayer {
        doc,
        layer: LayerId(2),
    });
    let effects = editor.apply(Intent::MoveObjectsToLayer {
        doc,
        layer: LayerId(2),
    });
    assert!(!effects.is_empty(), "a locked target says so");
}

#[test]
fn objects_of_one_layer_make_it_current_when_selected() {
    let (mut editor, doc) = editor();
    let current = |editor: &Editor| editor.document(doc).unwrap().selection().clone();
    select(&mut editor, doc, &[3]);
    assert_eq!(current(&editor).current_layer, Some(LayerId(2)));
    assert_eq!(current(&editor).layers, [LayerId(2)]);
    select(&mut editor, doc, &[1, 3]);
    assert_eq!(
        current(&editor).current_layer,
        Some(LayerId(2)),
        "two layers: it stays"
    );
    // A layer among the selected ones becomes current and they stay selected.
    editor.apply(Intent::SelectLayers {
        doc,
        current: LayerId(2),
        layers: vec![LayerId(1), LayerId(2)],
    });
    select(&mut editor, doc, &[2]);
    assert_eq!(current(&editor).current_layer, Some(LayerId(1)));
    assert_eq!(current(&editor).layers, [LayerId(1), LayerId(2)]);
}

#[test]
fn the_objects_panel_shows_hides_and_centres_on_objects() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::SetObjectVisible {
        doc,
        object: ObjectId(2),
        visible: false,
    });
    assert!(!object(&editor, doc, 2).visible);
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(object(&editor, doc, 2).visible, "one step");
    let effects = editor.apply(Intent::CenterOnObject {
        doc,
        object: ObjectId(2),
    });
    assert!(
        matches!(effects[..], [Effect::View(ViewRequest::CenterOn(at))] if at == DVec2::new(40.0, 8.0)),
        "{effects:?}"
    );
    let effects = editor.apply(Intent::CenterOnObject {
        doc,
        object: ObjectId(9),
    });
    assert!(matches!(effects[..], [Effect::Notify(_)]), "{effects:?}");
}

/// “a” drawn in list order with objects 1 and 2, “b” drawn top-down with object 3.
fn ordered_editor() -> (Editor, DocumentId) {
    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
        .object_layer("a", vec![rect(0.0, 0.0), rect(32.0, 0.0)])
        .object_layer("b", vec![rect(0.0, 32.0)])
        .build()
        .unwrap();
    if let bt_core::LayerKind::Object(objects) = &mut map.layers[0].kind {
        objects.draw_order = DrawOrder::Index;
    }
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    (editor, doc)
}

fn list(editor: &Editor, doc: DocumentId, layer: u32) -> Vec<u32> {
    let map = editor.map_of(doc).unwrap();
    let objects = map
        .layer(LayerId(layer))
        .unwrap()
        .as_object_layer()
        .unwrap();
    objects.objects.iter().map(|o| o.id.0).collect()
}

#[test]
fn objects_are_dragged_to_a_place_in_a_list_as_one_step() {
    let (mut editor, doc) = ordered_editor();
    let reorder = |editor: &mut Editor, objects: &[u32], layer: u32, index: usize| {
        editor.apply(Intent::ReorderObjects {
            doc,
            objects: objects.iter().map(|id| ObjectId(*id)).collect(),
            layer: LayerId(layer),
            index,
        })
    };
    reorder(&mut editor, &[1], 1, 2);
    assert_eq!(list(&editor, doc, 1), [2, 1]);
    reorder(&mut editor, &[3], 1, 0);
    assert_eq!(
        (list(&editor, doc, 1), list(&editor, doc, 2)),
        (vec![3, 2, 1], vec![])
    );
    let labels: Vec<String> = editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .map(|entry| entry.label)
        .collect();
    assert_eq!(labels, ["Reorder Objects", "Move Objects to Layer"]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        (list(&editor, doc, 1), list(&editor, doc, 2)),
        (vec![2, 1], vec![3])
    );

    // A layer drawn top-down is not reordered by hand, and a locked one takes nothing.
    let effects = reorder(&mut editor, &[3], 2, 1);
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: true,
    });
    let effects = reorder(&mut editor, &[3], 1, 0);
    assert!(matches!(&effects[..], [Effect::Notify(_)]), "{effects:?}");
    assert_eq!(
        (list(&editor, doc, 1), list(&editor, doc, 2)),
        (vec![2, 1], vec![3])
    );
}

#[test]
fn the_objects_panel_lists_layers_and_objects_topmost_first_and_filters_them() {
    use bt_core::{Layer, LayerKind, Shape};

    let named = |name: &str, class: &str| MapObject {
        name: name.to_owned(),
        class: class.to_owned(),
        ..rect(0.0, 0.0)
    };
    let mut inner = MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
        .object_layer("a", vec![named("oak", ""), named("pine", "Tree")])
        .build()
        .unwrap()
        .layers;
    inner[0].id = LayerId(0);
    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
        .layer(Layer::new(LayerId(0), "g", LayerKind::Group(inner)))
        .tile_layer("t", "")
        .object_layer(
            "b",
            vec![
                rect(0.0, 50.0),
                MapObject {
                    shape: Shape::Point,
                    ..rect(0.0, 10.0)
                },
            ],
        )
        .build()
        .unwrap();
    if let LayerKind::Group(children) = &mut map.layers[0].kind
        && let LayerKind::Object(objects) = &mut children[0].kind
    {
        objects.draw_order = DrawOrder::Index;
    }
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    let rows = |filter: &str| -> Vec<String> {
        editor
            .object_rows(doc, filter)
            .iter()
            .map(|row| match row {
                ObjectRow::Layer { layer, depth } => format!("{depth}:{}", layer.name),
                ObjectRow::Object {
                    object,
                    index,
                    depth,
                    ..
                } => format!("{depth}:#{}@{index}", object.id.0),
            })
            .collect()
    };
    // “b” draws top-down: the point higher up is drawn first, so it is listed last.
    assert_eq!(
        rows(""),
        ["0:b", "1:#3@0", "1:#4@1", "0:g", "1:a", "2:#2@1", "2:#1@0"]
    );
    assert_eq!(rows("TREE"), ["0:g", "1:a", "2:#2@1"], "by class");
    assert_eq!(rows("point"), ["0:b", "1:#4@1"], "by kind");
    assert_eq!(rows("oak"), ["0:g", "1:a", "2:#1@0"], "by name");
    assert_eq!(rows("3"), ["0:b", "1:#3@0"], "by id");
    assert!(rows("zzz").is_empty());
}
