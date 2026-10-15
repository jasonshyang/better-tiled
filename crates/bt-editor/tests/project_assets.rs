//! Project creation, scoped sessions, safe switching and asynchronous browser ownership.

#![cfg(test)]
mod common;

use bt_core::{FileRef, Project, Property, Value};
use bt_editor::{ActionId, Choice, Editor, Effect, FileRequest, Intent};
use bt_test_data::Scratch;
use common::{apply, ask, editor_in, write_map};
use std::path::{Path, PathBuf};

fn project(folder: &Path) -> PathBuf {
    std::fs::create_dir_all(folder).unwrap();
    let path = folder.join("game.tiled-project");
    let value = Project {
        folders: vec![FileRef::new(".", folder)],
        ..Project::default()
    };
    bt_tmx::WriteRequest::for_project(&value, &path)
        .write()
        .unwrap();
    path
}
fn finish(editor: &mut Editor, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Load(job) => {
                apply(editor, Intent::Loaded(Box::new(job.run())));
            }
            Effect::ScanProject(job) => {
                apply(editor, Intent::ProjectFilesScanned(job.run()));
            }
            Effect::Stamps(job) => {
                apply(editor, Intent::StampsLoaded(job.run()));
            }
            _ => {}
        }
    }
}

#[test]
fn project_switch_cancel_save_and_reopening_restore_each_projects_documents() {
    let scratch = Scratch::new("project-switch-sessions").unwrap();
    let a = project(&scratch.join("a"));
    let b = project(&scratch.join("b"));
    let a_map = write_map(&scratch.join("a"), "map.tmx");
    let b_map = write_map(&scratch.join("b"), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(a_map.clone()));
    let doc = editor.current().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    editor
        .transact(doc, "Edit Map", |tx| {
            tx.map_mut(asset)?.class = "Dirty".into();
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    let effects = apply(&mut editor, Intent::Open(b.clone()));
    let question = ask(&effects).id;
    apply(
        &mut editor,
        Intent::Decided {
            decision: question,
            choice: Choice::Cancel,
        },
    );
    assert_eq!(editor.project_path(), Some(a.as_path()));
    assert!(editor.document(doc).is_some());
    let effects = apply(&mut editor, Intent::Open(b.clone()));
    let question = ask(&effects).id;
    apply(
        &mut editor,
        Intent::Decided {
            decision: question,
            choice: Choice::Save,
        },
    );
    assert_eq!(editor.project_path(), Some(b.as_path()));
    assert!(editor.document(doc).is_none());
    assert_eq!(
        bt_tmx::Loader::default()
            .load_map(&a_map)
            .unwrap()
            .value
            .class,
        "Dirty"
    );
    apply(&mut editor, Intent::Open(b_map.clone()));
    apply(&mut editor, Intent::Open(a.clone()));
    assert!(
        editor
            .documents()
            .in_order()
            .any(|doc| editor.path_of(doc.id()) == Some(a_map.clone()))
    );
    apply(&mut editor, Intent::Open(b.clone()));
    assert!(
        editor
            .documents()
            .in_order()
            .any(|doc| editor.path_of(doc.id()) == Some(b_map.clone()))
    );
    let session = editor.session();
    assert_eq!(session.project, Some(b.clone()));
    let mut restored = editor_in(&scratch);
    let effects = restored.restore_session(session);
    finish(&mut restored, effects);
    assert_eq!(restored.project_path(), Some(b.as_path()));
    assert!(
        restored
            .documents()
            .in_order()
            .any(|doc| restored.path_of(doc.id()) == Some(b_map.clone()))
    );
}

#[test]
fn a_load_from_the_old_project_cannot_reopen_after_a_switch() {
    let scratch = Scratch::new("project-load-epoch").unwrap();
    let old = write_map(scratch.path(), "old.tmx");
    let mut editor = editor_in(&scratch);
    let late = editor
        .apply(Intent::Open(old))
        .into_iter()
        .find_map(|effect| {
            if let Effect::Load(job) = effect {
                Some(job.run())
            } else {
                None
            }
        })
        .unwrap();
    let target = scratch.join("new/game.tiled-project");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::NewProject,
            path: Some(target.clone()),
        },
    );
    apply(&mut editor, Intent::Loaded(Box::new(late)));
    assert_eq!(editor.project_path(), Some(target.as_path()));
    assert_eq!(
        editor.documents().len(),
        1,
        "only the new project's document remains"
    );
    assert!(editor.map_of(editor.current().unwrap()).is_none());
}

