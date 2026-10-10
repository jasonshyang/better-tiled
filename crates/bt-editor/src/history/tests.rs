//! Transactions, undo and redo, merging, gestures, and the store's authority on tilesets.

use std::sync::Arc;

use bt_core::{FileRef, LayerId, Map, Tileset, TilesetRef};
use bt_utils::rng::Rng;

use crate::document::{DocumentId, Selection};
use crate::editor::Editor;
use crate::error::EditError;
use crate::history::MergeKey;
use crate::store::{AssetId, NewAsset, Root};

fn map(width: u32) -> Map {
    Map {
        width,
        height: 1,
        tile_width: 16,
        tile_height: 16,
        ..Map::default()
    }
}

/// An editor with `count` maps open, each saved to disk as it is.
fn editor_with_maps(count: u32) -> (Editor, Vec<DocumentId>, Vec<AssetId>) {
    let mut editor = Editor::new();
    let mut docs = Vec::new();
    let mut assets = Vec::new();
    for i in 0..count {
        let asset = editor.store.insert(NewAsset {
            root: Root::Map(Arc::new(map(10 + i))),
            path: Some(format!("/maps/{i}.tmx").into()),
            content_hash: Some(u64::from(i)),
            warnings: Vec::new(),
        });
        docs.push(editor.open_document(asset));
        assets.push(asset);
    }
    (editor, docs, assets)
}

fn width(editor: &Editor, asset: AssetId) -> u32 {
    editor.store.map(asset).unwrap().width
}

fn set_width(editor: &mut Editor, doc: DocumentId, asset: AssetId, width: u32) {
    editor
        .transact(doc, "Resize", |tx| {
            tx.map_mut(asset)?.width = width;
            Ok::<_, EditError>(())
        })
        .unwrap();
}

#[test]
fn a_transaction_that_changes_nothing_is_not_recorded() {
    let (mut editor, docs, assets) = editor_with_maps(1);
    editor
        .transact(docs[0], "Nothing", |tx| {
            tx.map_mut(assets[0])?.width = 10;
            tx.selection = Selection::of_layer(LayerId(7));
            Ok::<_, EditError>(())
        })
        .unwrap();
    let document = editor.document(docs[0]).unwrap();
    assert!(document.history().entries().is_empty());
    assert_eq!(
        document.selection().current_layer,
        Some(LayerId(7)),
        "the selection is kept"
    );
    assert!(!editor.store.get(assets[0]).unwrap().is_dirty());
}

#[test]
fn undo_back_to_the_saved_version_is_clean_and_new_edits_are_not() {
    let (mut editor, docs, assets) = editor_with_maps(1);
    let (doc, asset) = (docs[0], assets[0]);
    set_width(&mut editor, doc, asset, 20);
    assert!(editor.store.get(asset).unwrap().is_dirty());
    editor.undo(doc).unwrap();
    assert_eq!(width(&editor, asset), 10);
    assert!(
        !editor.store.get(asset).unwrap().is_dirty(),
        "undone to the saved version"
    );
    editor.redo(doc).unwrap();
    assert_eq!(width(&editor, asset), 20);
    editor.undo(doc).unwrap();
    // A different change after the undo must not take the undone change's version.
    set_width(&mut editor, doc, asset, 30);
    let changed = editor.store.get(asset).unwrap();
    assert!(changed.is_dirty());
    assert_eq!(changed.version(), 3);
    assert_eq!(
        editor.redo(doc),
        Err(EditError::NothingToRedo),
        "a new change drops the redo side"
    );
}

#[test]
fn merge_keys_fold_until_the_run_breaks() {
    let (mut editor, docs, assets) = editor_with_maps(1);
    let (doc, asset) = (docs[0], assets[0]);
    for w in [11, 12, 13] {
        editor
            .transact_merging(doc, "Typing", MergeKey("width".into()), |tx| {
                tx.map_mut(asset)?.width = w;
                Ok::<_, EditError>(())
            })
            .unwrap();
    }
    assert_eq!(editor.document(doc).unwrap().history().entries().len(), 1);
    editor.break_merge(doc);
    editor
        .transact_merging(doc, "Typing", MergeKey("width".into()), |tx| {
            tx.map_mut(asset)?.width = 14;
            Ok::<_, EditError>(())
        })
        .unwrap();
    assert_eq!(editor.document(doc).unwrap().history().entries().len(), 2);
    editor.undo(doc).unwrap();
    assert_eq!(width(&editor, asset), 13);
    editor.undo(doc).unwrap();
    assert_eq!(width(&editor, asset), 10, "the folded run undoes as one");
}

