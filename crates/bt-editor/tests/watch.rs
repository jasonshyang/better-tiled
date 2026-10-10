//! The watcher's decisions on a temporary folder: a clean map reloads as a step that undoes, a
//! map with unsaved changes asks, our own saves are recognised, a deleted file leaves the tab as
//! the only copy, and changed images are decoded again.

#![cfg(test)]

mod common;

use std::path::Path;

use bt_core::LayerId;
use bt_editor::{ActionId, Choice, Editor, Effect, Found, Intent, Observation, Question};
use bt_test_data::Scratch;
use bt_utils::hash::fnv1a_64;
use common::{apply, ask, editor_in, write_map};

/// What the window's poll finds: every watched file, hashed.
fn poll(editor: &mut Editor) -> Vec<Effect> {
    let observations = editor
        .watched()
        .into_iter()
        .map(|path| {
            let found = match std::fs::read(&path) {
                Ok(bytes) => Found::Present {
                    content_hash: fnv1a_64(&bytes),
                },
                Err(_) => Found::Missing,
            };
            Observation { path, found }
        })
        .collect();
    apply(editor, Intent::Observed(observations))
}

/// The map file rewritten by someone else: a layer renamed.
fn rename_on_disk(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let renamed = text.replace(&format!("name=\"{from}\""), &format!("name=\"{to}\""));
    assert_ne!(renamed, text, "the file has a layer “{from}”");
    std::fs::write(path, renamed).unwrap();
}

fn layer_name(editor: &Editor, id: u32) -> String {
    let doc = editor.current().unwrap();
    editor
        .map_of(doc)
        .unwrap()
        .layer(LayerId(id))
        .unwrap()
        .name
        .clone()
}

#[test]
fn a_clean_map_reloads_as_a_step_that_undoes() {
    let scratch = Scratch::new("bt-editor-watch-reload").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    assert!(editor.watched().contains(&path));
    assert!(poll(&mut editor).is_empty(), "nothing changed");

    rename_on_disk(&path, "top", "changed");
    let effects = poll(&mut editor);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Notify(n) if n.text.contains("reloaded"))),
        "{effects:?}"
    );
    assert_eq!(layer_name(&editor, 2), "changed");
    assert!(!editor.has_unsaved(), "what is on disk now");
    assert!(
        poll(&mut editor).is_empty(),
        "the reload is not reloaded again"
    );

    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(layer_name(&editor, 2), "top");
    assert!(
        editor.has_unsaved(),
        "the old content is now an unsaved change"
    );
}

#[test]
fn unsaved_changes_ask_and_keeping_them_asks_once() {
    let scratch = Scratch::new("bt-editor-watch-conflict").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::SetLayerVisible {
            doc,
            layer: LayerId(1),
            visible: false,
        },
    );
    rename_on_disk(&path, "top", "theirs");
    let effects = poll(&mut editor);
    let decision = ask(&effects).clone();
    assert!(matches!(
        decision.question,
        Question::Conflict {
            returned: false,
            ..
        }
    ));
    assert_eq!(decision.choices, [Choice::KeepMine, Choice::Reload]);
    assert!(
        poll(&mut editor).is_empty(),
        "asked once while the question is open"
    );
    apply(
        &mut editor,
        Intent::Decided {
            decision: decision.id,
            choice: Choice::KeepMine,
        },
    );
    assert!(
        poll(&mut editor).is_empty(),
        "the same change is not asked about again"
    );
    assert_eq!(layer_name(&editor, 2), "top");

    // Another change asks again; this time the file wins.
    rename_on_disk(&path, "theirs", "again");
    let effects = poll(&mut editor);
    let decision = ask(&effects).clone();
    apply(
        &mut editor,
        Intent::Decided {
            decision: decision.id,
            choice: Choice::Reload,
        },
    );
    assert_eq!(layer_name(&editor, 2), "again");
    assert!(!editor.has_unsaved());
}

#[test]
fn our_own_save_is_not_a_change_and_a_deleted_file_leaves_the_only_copy() {
    let scratch = Scratch::new("bt-editor-watch-own").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::RenameLayer {
            doc,
            layer: LayerId(2),
            name: "ours".into(),
        },
    );
    apply(&mut editor, Intent::Run(ActionId::Save));
    assert!(poll(&mut editor).is_empty(), "our own write");

    let moved = scratch.join("moved.tmx");
    std::fs::rename(&path, &moved).unwrap();
    let effects = poll(&mut editor);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Notify(n) if n.text.contains("only one"))),
        "{effects:?}"
    );
    assert!(editor.has_unsaved(), "the tab is the only copy");
    assert!(poll(&mut editor).is_empty(), "said once");
    std::fs::rename(&moved, &path).unwrap();
    let effects = poll(&mut editor);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Notify(n) if n.text.contains("is back")))
    );
    assert!(!editor.has_unsaved(), "back unchanged: clean again");
}

#[test]
fn changed_images_are_decoded_again() {
    let scratch = Scratch::new("bt-editor-watch-images").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path));
    let image = editor
        .watched()
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .expect("the tileset's image is watched");
    assert!(poll(&mut editor).is_empty(), "the first look only records");
    let mut bytes = std::fs::read(&image).unwrap();
    bytes.push(0);
    std::fs::write(&image, bytes).unwrap();
    let effects = poll(&mut editor);
    assert!(
        matches!(&effects[..], [Effect::ReloadImages(paths)] if paths == std::slice::from_ref(&image)),
        "{effects:?}"
    );
}

#[test]
fn a_change_while_a_question_is_open_counts_once_it_is_answered() {
    let scratch = Scratch::new("bt-editor-watch-deferred").unwrap();
    let path = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(path.clone()));
    let doc = editor.current().unwrap();
    apply(
        &mut editor,
        Intent::SetLayerVisible {
            doc,
            layer: LayerId(1),
            visible: false,
        },
    );
    rename_on_disk(&path, "top", "first");
    let decision = ask(&poll(&mut editor)).clone();
    // The file changes again while the question is open; the window reports it once.
    rename_on_disk(&path, "first", "second");
    assert!(poll(&mut editor).is_empty());
    let effects = apply(
        &mut editor,
        Intent::Decided {
            decision: decision.id,
            choice: Choice::KeepMine,
        },
    );
    let again = ask(&effects);
    assert!(
        matches!(again.question, Question::Conflict { .. }),
        "the newer change asks"
    );
}
