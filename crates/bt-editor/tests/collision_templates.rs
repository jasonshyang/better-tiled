//! Template actions also apply to tile collision shapes, with shared-root propagation.
#![cfg(test)]
mod common;
use bt_core::{
    CollisionGroup, FileRef, MapObject, ObjectId, ObjectLayer, ObjectTemplate, Property, TileData,
    TileId, Value,
};
use bt_editor::{
    ActionId, Editor, Effect, Enabled, FileRequest, Intent, IssueTarget, LocateTarget,
    SelectionMode, SnapToggle, TilesetHome, TilesetMode,
};
use bt_test_data::{Scratch, fixtures::Atlas};
use common::{apply, editor_in};
use glam::DVec2;
use std::{path::Path, sync::Arc};

fn fixture(scratch: &Scratch) {
    let base = MapObject {
        name: "Shape".into(),
        size: DVec2::new(8.0, 6.0),
        properties: [("hp".into(), Property::new(Value::Int(5)))].into(),
        ..MapObject::default()
    };
    let template = ObjectTemplate {
        object: Some(base.clone()),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &scratch.join("shape.tx"))
        .write()
        .unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let mut tileset = (*atlas.tileset).clone();
    let instance = MapObject {
        id: ObjectId(1),
        pos: DVec2::new(2.0, 3.0),
        template: Some(FileRef::new("shape.tx", scratch.join("shape.tx"))),
        ..base
    };
    tileset.tiles.insert(
        TileId(0),
        TileData {
            collision: Some(CollisionGroup {
                layer: ObjectLayer {
                    objects: Arc::new(vec![instance]),
                    ..ObjectLayer::default()
                },
                ..CollisionGroup::default()
            }),
            ..TileData::default()
        },
    );
    bt_tmx::WriteRequest::for_tileset(&tileset, &scratch.join("tiles.tsx"))
        .write()
        .unwrap();
    bt_tmx::WriteRequest::for_tileset(&tileset, &scratch.join("other.tsx"))
        .write()
        .unwrap();
}
fn collision(editor: &mut Editor, path: &Path) -> bt_editor::DocumentId {
    apply(editor, Intent::Open(path.to_path_buf()));
    let doc = editor.current().unwrap();
    editor.apply(Intent::SelectTilesetTiles {
        doc,
        tiles: vec![TileId(0)],
        current: Some(TileId(0)),
    });
    editor.apply(Intent::SetTilesetMode {
        doc,
        mode: TilesetMode::Collision,
    });
    doc
}
fn shape(editor: &Editor, asset: bt_editor::AssetId, id: u32) -> MapObject {
    editor
        .store()
        .tileset(asset)
        .unwrap()
        .tile(TileId(0))
        .unwrap()
        .collision
        .as_ref()
        .unwrap()
        .layer
        .objects
        .iter()
        .find(|object| object.id == ObjectId(id))
        .unwrap()
        .clone()
}
fn select(editor: &mut Editor, doc: bt_editor::DocumentId, id: u32) {
    editor.apply(Intent::SelectObjects {
        doc,
        ids: vec![ObjectId(id)],
        mode: SelectionMode::Replace,
    });
}

