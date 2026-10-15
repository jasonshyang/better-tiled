//! Library writes, read failures and quick-stamp session recovery, without a window or GPU.

#![cfg(test)]
mod common;

use bt_editor::{ActionId, Effect, Intent, Session, StampEdit};
use bt_test_data::Scratch;
use bt_utils::IRect;
use common::{apply, editor_in, write_map};
use glam::IVec2;

fn capture(editor: &mut bt_editor::Editor) {
    let doc = editor.current().unwrap();
    let map = editor.map_of(doc).unwrap();
    let layers = map.layers.iter().map(|layer| layer.id).collect::<Vec<_>>();
    let current = layers[0];
    apply(
        editor,
        Intent::SelectLayers {
            doc,
            layers,
            current,
        },
    );
    apply(
        editor,
        Intent::Capture {
            doc,
            rect: IRect::from_min_size(IVec2::ZERO, IVec2::new(3, 2)),
            cut: false,
        },
    );
}

fn stamp_job(effects: Vec<Effect>) -> bt_editor::StampJob {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Stamps(job) => Some(job),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_late_folder_load_cannot_replace_a_new_folder_and_worker_failure_releases_busy() {
    let scratch = Scratch::new("stamp-worker-owners").unwrap();
    let mut editor = editor_in(&scratch);
    let old = stamp_job(editor.apply(Intent::Stamp(StampEdit::Reload)));
    let folder = scratch.join("other");
    std::fs::create_dir(&folder).unwrap();
    apply(
        &mut editor,
        Intent::FilePicked {
            request: bt_editor::FileRequest::StampsFolder,
            path: Some(folder.clone()),
        },
    );
    editor.apply(Intent::StampsLoaded(old.run()));
    assert_eq!(editor.stamps().folder(), folder);
    let failed = stamp_job(editor.apply(Intent::Stamp(StampEdit::Reload)));
    assert!(editor.stamps().busy());
    let effects = editor.apply(Intent::StampsLoaded(failed.failed("no worker".into())));
    assert!(!editor.stamps().busy());
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Notify(notice) if notice.text == "no worker"))
    );
}

#[test]
fn a_late_session_restore_cannot_replace_a_quick_stamp_saved_after_it_started() {
    let scratch = Scratch::new("quick-stamp-worker-owner").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let doc = editor.current().unwrap();
    let ground = editor.map_of(doc).unwrap().layers[0].id;
    apply(&mut editor, Intent::SelectLayer { doc, layer: ground });
    apply(
        &mut editor,
        Intent::PickFromMap {
            doc,
            at: IVec2::ZERO,
        },
    );
    apply(&mut editor, Intent::Stamp(StampEdit::SaveQuick(0)));
    let restore = stamp_job(editor.restore_session(editor.session()));
    apply(&mut editor, Intent::PickFromMap { doc, at: IVec2::X });
    apply(&mut editor, Intent::Stamp(StampEdit::SaveQuick(0)));
    editor.apply(Intent::StampsLoaded(restore.run()));
    apply(&mut editor, Intent::Stamp(StampEdit::ChooseQuick(0)));
    assert_eq!(
        editor.tools().brush().unwrap().variations()[0].clip.layers[0]
            .cells
            .cell(IVec2::ZERO)
            .tile(),
        bt_core::TileId(6)
    );
}

