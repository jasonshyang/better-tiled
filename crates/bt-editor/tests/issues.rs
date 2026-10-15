//! Typed diagnostics, asynchronous ownership, live file probes and undoable repairs.
#![cfg(test)]
mod common;
use bt_core::{
    FileRef, ImageLayer, ImageRef, Layer, LayerId, LayerKind, MapObject, ObjectId, ObjectTemplate,
    TilesetImage, TilesetSlot,
};
use bt_editor::{
    ActionId, Editor, Effect, FileRequest, Intent, Issue, IssueKind, IssueTarget, LocateTarget,
    ResourceKind, Severity, TilesetHome,
};
use bt_test_data::{Scratch, fixtures::Atlas};
use common::{apply, editor_in, write_map};
use glam::{DVec2, UVec2};
use std::{collections::HashMap, path::Path, sync::Arc};

fn check(editor: &mut Editor) {
    for effect in editor.apply(Intent::RefreshIssues) {
        if let Effect::CheckIssues(job) = effect {
            editor.apply(Intent::IssuesChecked(job.run()));
        }
    }
}
fn repair(editor: &mut Editor, target: LocateTarget, path: &Path) {
    let doc = editor.current().unwrap();
    let request = editor
        .apply(Intent::LocateIssue { doc, target })
        .into_iter()
        .find_map(|effect| {
            if let Effect::PickFile(request) = effect {
                Some(request)
            } else {
                None
            }
        })
        .unwrap();
    let effects = apply(
        editor,
        Intent::FilePicked {
            request,
            path: Some(path.to_path_buf()),
        },
    );
    assert!(
        !effects.iter().any(
            |effect| matches!(effect, Effect::Notify(notice) if notice.severity == Severity::Error)
        ),
        "{effects:?}"
    );
    for effect in effects {
        if let Effect::CheckIssues(job) = effect {
            editor.apply(Intent::IssuesChecked(job.run()));
        }
    }
}
#[test]
fn missing_tileset_and_template_repair_keep_cells_and_object_overrides_and_undo() {
    let scratch = Scratch::new("issues-reference-repair").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let good = scratch.join("repaired.tsx");
    std::fs::rename(scratch.join("tiles.tsx"), &good).unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    let owner = editor.document(doc).unwrap().asset();
    check(&mut editor);
    let target = LocateTarget::Tileset {
        owner,
        slot: TilesetSlot::new(0).unwrap(),
    };
    assert!(
        editor
            .issues(doc)
            .iter()
            .any(|issue| issue.fix == Some(target) && issue.severity == Severity::Error)
    );
    let cells = editor
        .map_of(doc)
        .unwrap()
        .layer(LayerId(1))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .cells()
        .collect::<Vec<_>>();
    repair(&mut editor, target, &good);
    assert_eq!(
        editor.map_of(doc).unwrap().tilesets[0]
            .file
            .as_ref()
            .unwrap()
            .path,
        good
    );
    assert_eq!(
        editor
            .map_of(doc)
            .unwrap()
            .layer(LayerId(1))
            .unwrap()
            .as_tile_layer()
            .unwrap()
            .cells()
            .collect::<Vec<_>>(),
        cells
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor.map_of(doc).unwrap().tilesets[0]
            .file
            .as_ref()
            .unwrap()
            .path
            .ends_with("tiles.tsx")
    );
    apply(&mut editor, Intent::Run(ActionId::Redo));

    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "base".into(),
            size: DVec2::new(12.0, 8.0),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &scratch.join("base.tx"))
        .write()
        .unwrap();
    let text = r#"<map orientation="orthogonal" width="4" height="4" tilewidth="16" tileheight="16"><objectgroup id="1" name="Objects"><object id="7" template="lost.tx" name="Own name" x="10" y="20"/></objectgroup></map>"#;
    std::fs::write(scratch.join("objects.tmx"), text).unwrap();
    apply(&mut editor, Intent::Open(scratch.join("objects.tmx")));
    let doc = editor.current().unwrap();
    let map = editor.document(doc).unwrap().asset();
    check(&mut editor);
    let target = LocateTarget::Template {
        map,
        object: ObjectId(7),
    };
    assert!(
        editor
            .issues(doc)
            .iter()
            .any(|issue| issue.fix == Some(target))
    );
    repair(&mut editor, target, &scratch.join("base.tx"));
    let object = editor.map_of(doc).unwrap().object(ObjectId(7)).unwrap();
    assert_eq!(object.name, "Own name");
    assert_eq!(object.pos, DVec2::new(10.0, 20.0));
    assert_eq!(object.size, DVec2::new(12.0, 8.0));
    assert!(
        editor
            .issues(doc)
            .iter()
            .all(|issue| issue.fix != Some(target))
    );
    let effects = editor.apply(Intent::GoToIssue(IssueTarget::Object {
        map,
        object: ObjectId(7),
    }));
    assert_eq!(
        editor.document(doc).unwrap().selection().objects,
        vec![ObjectId(7)]
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::View(_)))
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor
            .map_of(doc)
            .unwrap()
            .object(ObjectId(7))
            .unwrap()
            .template
            .as_ref()
            .unwrap()
            .path
            .ends_with("lost.tx")
    );
}
#[test]
fn stale_workers_and_pickers_cannot_replace_edited_roots() {
    let scratch = Scratch::new("issues-late-results").unwrap();
    let mut editor = editor_in(&scratch);
    let doc = editor.open_new_map(bt_core::Map::default());
    let map = editor.document(doc).unwrap().asset();
    let pending = editor
        .apply(Intent::RefreshIssues)
        .into_iter()
        .find_map(|effect| {
            if let Effect::CheckIssues(job) = effect {
                Some(job.run())
            } else {
                None
            }
        })
        .unwrap();
    let target = LocateTarget::LayerImage {
        map,
        layer: LayerId(1),
    };
    let request = editor
        .apply(Intent::LocateIssue { doc, target })
        .into_iter()
        .find_map(|effect| {
            if let Effect::PickFile(request) = effect {
                Some(request)
            } else {
                None
            }
        })
        .unwrap();
    editor
        .transact(doc, "Edit", |tx| {
            tx.map_mut(map)?.class = "edited".into();
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    editor.apply(Intent::IssuesChecked(pending));
    assert!(editor.issues_pending(doc));
    let effects = editor.apply(Intent::FilePicked {
        request,
        path: Some(scratch.join("image.png")),
    });
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Notify(notice) if notice.severity == Severity::Error)
    ));
    assert_eq!(editor.map_of(doc).unwrap().class, "edited");
}
#[test]
fn image_repairs_preserve_transparency_dimensions_and_shared_undo() {
    let scratch = Scratch::new("issues-image-repair").unwrap();
    let mut editor = editor_in(&scratch);
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let missing = ImageRef::file(
        FileRef::new("lost.png", scratch.join("lost.png")),
        UVec2::splat(16),
    );
    let mut tileset = (*atlas.tileset).clone();
    tileset.image = TilesetImage::Atlas {
        image: missing.clone(),
        columns: 1,
        tile_count: 1,
    };
    let mut map = bt_core::Map {
        tilesets: vec![bt_core::TilesetRef::embedded(Arc::new(tileset))],
        layers: vec![Layer::new(
            LayerId(1),
            "Backdrop",
            LayerKind::Image(ImageLayer {
                image: Some(ImageRef {
                    transparent: Some(bt_core::Color::BLACK),
                    ..missing.clone()
                }),
                repeat_x: true,
                repeat_y: false,
            }),
        )],
        next_layer_id: 2,
        ..bt_core::Map::default()
    };
    map.width = 4;
    map.height = 4;
    let doc = editor.open_new_map(map);
    let map = editor.document(doc).unwrap().asset();
    check(&mut editor);
    for target in [
        LocateTarget::LayerImage {
            map,
            layer: LayerId(1),
        },
        LocateTarget::TilesetImage {
            home: TilesetHome::Embedded {
                map,
                slot: TilesetSlot::new(0).unwrap(),
            },
            tile: None,
        },
    ] {
        assert!(
            editor
                .issues(doc)
                .iter()
                .any(|issue| issue.fix == Some(target))
        );
        let version = editor.store().get(map).unwrap().version();
        let image = ImageRef::file(
            FileRef::new("new.png", scratch.join("new.png")),
            UVec2::new(64, 32),
        );
        editor.apply(Intent::IssueImageRead {
            request: FileRequest::LocateFile {
                doc,
                target,
                version,
            },
            result: Ok(image),
        });
        check(&mut editor);
    }
    let layer = editor
        .map_of(doc)
        .unwrap()
        .layer(LayerId(1))
        .unwrap()
        .as_image_layer()
        .unwrap();
    assert!(layer.repeat_x);
    assert_eq!(
        layer.image.as_ref().unwrap().transparent,
        Some(bt_core::Color::BLACK)
    );
    let ts = &editor.map_of(doc).unwrap().tilesets[0].tileset;
    assert_eq!(ts.columns(), 4);
    assert_eq!(ts.tile_count(), 8);
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(editor.map_of(doc).unwrap().tilesets[0].tileset.columns(), 1);
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor
            .map_of(doc)
            .unwrap()
            .layer(LayerId(1))
            .unwrap()
            .as_image_layer()
            .unwrap()
            .image
            .as_ref()
            .unwrap()
            .path()
            .unwrap()
            .ends_with("lost.png")
    );
}
#[test]
fn file_probes_repeat_and_decode_failures_deduplicate_by_reference() {
    let scratch = Scratch::new("issues-live-probes").unwrap();
    let mut editor = editor_in(&scratch);
    let path = scratch.join("image.png");
    let image = ImageRef::file(FileRef::new("image.png", &path), UVec2::ONE);
    let doc = editor.open_new_map(bt_core::Map {
        layers: vec![Layer::new(
            LayerId(1),
            "Image",
            LayerKind::Image(ImageLayer {
                image: Some(image),
                ..ImageLayer::default()
            }),
        )],
        next_layer_id: 2,
        ..bt_core::Map::default()
    });
    check(&mut editor);
    assert_eq!(editor.issues(doc).len(), 1);
    bt_image::replace_png(
        &path,
        &bt_image::DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        },
    )
    .unwrap();
    for effect in editor.apply(Intent::Tick { now_ms: 3_000 }) {
        if let Effect::CheckIssues(job) = effect {
            editor.apply(Intent::IssuesChecked(job.run()));
        }
    }
    assert!(editor.issues(doc).is_empty());
    let key = bt_image::ImageKey::of(&bt_image::ImageRequest {
        input: bt_image::ImageInput::File(path.clone()),
        transparent: None,
    });
    let failures = HashMap::from([(
        key,
        bt_image::Error::Decode {
            source_name: "image.png".into(),
            message: "bad pixels".into(),
        },
    )]);
    assert_eq!(editor.image_issues(doc, &failures).len(), 1);
    std::fs::remove_file(&path).unwrap();
    check(&mut editor);
    let mut issues = editor.issues(doc);
    issues.extend(editor.image_issues(doc, &failures));
    assert_eq!(bt_editor::deduplicate_issues(issues).len(), 1);
}
#[test]
fn issue_identity_keeps_distinct_references_and_rule_problems() {
    let first = Issue::new(
        IssueKind::Resource {
            kind: ResourceKind::Image,
            path: "image.png".into(),
        },
        "header failure",
    );
    let second = Issue {
        text: "decode failure".into(),
        ..first.clone()
    };
    let third = Issue::new(
        IssueKind::Resource {
            kind: ResourceKind::Tileset,
            path: "image.png".into(),
        },
        "same name, different resource kind",
    );
    let rule = Issue::new(
        IssueKind::AutoMap(bt_automap::ProblemKind::UnknownLayer),
        "unknown layer A",
    );
    let other = Issue {
        text: "unknown layer B".into(),
        ..rule.clone()
    };
    assert_eq!(
        bt_editor::deduplicate_issues(vec![first, second, third, rule.clone(), rule, other]).len(),
        4
    );
}

