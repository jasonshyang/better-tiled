//! Opening, saving, closing and quitting on temporary folders, with loads run as the window's
//! workers run them; recovery snapshots and projects found above maps.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bt_core::{FileRef, LayerId, TilesetRef};
use bt_editor::{
    ActionId, Choice, Editor, Effect, FileRequest, Intent, Purpose, Question, Severity, orphans,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use glam::UVec2;

/// A map with an external tileset, written in `folder`.
fn write_map(folder: &Path, name: &str) -> PathBuf {
    let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
    let tileset_path = folder.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 2))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 1 2\n3 4 5")
        .tile_layer("top", ". 6 .\n. . .")
        .build()
        .unwrap();
    map.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", &tileset_path)),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    let path = folder.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// Applies an intent and carries out loads as a worker would, returning the other effects.
fn apply(editor: &mut Editor, intent: Intent) -> Vec<Effect> {
    let mut pending = vec![intent];
    let mut out = Vec::new();
    while let Some(intent) = pending.pop() {
        for effect in editor.apply(intent) {
            match effect {
                Effect::Load(job) => pending.push(Intent::Loaded(Box::new(job.run()))),
                other => out.push(other),
            }
        }
    }
    out
}

fn editor_in(scratch: &Scratch) -> Editor {
    Editor::builder()
        .recovery_folder(scratch.join("recovery"))
        .pid(1000)
        .home(Some(scratch.path().to_path_buf()))
        .build()
        .unwrap()
}

fn ask(effects: &[Effect]) -> &bt_editor::Decision {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Ask(decision) => Some(decision),
            _ => None,
        })
        .expect("a question")
}

#[test]
fn opening_shares_the_stores_tileset_and_reopening_focuses_the_tab() {
    let scratch = Scratch::new("bt-editor-open").unwrap();
    let a = write_map(scratch.path(), "a.tmx");
    let b = write_map(scratch.path(), "b.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(a.clone()));
    apply(&mut editor, Intent::Open(b));
    assert_eq!(editor.documents().len(), 2);
    let maps: Vec<_> = editor
        .documents()
        .in_order()
        .map(|d| editor.map_of(d.id()).unwrap().clone())
        .collect();
    assert!(
        Arc::ptr_eq(&maps[0].tilesets[0].tileset, &maps[1].tilesets[0].tileset),
        "one tileset asset"
    );
    let first = editor.documents().ids()[0];
    apply(&mut editor, Intent::Open(a));
    assert_eq!(editor.documents().len(), 2);
    assert_eq!(editor.current(), Some(first));
    assert!(!editor.has_unsaved());
}

#[test]
fn saving_writes_tileds_bytes_and_cleans_the_asset() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let scratch = Scratch::new("bt-editor-save").unwrap();
    for file in ["desert.tmx", "desert.tsx", "tmw_desert_spacing.png"] {
        std::fs::copy(examples.join(file), scratch.join(file)).unwrap();
    }
    let path = scratch.join("desert.tmx");
    let original = std::fs::read(&path).unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    // A change and its undo: the map is clean again, and saving writes what was there.
    apply(
        &mut editor,
        Intent::SetLayerVisible {
            doc,
            layer: LayerId(1),
            visible: false,
        },
    );
    assert!(editor.has_unsaved());
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(!editor.has_unsaved());
    let effects = apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::Notify(n) if n.severity == Severity::Error)),
        "{effects:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original, "byte for byte");
    // A real change is saved, and the file holds it.
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "Sand".into(),
        },
    );
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(!editor.has_unsaved());
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("name=\"Sand\""));
    let asset = editor
        .store()
        .get(editor.document(doc).unwrap().asset())
        .unwrap();
    assert_eq!(
        asset.on_disk().unwrap().content_hash,
        bt_utils::hash::fnv1a_64(saved.as_bytes())
    );
}

#[test]
fn save_as_moves_the_map_and_keeps_its_references() {
    let scratch = Scratch::new("bt-editor-save-as").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    std::fs::create_dir(scratch.join("elsewhere")).unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    let effects = apply(&mut editor, Intent::Run(ActionId::SaveAs));
    assert!(matches!(
        effects.as_slice(),
        [Effect::PickFile(FileRequest::SaveAs { .. })]
    ));
    let target = scratch.join("elsewhere/moved.tmx");
    apply(
        &mut editor,
        Intent::FilePicked {
            request: FileRequest::SaveAs {
                doc,
                suggested: PathBuf::new(),
            },
            path: Some(target.clone()),
        },
    );
    assert_eq!(editor.path_of(doc), Some(target.clone()));
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("source=\"../tiles.tsx\""), "{text}");
    assert!(!editor.has_unsaved());
}

