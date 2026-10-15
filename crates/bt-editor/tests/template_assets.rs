//! Template views edit the template root and every instance in one transaction. Writers must
//! compare instances with the current definition, including after undo and redo.

#![cfg(test)]
mod common;

use bt_core::{MapObject, ObjectTemplate, Property, Value};
use bt_editor::{ActionId, Attribute, AttributeValue, Inspected, Intent};
use bt_test_data::Scratch;
use common::{apply, editor_in};
use glam::DVec2;
use std::sync::Arc;

fn write_fixture(scratch: &Scratch) {
    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "Base".into(),
            size: DVec2::new(16.0, 8.0),
            properties: bt_core::Properties::from([
                ("hp".into(), Property::new(Value::Int(5))),
                ("score".into(), Property::new(Value::Int(7))),
            ]),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &scratch.join("unit.tx"))
        .write()
        .unwrap();
    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="8" height="8" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="3">
 <objectgroup id="1" name="Objects">
  <object id="1" template="unit.tx" x="8" y="16"/>
  <object id="2" template="unit.tx" name="own" x="24" y="32" width="9" height="7"><properties><property name="hp" type="int" value="99"/></properties></object>
 </objectgroup>
</map>
"#;
    std::fs::write(scratch.join("a.tmx"), map).unwrap();
    std::fs::write(scratch.join("b.tmx"), map).unwrap();
}

#[test]
fn editing_a_template_updates_all_maps_atomically_and_preserves_overrides() {
    let scratch = Scratch::new("template-asset-cross-map").unwrap();
    write_fixture(&scratch);
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("a.tmx")));
    let first = editor.current().unwrap();
    apply(&mut editor, Intent::Open(scratch.join("b.tmx")));
    let second = editor.current().unwrap();
    apply(&mut editor, Intent::Open(scratch.join("unit.tx")));
    let doc = editor.current().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    assert!(editor.template_of(doc).is_some());
    assert_eq!(
        editor.inspected(doc),
        Some(Inspected::Objects(vec![bt_core::ObjectId(1)]))
    );
    editor
        .transact(doc, "Edit Template", |tx| {
            let object = tx.template_mut(asset)?.object.as_mut().unwrap();
            object.name = "Changed".into();
            object.size = DVec2::new(32.0, 24.0);
            object
                .properties
                .insert("hp".into(), Property::new(Value::Int(10)));
            object.properties.remove("score");
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    for map_doc in [first, second] {
        let map = editor.map_of(map_doc).unwrap();
        let inherited = map.object(bt_core::ObjectId(1)).unwrap();
        assert_eq!(inherited.name, "Changed");
        assert_eq!(inherited.size, DVec2::new(32.0, 24.0));
        assert_eq!(inherited.pos, DVec2::new(8.0, 16.0));
        assert_eq!(inherited.properties["hp"].value, Value::Int(10));
        assert!(!inherited.properties.contains_key("score"));
        let own = map.object(bt_core::ObjectId(2)).unwrap();
        assert_eq!(own.name, "own");
        assert_eq!(own.size, DVec2::new(9.0, 7.0));
        assert_eq!(own.properties["hp"].value, Value::Int(99));
    }
    assert_eq!(
        editor
            .document(doc)
            .unwrap()
            .history()
            .undoable()
            .unwrap()
            .changes()
            .len(),
        3
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(
        editor
            .map_of(first)
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .name,
        "Base"
    );
    apply(&mut editor, Intent::Run(ActionId::Redo));
    apply(&mut editor, Intent::Run(ActionId::SaveAll));
    for path in [scratch.join("a.tmx"), scratch.join("b.tmx")] {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("value=\"10\""),
            "inherited values must not become explicit overrides: {text}"
        );
        let read = bt_tmx::Loader::default().load_map(&path).unwrap().value;
        assert_eq!(
            read.object(bt_core::ObjectId(1)).unwrap().properties["hp"].value,
            Value::Int(10)
        );
        assert_eq!(
            read.object(bt_core::ObjectId(2)).unwrap().properties["hp"].value,
            Value::Int(99)
        );
    }
    assert!(!editor.has_unsaved());
}

#[test]
fn object_properties_edit_the_template_view_and_snapshots_keep_its_file_kind() {
    let scratch = Scratch::new("template-view-properties").unwrap();
    write_fixture(&scratch);
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("unit.tx")));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::SetAttribute {
            doc,
            inspected: Inspected::Objects(vec![bt_core::ObjectId(1)]),
            attribute: Attribute::Name,
            value: AttributeValue::Text("Edited through view".into()),
            merge: None,
        },
    );
    assert_eq!(
        editor
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .name,
        "Edited through view"
    );
    let snapshots = editor.snapshots_for_unanswered_exit();
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots[0].text.contains("<template>"));
    assert!(!snapshots[0].text.contains("<map "));
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(!editor.has_unsaved());
    assert_eq!(
        bt_tmx::Loader::default()
            .load_template(&scratch.join("unit.tx"))
            .unwrap()
            .value
            .object
            .as_ref()
            .unwrap()
            .name,
        "Edited through view"
    );
    let root = Arc::clone(editor.template_of(doc).unwrap());
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_ne!(editor.template_of(doc).unwrap(), &root);
}