#[test]
fn collision_template_placement_commands_base_propagation_and_save_share_history() {
    let scratch = Scratch::new("collision-template-actions").unwrap();
    fixture(&scratch);
    let mut editor = editor_in(&scratch);
    let doc = collision(&mut editor, &scratch.join("tiles.tsx"));
    let asset = editor.document(doc).unwrap().asset();
    assert!(editor.store().by_path(&scratch.join("shape.tx")).is_some());
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ObjectTemplate(doc),
            path: Some(scratch.join("shape.tx")),
        },
    );
    assert_eq!(
        editor.enabled(ActionId::InsertTemplateTool),
        Enabled::Enabled
    );
    let ghost = editor
        .template_preview(doc, DVec2::new(20.0, 20.0), SnapToggle::Keep)
        .unwrap();
    assert!(
        ghost
            .layer(bt_core::LayerId(2))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects
            .last()
            .unwrap()
            .opacity
            < 1.0
    );
    apply(
        &mut editor,
        Intent::PlaceTemplate {
            doc,
            at: DVec2::new(20.0, 20.0),
            snap: SnapToggle::Keep,
        },
    );
    let placed = editor.document(doc).unwrap().selection().objects[0];
    assert_eq!(placed, ObjectId(2));
    assert!(shape(&editor, asset, 2).template.is_some());
    apply(&mut editor, Intent::Run(ActionId::DetachTemplate));
    assert!(shape(&editor, asset, 2).template.is_none());
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(shape(&editor, asset, 2).template.is_some());
    apply(&mut editor, Intent::Run(ActionId::EditTemplate));
    let base_doc = editor.current().unwrap();
    let base_asset = editor.document(base_doc).unwrap().asset();
    editor
        .transact(base_doc, "Change base", |tx| {
            tx.template_mut(base_asset)?.object.as_mut().unwrap().size = DVec2::new(12.0, 10.0);
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    assert_eq!(shape(&editor, asset, 1).size, DVec2::new(12.0, 10.0));
    assert_eq!(shape(&editor, asset, 2).size, DVec2::new(12.0, 10.0));
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(shape(&editor, asset, 2).size, DVec2::new(8.0, 6.0));
    apply(&mut editor, Intent::Run(ActionId::Redo));
    // A tileset loaded later uses the edited base even though the template file is still old.
    let other_doc = collision(&mut editor, &scratch.join("other.tsx"));
    let other = editor.document(other_doc).unwrap().asset();
    assert_eq!(shape(&editor, other, 1).size, DVec2::new(12.0, 10.0));
    apply(
        &mut editor,
        Intent::GoToIssue(IssueTarget::CollisionObject {
            home: TilesetHome::Asset(asset),
            tile: TileId(0),
            object: ObjectId(2),
        }),
    );
    assert_eq!(editor.current(), Some(doc));
    assert_eq!(
        editor.document(doc).unwrap().selection().objects,
        vec![ObjectId(2)]
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(!effects.iter().any(|effect| matches!(effect,Effect::Notify(notice) if notice.severity==bt_editor::Severity::Error)),"{effects:?}");
    assert_eq!(
        bt_tmx::Loader::default()
            .load_template(&scratch.join("shape.tx"))
            .unwrap()
            .value
            .object
            .as_ref()
            .unwrap()
            .size,
        DVec2::new(12.0, 10.0)
    );
    let text = std::fs::read_to_string(scratch.join("tiles.tsx")).unwrap();
    assert!(
        !text.contains("name=\"hp\""),
        "Inherited properties must not become overrides: {text}"
    );
    // Save As Template applies to a detached shape and is undoable on the tileset document.
    select(&mut editor, doc, 2);
    apply(&mut editor, Intent::Run(ActionId::DetachTemplate));
    assert_eq!(editor.enabled(ActionId::SaveAsTemplate), Enabled::Enabled);
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::SaveAsTemplate {
                doc,
                object: ObjectId(2),
            },
            path: Some(scratch.join("new-shape.tx")),
        },
    );
    assert!(scratch.join("new-shape.tx").is_file());
    assert!(
        shape(&editor, asset, 2)
            .template
            .as_ref()
            .unwrap()
            .path
            .ends_with("new-shape.tx")
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(shape(&editor, asset, 2).template.is_none());
}

#[test]
fn collision_template_reset_replace_and_missing_reference_repair_are_undoable() {
    let scratch = Scratch::new("collision-template-repairs").unwrap();
    fixture(&scratch);
    std::fs::rename(scratch.join("shape.tx"), scratch.join("found.tx")).unwrap();
    let mut editor = editor_in(&scratch);
    let doc = collision(&mut editor, &scratch.join("tiles.tsx"));
    let asset = editor.document(doc).unwrap().asset();
    apply(&mut editor, Intent::RefreshIssues);
    let target = LocateTarget::CollisionTemplate {
        home: TilesetHome::Asset(asset),
        tile: TileId(0),
        object: ObjectId(1),
    };
    assert!(
        editor
            .issues(doc)
            .iter()
            .any(|issue| issue.fix == Some(target))
    );
    let version = editor.store().get(asset).unwrap().version();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::LocateFile {
                doc,
                target,
                version,
            },
            path: Some(scratch.join("found.tx")),
        },
    );
    assert_eq!(shape(&editor, asset, 1).name, "Shape");
    assert_eq!(shape(&editor, asset, 1).pos, DVec2::new(2.0, 3.0));
    select(&mut editor, doc, 1);
    editor
        .transact(doc, "Override shape", |tx| {
            let ts = tx.tileset_mut(asset)?;
            let object = Arc::make_mut(
                &mut ts
                    .tiles
                    .get_mut(&TileId(0))
                    .unwrap()
                    .collision
                    .as_mut()
                    .unwrap()
                    .layer
                    .objects,
            )
            .first_mut()
            .unwrap();
            object.name = "Own".into();
            object.overrides |= bt_core::ObjectOverrides::NAME;
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    apply(&mut editor, Intent::Run(ActionId::ResetTemplate));
    assert_eq!(shape(&editor, asset, 1).name, "Shape");
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(shape(&editor, asset, 1).name, "Own");
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ObjectTemplate(doc),
            path: Some(scratch.join("found.tx")),
        },
    );
    apply(&mut editor, Intent::Run(ActionId::ReplaceWithTemplate));
    assert_eq!(shape(&editor, asset, 1).name, "Shape");
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(shape(&editor, asset, 1).name, "Own");
}

