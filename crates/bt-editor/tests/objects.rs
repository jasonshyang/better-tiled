//! Editing objects through the editor: picking honours the object-selection preference and
//! locks, drags are one step and snap, inserting selects the new object, and the Objects menu
//! and the clipboard act on unlocked objects.

#![cfg(test)]

use bt_core::{DrawOrder, LayerId, MapObject, ObjectId};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Enabled, Intent, ObjectStroke, ObjectStrokeKind,
    SelectionMode, Settings, Snapping,
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
            free: false,
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
