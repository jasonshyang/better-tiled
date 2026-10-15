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

#[test]
fn save_as_template_rebases_files_and_insert_reset_detach_share_normal_history() {
    use bt_core::{Layer, LayerId, LayerKind, ObjectId, ObjectLayer};
    use bt_editor::{FileRequest, SnapToggle};
    let scratch = Scratch::new("template-create-place").unwrap();
    std::fs::create_dir_all(scratch.join("templates")).unwrap();
    let map_path = scratch.join("map.tmx");
    let template_path = scratch.join("templates/chest.tx");
    let map = bt_core::Map {
        width: 8,
        height: 8,
        tile_width: 16,
        tile_height: 16,
        next_object_id: 2,
        next_layer_id: 2,
        layers: vec![Layer::new(
            LayerId(1),
            "Objects",
            LayerKind::Object(ObjectLayer {
                objects: vec![MapObject {
                    id: ObjectId(1),
                    name: "Chest".into(),
                    pos: DVec2::new(16.0, 16.0),
                    size: DVec2::splat(16.0),
                    properties: bt_core::Properties::from([(
                        "script".into(),
                        Property::new(Value::File("chest.lua".into())),
                    )]),
                    ..MapObject::default()
                }]
                .into(),
                ..ObjectLayer::default()
            }),
        )],
        ..bt_core::Map::default()
    };
    bt_tmx::WriteRequest::for_map(&map, &map_path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(map_path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::SaveAsTemplate {
                doc,
                object: ObjectId(1),
            },
            path: Some(template_path.clone()),
        },
    );
    let template = editor.chosen_template().unwrap();
    let definition = editor.store().template(template).unwrap();
    assert_eq!(
        definition.object.as_ref().unwrap().properties["script"].value,
        Value::File("../chest.lua".into())
    );
    assert!(
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .template
            .is_some()
    );
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ObjectTemplate(doc),
            path: Some(template_path.clone()),
        },
    );
    let preview = editor
        .template_preview(doc, DVec2::new(64.0, 64.0), SnapToggle::Keep)
        .unwrap();
    assert_eq!(preview.object(ObjectId(2)).unwrap().name, "Chest");
    assert_eq!(
        editor.map_of(doc).unwrap().next_object_id,
        2,
        "the ghost does not allocate in the stored map"
    );
    apply(
        &mut editor,
        Intent::PlaceTemplate {
            doc,
            at: DVec2::new(64.0, 64.0),
            snap: SnapToggle::Keep,
        },
    );
    let placed = editor
        .map_of(doc)
        .unwrap()
        .object(ObjectId(2))
        .unwrap()
        .clone();
    assert!(placed.overrides.is_empty());
    apply(
        &mut editor,
        Intent::SetAttribute {
            doc,
            inspected: Inspected::Objects(vec![ObjectId(2)]),
            attribute: Attribute::Name,
            value: AttributeValue::Text("Own name".into()),
            merge: None,
        },
    );
    apply(&mut editor, Intent::Run(ActionId::ResetTemplate));
    assert_eq!(
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(2))
            .unwrap()
            .name,
        "Chest"
    );
    assert_eq!(
        editor.map_of(doc).unwrap().object(ObjectId(2)).unwrap().pos,
        placed.pos
    );
    apply(&mut editor, Intent::Run(ActionId::DetachTemplate));
    let detached = editor.map_of(doc).unwrap().object(ObjectId(2)).unwrap();
    assert!(detached.template.is_none());
    assert_eq!(
        detached.properties["script"].value,
        Value::File("chest.lua".into())
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(2))
            .unwrap()
            .template
            .is_some()
    );
    apply(&mut editor, Intent::Run(ActionId::Save));
    let text = std::fs::read_to_string(&map_path).unwrap();
    assert!(
        !text.contains("name=\"script\""),
        "inherited script path must stay inherited: {text}"
    );
    let read = bt_tmx::Loader::default().load_map(&map_path).unwrap().value;
    assert_eq!(
        read.object(ObjectId(2)).unwrap().properties["script"].value,
        Value::File("../chest.lua".into())
    );
}