#[test]
fn embedded_collision_instances_follow_bases_and_refuse_conversion_to_tile_objects() {
    let scratch = Scratch::new("embedded-collision-template").unwrap();
    fixture(&scratch);
    let tileset = bt_tmx::Loader::default()
        .load_tileset(&scratch.join("tiles.tsx"))
        .unwrap()
        .value;
    let map = bt_core::Map {
        width: 2,
        height: 2,
        tile_width: 16,
        tile_height: 16,
        tilesets: vec![bt_core::TilesetRef::embedded(tileset)],
        ..bt_core::Map::default()
    };
    bt_tmx::WriteRequest::for_map(&map, &scratch.join("map.tmx"))
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(scratch.join("map.tmx")));
    let map_doc = editor.current().unwrap();
    apply(&mut editor, Intent::Open(scratch.join("shape.tx")));
    let doc = editor.current().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    editor
        .transact(doc, "Base size", |tx| {
            tx.template_mut(asset)?.object.as_mut().unwrap().size = DVec2::new(22.0, 11.0);
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    let object = editor.map_of(map_doc).unwrap().tilesets[0]
        .tileset
        .tile(TileId(0))
        .unwrap()
        .collision
        .as_ref()
        .unwrap()
        .layer
        .objects
        .first()
        .unwrap();
    assert_eq!(object.size, DVec2::new(22.0, 11.0));
    let result = editor.transact(doc, "Invalid tile base", |tx| {
        tx.template_mut(asset)?.object.as_mut().unwrap().tile = Some(bt_core::Cell::new(
            bt_core::TilesetSlot::new(0).unwrap(),
            TileId(0),
        ));
        Ok::<_, bt_editor::EditError>(())
    });
    assert!(matches!(
        result,
        Err(bt_editor::EditError::Operation(
            bt_edit::EditError::TileTemplateInCollision
        ))
    ));
    assert!(
        editor
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .tile
            .is_none()
    );
}

#[test]
fn failed_template_save_keeps_its_tileset_and_map_unsaved() {
    let scratch = Scratch::new("collision-template-save-failure").unwrap();
    fixture(&scratch);
    let mut editor = editor_in(&scratch);
    let doc = collision(&mut editor, &scratch.join("tiles.tsx"));
    let asset = editor.document(doc).unwrap().asset();
    apply(&mut editor, Intent::Open(scratch.join("shape.tx")));
    let base_doc = editor.current().unwrap();
    let base = editor.document(base_doc).unwrap().asset();
    editor
        .transact(base_doc, "Edit", |tx| {
            tx.template_mut(base)?.object.as_mut().unwrap().size = DVec2::splat(22.0);
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    let before = std::fs::read(scratch.join("tiles.tsx")).unwrap();
    let path = scratch.join("shape.tx");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    apply(
        &mut editor,
        Intent::GoToIssue(IssueTarget::Tileset {
            home: TilesetHome::Asset(asset),
            tile: Some(TileId(0)),
        }),
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::SaveAll));
    assert!(effects.iter().any(|effect|matches!(effect,Effect::Notify(notice)if notice.severity==bt_editor::Severity::Error)));
    assert_eq!(std::fs::read(scratch.join("tiles.tsx")).unwrap(), before);
    assert!(editor.store().get(asset).unwrap().is_dirty());
    #[allow(clippy::permissions_set_readonly_false, reason = "scratch cleanup")]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
}