#[test]
fn closing_and_quitting_ask_about_unsaved_work() {
    let scratch = Scratch::new("bt-editor-close").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "changed".into(),
        },
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::Close));
    let question = ask(&effects).clone();
    assert!(matches!(
        question.question,
        Question::Unsaved {
            purpose: Purpose::Close,
            ..
        }
    ));
    apply(
        &mut editor,
        Intent::Decided {
            decision: question.id,
            choice: Choice::Cancel,
        },
    );
    assert_eq!(editor.documents().len(), 1);
    let effects = apply(&mut editor, Intent::Run(ActionId::Close));
    let question = ask(&effects).clone();
    apply(
        &mut editor,
        Intent::Decided {
            decision: question.id,
            choice: Choice::DontSave,
        },
    );
    assert!(editor.documents().is_empty());
    assert_eq!(
        editor.store().assets().count(),
        0,
        "the map and its tileset are gone"
    );

    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "saved on quit".into(),
        },
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::Quit));
    let question = ask(&effects).clone();
    assert!(matches!(
        question.question,
        Question::Unsaved {
            purpose: Purpose::Quit,
            ..
        }
    ));
    let effects = apply(
        &mut editor,
        Intent::Decided {
            decision: question.id,
            choice: Choice::Save,
        },
    );
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Quit)),
        "{effects:?}"
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("saved on quit")
    );
}

#[test]
fn a_read_only_file_is_refused_and_the_map_stays_unsaved() {
    let scratch = Scratch::new("bt-editor-read-only").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "x".into(),
        },
    );
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    let effects = apply(&mut editor, Intent::Run(ActionId::Save));
    let error = effects
        .iter()
        .find_map(|e| match e {
            Effect::Notify(n) if n.severity == Severity::Error => Some(n.clone()),
            _ => None,
        })
        .expect("an error");
    assert!(error.detail.unwrap().contains("read-only"));
    assert!(editor.has_unsaved());
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "letting the scratch folder be removed"
    )]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
}

#[test]
fn unsaved_maps_are_snapshotted_and_recovered_after_a_crash() {
    let scratch = Scratch::new("bt-editor-recovery").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut crashed = editor_in(&scratch);
    apply(&mut crashed, Intent::Open(path.clone()));
    let doc = crashed.current().unwrap();
    apply(
        &mut crashed,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "unsaved".into(),
        },
    );
    assert!(
        apply(&mut crashed, Intent::Tick { now_ms: 1_000 }).is_empty(),
        "not quiet yet"
    );
    let effects = apply(&mut crashed, Intent::Tick { now_ms: 3_500 });
    let [Effect::WriteSnapshot(snapshot)] = effects.as_slice() else {
        panic!("one snapshot: {effects:?}");
    };
    snapshot.write().unwrap();
    assert!(
        apply(&mut crashed, Intent::Tick { now_ms: 9_000 }).is_empty(),
        "nothing changed since"
    );
    drop(crashed);

    // A later run finds the snapshot of a run with no heartbeat.
    let found = orphans(&scratch.join("recovery"), 2000, SystemTime::now());
    assert_eq!(found.len(), 1);
    let mut editor = Editor::builder()
        .recovery_folder(scratch.join("recovery"))
        .pid(2000)
        .build()
        .unwrap();
    let effects = apply(&mut editor, Intent::Recoverable(found));
    let question = ask(&effects).clone();
    let effects = apply(
        &mut editor,
        Intent::Decided {
            decision: question.id,
            choice: Choice::Recover,
        },
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::DeleteSnapshot(_)))
    );
    let doc = editor.current().unwrap();
    assert_eq!(editor.map_of(doc).unwrap().layers[0].name, "unsaved");
    assert!(editor.has_unsaved(), "recovered work is unsaved");
    assert_eq!(
        editor.path_of(doc),
        Some(std::fs::canonicalize(&path).unwrap())
    );
}

#[test]
fn a_project_above_the_map_is_adopted() {
    let scratch = Scratch::new("bt-editor-project").unwrap();
    // The scratch folder stands for the home folder, where the search stops.
    std::fs::create_dir_all(scratch.join("game/maps")).unwrap();
    let path = write_map(&scratch.join("game/maps"), "a.tmx");
    std::fs::write(
        scratch.join("game/game.tiled-project"),
        "{\n    \"folders\": [\n        \".\"\n    ]\n}\n",
    )
    .unwrap();
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let kinds: Vec<_> = editor.store().assets().map(|a| a.kind()).collect();
    assert!(kinds.contains(&bt_editor::AssetKind::Project), "{kinds:?}");
}