#[test]
fn tile_template_preview_and_placement_adopt_a_tileset_once_and_undo_it() {
    use bt_core::{Cell, ObjectId, TileId, TilesetRef, TilesetSlot};
    use bt_editor::{FileRequest, SnapToggle};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;
    let scratch = Scratch::new("template-tile-placement").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let tileset = scratch.join("tiles.tsx");
    bt_tmx::WriteRequest::for_tileset(&atlas.tileset, &tileset)
        .write()
        .unwrap();
    let path = scratch.join("tile.tx");
    let template = ObjectTemplate {
        tilesets: vec![TilesetRef {
            file: Some(bt_core::FileRef::new("tiles.tsx", &tileset)),
            first_gid: Some(1),
            tileset: atlas.tileset,
        }],
        object: Some(MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(1))),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
        .object_layer("Objects", Vec::new())
        .build()
        .unwrap();
    let map_path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &map_path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(map_path));
    let doc = editor.current().unwrap();
    apply(&mut editor, Intent::Open(tileset.clone()));
    let tileset_doc = editor.current().unwrap();
    let tileset_asset = editor.document(tileset_doc).unwrap().asset();
    editor
        .transact(tileset_doc, "Offset Tileset", |tx| {
            tx.tileset_mut(tileset_asset)?.tile_offset.x = 3;
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    apply(&mut editor, Intent::SelectDocument(doc));
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ObjectTemplate(doc),
            path: Some(path),
        },
    );
    let chosen = editor.chosen_template().unwrap();
    assert!(
        Arc::ptr_eq(
            &editor.store().template(chosen).unwrap().tilesets[0].tileset,
            editor.store().tileset(tileset_asset).unwrap()
        ),
        "templates take the authoritative unsaved tileset root"
    );
    assert_eq!(
        editor
            .template_preview(doc, DVec2::splat(48.0), SnapToggle::Keep)
            .unwrap()
            .tilesets
            .len(),
        1
    );
    assert!(editor.map_of(doc).unwrap().tilesets.is_empty());
    apply(
        &mut editor,
        Intent::PlaceTemplate {
            doc,
            at: DVec2::splat(48.0),
            snap: SnapToggle::Keep,
        },
    );
    assert_eq!(editor.map_of(doc).unwrap().tilesets.len(), 1);
    assert!(
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(1))
            .unwrap()
            .tile
            .is_some()
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(editor.map_of(doc).unwrap().tilesets.is_empty());
    apply(&mut editor, Intent::Run(ActionId::Redo));
    apply(
        &mut editor,
        Intent::PlaceTemplate {
            doc,
            at: DVec2::splat(80.0),
            snap: SnapToggle::Keep,
        },
    );
    assert_eq!(editor.map_of(doc).unwrap().tilesets.len(), 1);
}

#[test]
fn isometric_template_ghost_is_centred_in_drawing_coordinates_with_layer_offset() {
    use bt_core::{ObjectId, Orientation};
    use bt_editor::{FileRequest, SnapToggle, Snapping};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;
    let scratch = Scratch::new("template-isometric-ghost").unwrap();
    let template_path = scratch.join("base.tx");
    let template = ObjectTemplate {
        object: Some(MapObject {
            size: DVec2::new(32.0, 16.0),
            rotation: 15.0,
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &template_path)
        .write()
        .unwrap();
    let mut map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
        .orientation(Orientation::Isometric)
        .object_layer("Objects", Vec::new())
        .build()
        .unwrap();
    map.layers[0].offset = DVec2::new(10.0, 20.0);
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let mut editor = editor_in(&scratch);
    let mut settings = editor.settings().clone();
    settings.behaviour.snapping = Snapping::None;
    apply(&mut editor, Intent::ChangeSettings(Box::new(settings)));
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ObjectTemplate(doc),
            path: Some(template_path),
        },
    );
    let at = DVec2::new(128.0, 64.0);
    let ghost = editor.template_preview(doc, at, SnapToggle::Keep).unwrap();
    let object = ghost.object(ObjectId(1)).unwrap();
    let bounds = bt_core::object_outline(&ghost.geometry(), object, None).bounds();
    let centre = (bounds.min + bounds.max) * 0.5 + ghost.layers[0].offset;
    assert!(
        (centre - at).length() < 0.00001,
        "{centre:?} should equal {at:?}"
    );
}