/// Two maps using one tileset file, the tileset an asset of its own.
fn shared_tileset() -> (Editor, [DocumentId; 2], [AssetId; 2], AssetId) {
    let mut editor = Editor::new();
    let tileset = Arc::new(Tileset {
        name: "shared".into(),
        tile_width: 16,
        tile_height: 16,
        ..Tileset::default()
    });
    let tileset_asset = editor.store.insert(NewAsset {
        root: Root::Tileset(Arc::clone(&tileset)),
        path: Some("/maps/shared.tsx".into()),
        content_hash: Some(1),
        warnings: Vec::new(),
    });
    let mut docs = [DocumentId(0); 2];
    let mut maps = [AssetId(0); 2];
    for i in 0..2 {
        let mut m = map(10);
        m.tilesets.push(TilesetRef {
            file: Some(FileRef::new("shared.tsx", "/maps/shared.tsx")),
            first_gid: Some(1),
            tileset: Arc::clone(&tileset),
        });
        let asset = editor.store.insert(NewAsset {
            root: Root::Map(Arc::new(m)),
            path: Some(format!("/maps/{i}.tmx").into()),
            content_hash: Some(2),
            warnings: Vec::new(),
        });
        docs[i] = editor.open_document(asset);
        maps[i] = asset;
    }
    (editor, docs, maps, tileset_asset)
}

fn rename_tileset(
    editor: &mut Editor,
    doc: DocumentId,
    tileset: AssetId,
    name: &str,
    label: &str,
) -> Result<(), EditError> {
    editor.transact(doc, label, |tx| {
        tx.tileset_mut(tileset)?.name = name.to_owned();
        Ok(())
    })
}

#[test]
fn a_tileset_edit_shows_in_every_map_and_later_edits_win() {
    let (mut editor, docs, maps, tileset) = shared_tileset();
    rename_tileset(&mut editor, docs[0], tileset, "renamed", "Rename Tileset").unwrap();
    for map in maps {
        let shown = &editor.store.map(map).unwrap().tilesets[0].tileset;
        assert_eq!(shown.name, "renamed");
        assert!(
            !editor.store.get(map).unwrap().is_dirty(),
            "the map did not change, its tileset did"
        );
    }
    assert!(editor.store.get(tileset).unwrap().is_dirty());
    rename_tileset(&mut editor, docs[1], tileset, "again", "Rename Again").unwrap();
    assert_eq!(
        editor.undo(docs[0]),
        Err(EditError::Blocked {
            asset: tileset,
            document: docs[1],
            label: "Rename Again".into()
        })
    );
    editor.undo(docs[1]).unwrap();
    editor.undo(docs[0]).unwrap();
    for map in maps {
        assert_eq!(
            editor.store.map(map).unwrap().tilesets[0].tileset.name,
            "shared"
        );
    }
    assert!(!editor.store.get(tileset).unwrap().is_dirty());
}

#[test]
fn restoring_a_map_takes_the_stores_tileset() {
    let (mut editor, docs, maps, tileset) = shared_tileset();
    set_width(&mut editor, docs[0], maps[0], 99);
    rename_tileset(&mut editor, docs[1], tileset, "later", "Rename").unwrap();
    // Undoing the map's resize brings back the old map root, but with today's tileset.
    editor.undo(docs[0]).unwrap();
    let restored = editor.store.map(maps[0]).unwrap();
    assert_eq!(restored.width, 10);
    assert_eq!(restored.tilesets[0].tileset.name, "later");
}

#[test]
fn gestures_record_once_and_cancel_cleanly() {
    let (mut editor, docs, assets) = editor_with_maps(1);
    let (doc, asset) = (docs[0], assets[0]);
    editor.begin_gesture(doc, "Stroke").unwrap();
    for w in [11, 12, 13] {
        editor
            .gesture_apply(|tx| {
                tx.map_mut(asset)?.width = w;
                Ok::<_, EditError>(())
            })
            .unwrap();
        assert_eq!(width(&editor, asset), w, "applied live");
    }
    assert_eq!(
        set_width_result(&mut editor, doc, asset),
        Err(EditError::GestureUnderWay)
    );
    assert!(editor.end_gesture().unwrap());
    assert_eq!(editor.document(doc).unwrap().history().entries().len(), 1);
    editor.undo(doc).unwrap();
    assert_eq!(width(&editor, asset), 10);
    assert!(!editor.store.get(asset).unwrap().is_dirty());

    editor.begin_gesture(doc, "Wobble").unwrap();
    editor
        .gesture_apply(|tx| {
            tx.map_mut(asset)?.width = 50;
            Ok::<_, EditError>(())
        })
        .unwrap();
    editor
        .gesture_apply(|tx| {
            tx.map_mut(asset)?.width = 10;
            Ok::<_, EditError>(())
        })
        .unwrap();
    assert!(!editor.end_gesture().unwrap(), "back where it started");
    assert!(!editor.store.get(asset).unwrap().is_dirty());

    editor.begin_gesture(doc, "Cancelled").unwrap();
    editor
        .gesture_apply(|tx| {
            tx.map_mut(asset)?.width = 70;
            Ok::<_, EditError>(())
        })
        .unwrap();
    editor.cancel_gesture().unwrap();
    assert_eq!(width(&editor, asset), 10);
    assert!(!editor.store.get(asset).unwrap().is_dirty());
}