#[test]
fn saving_an_instance_saves_its_dirty_template_and_new_maps_take_unsaved_bases() {
    let scratch = Scratch::new("template-dependent-save").unwrap();
    write_fixture(&scratch);
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("a.tmx")));
    let first = editor.current().unwrap();
    apply(&mut editor, Intent::Open(scratch.join("unit.tx")));
    let template_doc = editor.current().unwrap();
    let asset = editor.document(template_doc).unwrap().asset();
    editor
        .transact(template_doc, "Rename Base", |tx| {
            tx.template_mut(asset)?.object.as_mut().unwrap().name = "Unsaved base".into();
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    apply(&mut editor, Intent::Open(scratch.join("b.tmx")));
    assert_eq!(
        editor
            .map_of(editor.current().unwrap())
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .name,
        "Unsaved base"
    );
    apply(&mut editor, Intent::SelectDocument(first));
    let effects = apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(!effects.iter().any(|effect| matches!(effect, bt_editor::Effect::Notify(notice) if notice.severity == bt_editor::Severity::Error)), "{effects:?}");
    assert!(!editor.store().get(asset).unwrap().is_dirty());
    let map = bt_tmx::Loader::default()
        .load_map(&scratch.join("a.tmx"))
        .unwrap()
        .value;
    assert_eq!(
        map.object(bt_core::ObjectId(1)).unwrap().name,
        "Unsaved base"
    );
}

#[test]
fn a_disk_template_reload_propagates_without_a_template_tab_and_is_undoable() {
    let scratch = Scratch::new("template-reload-instances").unwrap();
    write_fixture(&scratch);
    let path = scratch.join("unit.tx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("a.tmx")));
    let doc = editor.current().unwrap();
    let mut template = (*bt_tmx::Loader::default()
        .load_template(&path)
        .unwrap()
        .value)
        .clone();
    template.object.as_mut().unwrap().name = "Changed on disk".into();
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let content_hash = bt_utils::hash::fnv1a_64(&std::fs::read(&path).unwrap());
    apply(
        &mut editor,
        Intent::Observed(vec![bt_editor::Observation {
            path,
            found: bt_editor::Found::Present { content_hash },
        }]),
    );
    assert_eq!(
        editor
            .map_of(doc)
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .name,
        "Changed on disk"
    );
    assert_eq!(
        editor
            .document(doc)
            .unwrap()
            .history()
            .undoable()
            .unwrap()
            .label(),
        "Reload Template"
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(
        editor
            .map_of(doc)
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .name,
        "Base"
    );
}

#[test]
fn cancelling_a_template_gesture_restores_every_instance_and_the_writer_cache() {
    let scratch = Scratch::new("template-gesture-cancel").unwrap();
    write_fixture(&scratch);
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("a.tmx")));
    let map_doc = editor.current().unwrap();
    apply(&mut editor, Intent::Open(scratch.join("unit.tx")));
    let doc = editor.current().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    editor.begin_gesture(doc, "Edit Template").unwrap();
    editor
        .gesture_apply(|tx| {
            tx.template_mut(asset)?
                .object
                .as_mut()
                .unwrap()
                .properties
                .insert("hp".into(), Property::new(Value::Int(50)));
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    assert_eq!(
        editor
            .map_of(map_doc)
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .properties["hp"]
            .value,
        Value::Int(50)
    );
    editor.cancel_gesture().unwrap();
    assert_eq!(
        editor
            .map_of(map_doc)
            .unwrap()
            .object(bt_core::ObjectId(1))
            .unwrap()
            .properties["hp"]
            .value,
        Value::Int(5)
    );
    apply(&mut editor, Intent::Run(ActionId::SaveAll));
    assert!(
        !std::fs::read_to_string(scratch.join("a.tmx"))
            .unwrap()
            .contains("value=\"50\"")
    );
}