#[test]
fn stamps_save_reload_rename_extend_choose_and_delete_without_dirtying_the_map() {
    let scratch = Scratch::new("editor-stamps").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    capture(&mut editor);
    let before = editor.tools().brush().unwrap().clone();
    assert!(apply(&mut editor, Intent::Stamp(StampEdit::Save("hill".into()))).is_empty());
    assert!(!editor.stamps().busy());
    let stamp = editor.stamps().entries()[0].clone();
    assert_eq!(stamp.stamp.variations[0].map.layers.len(), 2);
    assert!(stamp.path.is_file());
    apply(
        &mut editor,
        Intent::Stamp(StampEdit::Rename {
            path: stamp.path.clone(),
            name: "ridge".into(),
        }),
    );
    apply(
        &mut editor,
        Intent::Stamp(StampEdit::AddVariation(stamp.path.clone())),
    );
    apply(
        &mut editor,
        Intent::Stamp(StampEdit::Probability {
            path: stamp.path.clone(),
            variation: 0,
            probability: 0.25,
        }),
    );
    assert_eq!(editor.stamps().entries()[0].stamp.variations.len(), 2);
    assert_eq!(
        editor.stamps().entries()[0].stamp.variations[0].probability,
        0.25
    );
    let mut reopened = editor_in(&scratch);
    apply(&mut reopened, Intent::Stamp(StampEdit::Reload));
    assert_eq!(reopened.stamps().entries()[0].stamp.name, "ridge");
    apply(
        &mut editor,
        Intent::Stamp(StampEdit::Choose {
            path: stamp.path.clone(),
            variation: Some(1),
        }),
    );
    let chosen = &editor.tools().brush().unwrap().variations()[0].clip;
    assert_eq!(chosen.layers, before.variations()[0].clip.layers);
    assert_eq!(chosen.size, before.variations()[0].clip.size);
    assert_eq!(
        chosen.tilesets[0].file.as_ref().unwrap().path,
        before.variations()[0].clip.tilesets[0]
            .file
            .as_ref()
            .unwrap()
            .path
    );
    assert!(
        !editor.has_unsaved(),
        "the library is separate from map history"
    );
    apply(
        &mut editor,
        Intent::Stamp(StampEdit::Delete(stamp.path.clone())),
    );
    assert!(!stamp.path.exists());
    assert!(editor.stamps().entries().is_empty());
}

#[test]
fn failed_writes_keep_the_library_and_a_bad_file_does_not_hide_other_stamps() {
    let scratch = Scratch::new("editor-stamps-refused").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    capture(&mut editor);
    apply(&mut editor, Intent::Stamp(StampEdit::Save("hill".into())));
    let saved = editor.stamps().entries()[0].clone();
    let bytes = std::fs::read(&saved.path).unwrap();
    let mut permissions = std::fs::metadata(&saved.path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&saved.path, permissions).unwrap();
    let effects = apply(
        &mut editor,
        Intent::Stamp(StampEdit::Rename {
            path: saved.path.clone(),
            name: "bad".into(),
        }),
    );
    assert!(effects.iter().any(
        |effect| matches!(effect, Effect::Notify(notice) if notice.text.contains("read-only"))
    ));
    assert_eq!(editor.stamps().entries()[0].stamp.name, "hill");
    assert_eq!(std::fs::read(&saved.path).unwrap(), bytes);
    assert!(!editor.stamps().busy());
    std::fs::write(editor.stamps().folder().join("broken.stamp"), "{").unwrap();
    apply(&mut editor, Intent::Stamp(StampEdit::Reload));
    assert_eq!(editor.stamps().entries().len(), 1);
    assert_eq!(editor.stamps().problems().len(), 1);
    let effects = apply(
        &mut editor,
        Intent::Stamp(StampEdit::Save("../outside".into())),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Notify(_)))
    );
    assert!(!scratch.join("outside.stamp").exists());
}

#[test]
fn all_quick_slots_and_weights_survive_session_restore_with_resolved_references() {
    let scratch = Scratch::new("editor-quick-stamps").unwrap();
    let path = write_map(scratch.path(), "map.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    capture(&mut editor);
    for slot in 0..9 {
        apply(&mut editor, Intent::Stamp(StampEdit::SaveQuick(slot)));
    }
    apply(&mut editor, Intent::Stamp(StampEdit::ExtendQuick(3)));
    let session = Session::parse(&editor.session().to_json().unwrap()).unwrap();
    let mut restored = editor_in(&scratch);
    for effect in restored.restore_session(session) {
        match effect {
            Effect::Load(job) => {
                apply(&mut restored, Intent::Loaded(Box::new(job.run())));
            }
            Effect::Stamps(job) => {
                apply(&mut restored, Intent::StampsLoaded(job.run()));
            }
            other => panic!("unexpected restoration effect: {other:?}"),
        }
    }
    for slot in 0..9 {
        assert!(restored.tools().quick_stamp(slot).is_some());
        apply(&mut restored, Intent::Stamp(StampEdit::ChooseQuick(slot)));
        let brush = restored.tools().brush().unwrap();
        assert_eq!(brush.variations().len(), if slot == 3 { 2 } else { 1 });
        assert_eq!(
            brush.variations()[0].clip.tilesets[0]
                .file
                .as_ref()
                .unwrap()
                .path,
            scratch.join("tiles.tsx")
        );
    }
    assert!(!restored.has_unsaved());
    apply(&mut restored, Intent::Run(ActionId::Save));
}