fn set_width_result(editor: &mut Editor, doc: DocumentId, asset: AssetId) -> Result<(), EditError> {
    editor.transact(doc, "Resize", |tx| {
        tx.map_mut(asset)?.width = 1;
        Ok(())
    })
}

/// A plain model of assets with versions and documents with histories.
#[derive(Clone, Debug, Default)]
struct Model {
    values: Vec<u32>,
    versions: Vec<u64>,
    high_water: Vec<u64>,
    saved: Vec<u64>,
    histories: Vec<ModelHistory>,
}

#[derive(Clone, Debug, Default)]
struct ModelHistory {
    entries: Vec<ModelEntry>,
    position: usize,
}

#[derive(Clone, Copy, Debug)]
struct ModelEntry {
    asset: usize,
    before: u32,
    after: u32,
    before_version: u64,
    after_version: u64,
}

#[test]
fn random_edits_undos_and_redos_follow_a_plain_model() {
    let seed: u64 = std::env::var("BT_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);
    let cases: usize = std::env::var("BT_FUZZ_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);
    let mut rng = Rng::new(seed);
    let (mut editor, docs, assets) = editor_with_maps(3);
    let mut model = Model {
        values: vec![10, 11, 12],
        versions: vec![1; 3],
        high_water: vec![1; 3],
        saved: vec![1; 3],
        histories: vec![ModelHistory::default(); 3],
    };
    for step in 0..cases {
        let doc = rng.below(3).unwrap() as usize;
        match rng.below(4).unwrap() {
            0 | 1 => {
                let asset = rng.below(3).unwrap() as usize;
                let value = 10 + rng.below(5).unwrap() as u32;
                set_width(&mut editor, docs[doc], assets[asset], value);
                if model.values[asset] != value {
                    model.high_water[asset] += 1;
                    let entry = ModelEntry {
                        asset,
                        before: model.values[asset],
                        after: value,
                        before_version: model.versions[asset],
                        after_version: model.high_water[asset],
                    };
                    model.values[asset] = value;
                    model.versions[asset] = entry.after_version;
                    let history = &mut model.histories[doc];
                    history.entries.truncate(history.position);
                    history.entries.push(entry);
                    history.position += 1;
                }
            }
            2 => {
                let result = editor.undo(docs[doc]);
                let history = &mut model.histories[doc];
                let expected_ok = history.position > 0 && {
                    let entry = history.entries[history.position - 1];
                    model.versions[entry.asset] == entry.after_version
                };
                assert_eq!(
                    result.is_ok(),
                    expected_ok,
                    "step {step}: undo in {doc}: {result:?}"
                );
                if expected_ok {
                    history.position -= 1;
                    let entry = history.entries[history.position];
                    model.values[entry.asset] = entry.before;
                    model.versions[entry.asset] = entry.before_version;
                }
            }
            _ => {
                let result = editor.redo(docs[doc]);
                let history = &mut model.histories[doc];
                let expected_ok = history.position < history.entries.len() && {
                    let entry = history.entries[history.position];
                    model.versions[entry.asset] == entry.before_version
                };
                assert_eq!(
                    result.is_ok(),
                    expected_ok,
                    "step {step}: redo in {doc}: {result:?}"
                );
                if expected_ok {
                    let entry = history.entries[history.position];
                    history.position += 1;
                    model.values[entry.asset] = entry.after;
                    model.versions[entry.asset] = entry.after_version;
                }
            }
        }
        for (index, asset) in assets.iter().enumerate() {
            let actual = editor.store.get(*asset).unwrap();
            assert_eq!(
                width(&editor, *asset),
                model.values[index],
                "step {step}, seed {seed}"
            );
            assert_eq!(
                actual.version(),
                model.versions[index],
                "step {step}, seed {seed}"
            );
            assert_eq!(
                actual.is_dirty(),
                model.versions[index] != model.saved[index],
                "step {step}"
            );
        }
        for (doc, history) in docs.iter().zip(&model.histories) {
            assert_eq!(
                editor.document(*doc).unwrap().history().position(),
                history.position
            );
        }
    }
}