#[test]
fn the_session_lists_open_files_and_restores_them() {
    let scratch = Scratch::new("bt-editor-session").unwrap();
    let a = write_map(scratch.path(), "a.tmx");
    let b = write_map(scratch.path(), "b.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(a.clone()));
    apply(&mut editor, Intent::Open(b.clone()));
    let first = editor.documents().ids()[0];
    apply(&mut editor, Intent::SelectDocument(first));
    apply(
        &mut editor,
        Intent::SetCamera {
            doc: first,
            view: bt_editor::ViewState {
                center: glam::DVec2::new(5.0, 6.0),
                zoom: 3.0,
            },
        },
    );
    let session = editor.session();
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap();
    assert_eq!(session.open_files, vec![canonical(&a), canonical(&b)]);
    assert_eq!(session.current, Some(canonical(&a)));
    let mut restored = editor_in(&scratch);
    let session = bt_editor::Session::parse(&session.to_json().unwrap()).unwrap();
    for effect in restored.restore_session(session) {
        if let Effect::Load(job) = effect {
            restored.apply(Intent::Loaded(Box::new(job.run())));
        }
    }
    assert_eq!(restored.documents().len(), 2);
    let current = restored.current().unwrap();
    assert_eq!(restored.path_of(current), Some(canonical(&a)));
    assert_eq!(restored.camera_of(current).map(|c| c.zoom), Some(3.0));
}

/// Which load of a restart finishes first.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FirstLoad {
    Recovered,
    Session,
}

/// A run that ends with unsaved work nobody was asked about, then the next start restoring the
/// session and recovering the work, its loads finishing in the order given.
fn restart_after_unanswered_exit(name: &str, first: FirstLoad) -> Editor {
    let scratch = Scratch::new(name).unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut ended = editor_in(&scratch);
    apply(&mut ended, Intent::Open(path));
    let doc = ended.current().unwrap();
    apply(
        &mut ended,
        Intent::RenameLayer {
            doc,
            layer: LayerId(1),
            name: "unsaved".into(),
        },
    );
    // No snapshot was due yet: the exit writes one of the latest change.
    let snapshots = ended.snapshots_for_unanswered_exit();
    let [snapshot] = snapshots.as_slice() else {
        panic!("one snapshot: {snapshots:?}");
    };
    snapshot.write().unwrap();
    assert!(
        ended.snapshots_for_unanswered_exit().is_empty(),
        "written once"
    );
    let session = ended.session();
    drop(ended);

    let mut editor = Editor::builder()
        .recovery_folder(scratch.join("recovery"))
        .pid(2000)
        .build()
        .unwrap();
    let session_loads = editor.restore_session(session);
    let found = orphans(&scratch.join("recovery"), 2000, SystemTime::now());
    let effects = editor.apply(Intent::Recoverable(found));
    let question = ask(&effects).clone();
    let recover_loads = editor.apply(Intent::Decided {
        decision: question.id,
        choice: Choice::Recover,
    });
    let loads: Vec<Effect> = match first {
        FirstLoad::Recovered => recover_loads.into_iter().chain(session_loads).collect(),
        FirstLoad::Session => session_loads.into_iter().chain(recover_loads).collect(),
    };
    for effect in loads {
        if let Effect::Load(job) = effect {
            editor.apply(Intent::Loaded(Box::new(job.run())));
        }
    }
    editor
}

#[test]
fn an_unanswered_exit_keeps_the_latest_work_and_recovery_wins_over_the_session() {
    // Recovered first, the work becomes the file's document and the session's load finds it.
    let editor =
        restart_after_unanswered_exit("bt-editor-exit-recovered-first", FirstLoad::Recovered);
    assert_eq!(editor.documents().len(), 1);
    let current = editor.current().unwrap();
    assert_eq!(editor.map_of(current).unwrap().layers[0].name, "unsaved");
    assert!(editor.has_unsaved());
    // The session's file first, the work opens beside it, and is the one shown.
    let editor = restart_after_unanswered_exit("bt-editor-exit-session-first", FirstLoad::Session);
    assert_eq!(editor.documents().len(), 2);
    let current = editor.current().unwrap();
    assert_eq!(editor.map_of(current).unwrap().layers[0].name, "unsaved");
}