#[test]
fn world_map_and_collection_image_references_are_repairable() {
    let scratch = Scratch::new("issues-world-and-collection").unwrap();
    let map_path = write_map(scratch.path(), "map.tmx");
    let world_path = scratch.join("game.world");
    std::fs::write(&world_path, r#"{"maps":[{"fileName":"map.tmx","x":0,"y":0,"width":48,"height":32},{"fileName":"lost.tmx","x":48,"y":0,"width":48,"height":32}],"type":"world"}"#).unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(map_path));
    let doc = editor.current().unwrap();
    let world = editor.world_of(doc).unwrap();
    check(&mut editor);
    let target = LocateTarget::WorldMap { world, index: 1 };
    assert!(
        editor
            .issues(doc)
            .iter()
            .any(|issue| issue.fix == Some(target))
    );
    let good = write_map(scratch.path(), "next.tmx");
    repair(&mut editor, target, &good);
    assert_eq!(editor.store().world(world).unwrap().maps[1].file.path, good);
    assert_eq!(editor.store().world(world).unwrap().maps[1].rect.x, 48);
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor.store().world(world).unwrap().maps[1]
            .file
            .path
            .ends_with("lost.tmx")
    );
    let ts_path = scratch.join("collection.tsx");
    std::fs::write(&ts_path,r#"<tileset name="Collection" tilewidth="16" tileheight="16" tilecount="1" columns="0"><tile id="3"><image source="missing.png" width="16" height="16"/></tile></tileset>"#).unwrap();
    apply(&mut editor, Intent::Open(ts_path));
    let doc = editor.current().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    check(&mut editor);
    let target = LocateTarget::TilesetImage {
        home: TilesetHome::Asset(asset),
        tile: Some(bt_core::TileId(3)),
    };
    assert!(
        editor
            .issues(doc)
            .iter()
            .any(|issue| issue.fix == Some(target))
    );
    let version = editor.store().get(asset).unwrap().version();
    editor.apply(Intent::IssueImageRead {
        request: FileRequest::LocateFile {
            doc,
            target,
            version,
        },
        result: Ok(ImageRef::file(
            FileRef::new("new.png", scratch.join("new.png")),
            UVec2::new(48, 24),
        )),
    });
    assert_eq!(
        editor.store().tileset(asset).unwrap().tile_size(),
        UVec2::new(48, 24)
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(
        editor.store().tileset(asset).unwrap().tile_size(),
        UVec2::splat(16)
    );
}

#[test]
fn rule_compilation_and_live_sources_are_in_the_same_issue_list() {
    let scratch = Scratch::new("issues-automap-sources").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let rule = bt_core::Map {
        width: 2,
        height: 2,
        tile_width: 16,
        tile_height: 16,
        ..bt_core::Map::default()
    };
    bt_tmx::WriteRequest::for_map(&rule, &scratch.join("bad-rule.tmx"))
        .write()
        .unwrap();
    std::fs::write(scratch.join("rules.txt"), "bad-rule.tmx\n").unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    apply(&mut editor, Intent::Tick { now_ms: 0 });
    let doc = editor.current().unwrap();
    assert!(editor.issues(doc).iter().any(|issue| {
        issue.source == bt_editor::IssueSource::AutoMap
            && issue.severity == Severity::Error
            && issue
                .targets
                .contains(&IssueTarget::File(scratch.join("bad-rule.tmx")))
    }));
}

#[test]
fn embedded_image_header_and_pixel_failures_have_a_stable_target() {
    let scratch = Scratch::new("issues-embedded-image").unwrap();
    let mut editor = editor_in(&scratch);
    let image = ImageRef {
        source: bt_core::ImageSource::Embedded(bt_core::EmbeddedImage {
            format: "png".into(),
            bytes: Arc::from(b"not a png".as_slice()),
        }),
        declared_size: UVec2::ONE,
        transparent: None,
    };
    let key = bt_image::ImageKey::of(&bt_image::image_request(&image));
    let doc = editor.open_new_map(bt_core::Map {
        next_layer_id: 2,
        layers: vec![Layer::new(
            LayerId(1),
            "Embedded",
            LayerKind::Image(ImageLayer {
                image: Some(image),
                ..ImageLayer::default()
            }),
        )],
        ..bt_core::Map::default()
    });
    check(&mut editor);
    assert!(matches!(
        &editor.issues(doc)[0].kind,
        IssueKind::EmbeddedImage(_)
    ));
    let mut issues = editor.issues(doc);
    issues.extend(editor.image_issues(
        doc,
        &HashMap::from([(
            key,
            bt_image::Error::Decode {
                source_name: "embedded".into(),
                message: "bad pixels".into(),
            },
        )]),
    ));
    let issues = bt_editor::deduplicate_issues(issues);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].targets.len(), 1);
    assert!(issues[0].fix.is_some());
}

#[test]
fn painted_state_restoration_failures_survive_the_open_notice() {
    let scratch = Scratch::new("issues-restoration").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    std::fs::write(scratch.join("better-tiled.json"), "{ broken json").unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    assert!(editor.issues(doc).iter().any(|issue| matches!(
        issue.kind,
        IssueKind::AutoMap(bt_automap::ProblemKind::PaintedNotRestored)
    )));
}