#[test]
fn folder_scans_sort_naturally_and_an_old_refresh_cannot_replace_a_new_one() {
    let scratch = Scratch::new("project-scans").unwrap();
    write_map(scratch.path(), "map10.tmx");
    write_map(scratch.path(), "map2.tmx");
    let path = project(scratch.path());
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let names = |editor: &Editor| {
        editor.project_entries()[0]
            .children
            .iter()
            .filter(|entry| {
                entry
                    .path
                    .extension()
                    .is_some_and(|extension| extension == "tmx")
            })
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&editor), ["map2.tmx", "map10.tmx"]);
    let old = editor
        .apply(Intent::Run(ActionId::RefreshProject))
        .into_iter()
        .find_map(|effect| {
            if let Effect::ScanProject(job) = effect {
                Some(job.run())
            } else {
                None
            }
        })
        .unwrap();
    write_map(scratch.path(), "map99.tmx");
    apply(&mut editor, Intent::Run(ActionId::RefreshProject));
    apply(&mut editor, Intent::ProjectFilesScanned(old));
    assert_eq!(names(&editor), ["map2.tmx", "map10.tmx", "map99.tmx"]);
}

#[test]
fn project_properties_and_save_as_preserve_file_targets_and_undo() {
    let scratch = Scratch::new("project-properties-save-as").unwrap();
    let path = project(scratch.path());
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let project = editor.project_asset().unwrap();
    let doc = editor.current().unwrap();
    let mut value = (**editor.store().project(project).unwrap()).clone();
    value.properties.insert(
        "script".into(),
        Property::new(Value::File("start.lua".into())),
    );
    apply(
        &mut editor,
        Intent::SetProject {
            doc,
            project,
            value: Box::new(value),
        },
    );
    let destination = scratch.join("other/game.tiled-project");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::ProjectAs(project),
            path: Some(destination.clone()),
        },
    );
    let saved = bt_tmx::parse_project(
        &std::fs::read_to_string(&destination).unwrap(),
        &destination,
    )
    .unwrap()
    .value;
    assert_eq!(
        saved.properties["script"].value,
        Value::File("../start.lua".into())
    );
    assert_eq!(saved.folders[0].path, scratch.path());
    assert!(
        editor.undo(doc).is_ok(),
        "Save Project As must not create an unrecorded version barrier"
    );
}

#[test]
fn generic_json_tilesets_open_as_tileset_documents() {
    let scratch = Scratch::new("project-generic-json").unwrap();
    let atlas = bt_test_data::fixtures::Atlas::builder()
        .build_in(scratch.path())
        .unwrap();
    let path = scratch.join("tiles.json");
    bt_tmx::WriteRequest::for_tileset(&atlas.tileset, &path)
        .write()
        .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.document(editor.current().unwrap()).unwrap();
    assert!(editor.store().tileset(doc.asset()).is_some());
}

#[test]
fn project_compatibility_applies_to_new_maps_and_project_edits_have_recovery_snapshots() {
    let scratch = Scratch::new("project-compatibility-recovery").unwrap();
    let path = project(scratch.path());
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let project = editor.project_asset().unwrap();
    let doc = editor.current().unwrap();
    let mut value = (**editor.store().project(project).unwrap()).clone();
    value.compatibility_version = bt_core::CompatibilityVersion::Tiled1_8;
    value
        .properties
        .insert("recover_me".into(), Property::new(Value::Int(42)));
    apply(
        &mut editor,
        Intent::SetProject {
            doc,
            project,
            value: Box::new(value),
        },
    );
    let snapshot = editor
        .snapshots_for_unanswered_exit()
        .into_iter()
        .find(|snapshot| snapshot.meta.original == Some(path.clone()))
        .unwrap();
    assert!(snapshot.text.contains("recover_me"));
    std::fs::create_dir_all(snapshot.path.parent().unwrap()).unwrap();
    std::fs::write(&snapshot.path, &snapshot.text).unwrap();
    let map = bt_test_data::fixtures::MapBuilder::new(glam::UVec2::splat(16), glam::UVec2::ONE)
        .tile_layer("ground", ".")
        .build()
        .unwrap();
    let map_doc = editor.open_new_map(map);
    let map_path = scratch.join("new.tmx");
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::SaveAs {
                doc: map_doc,
                suggested: map_path.clone(),
            },
            path: Some(map_path.clone()),
        },
    );
    assert!(
        std::fs::read_to_string(map_path)
            .unwrap()
            .contains("version=\"1.8\"")
    );
    let mut recovered = editor_in(&scratch);
    apply(&mut recovered, Intent::Open(path.clone()));
    let offered = apply(
        &mut recovered,
        Intent::Recoverable(vec![bt_editor::Orphan {
            snapshot: snapshot.path.clone(),
            meta: snapshot.meta,
        }]),
    );
    let decision = ask(&offered).id;
    apply(
        &mut recovered,
        Intent::Decided {
            decision,
            choice: Choice::Recover,
        },
    );
    let project = recovered.project_asset().unwrap();
    assert_eq!(
        recovered.store().project(project).unwrap().properties["recover_me"].value,
        Value::Int(42)
    );
    assert!(recovered.store().get(project).unwrap().is_dirty());
    let saved = apply(&mut recovered, Intent::Run(ActionId::Save));
    assert!(
        saved
            .iter()
            .any(|effect| matches!(effect, Effect::DeleteSnapshot(path) if path == &snapshot.path))
    );
}

#[test]
fn project_rule_changes_are_seen_on_tick_and_follow_undo() {
    let scratch = Scratch::new("project-rules-settings").unwrap();
    let project_path = project(scratch.path());
    let folder = scratch.join("maps");
    std::fs::create_dir_all(&folder).unwrap();
    let map = write_map(&folder, "map.tmx");
    let first = scratch.join("first.txt");
    let second = scratch.join("second.txt");
    std::fs::write(&first, "# no rules yet\n").unwrap();
    std::fs::write(&second, "# no rules yet\n").unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(project_path));
    apply(&mut editor, Intent::Open(map));
    let doc = editor.current().unwrap();
    let project = editor.project_asset().unwrap();
    apply(&mut editor, Intent::Tick { now_ms: 0 });
    for (index, path) in [&first, &second].into_iter().enumerate() {
        let mut value = (**editor.store().project(project).unwrap()).clone();
        value.automapping_rules_file = Some(FileRef::new(
            path.file_name().unwrap().to_str().unwrap(),
            path,
        ));
        apply(
            &mut editor,
            Intent::SetProject {
                doc,
                project,
                value: Box::new(value),
            },
        );
        apply(
            &mut editor,
            Intent::Tick {
                now_ms: (index as u64 + 1) * 20,
            },
        );
        assert_eq!(
            editor.automap_rules(doc).unwrap().file(),
            Some(path.as_path())
        );
    }
    apply(&mut editor, Intent::Run(ActionId::Undo));
    apply(&mut editor, Intent::Tick { now_ms: 100 });
    assert_eq!(
        editor.automap_rules(doc).unwrap().file(),
        Some(first.as_path())
    );
}
