#![cfg(test)]
//! The live link's side of the editor (`05-editor-core.md`, §17): a script's result lands as one
//! step only at the version it started from, `bt undo` takes back only the latest external
//! edit, and marks replace, add, move and clear.

mod common;

use bt_core::{Cell, LayerId, TileId, TilesetSlot};
use bt_edit::TileRegion;
use bt_editor::{
    DocumentId, Editor, ExternalEdit, ExternalEditError, ExternalUndo, ExternalUndoError, Intent,
    MarkCells, SelectionMode,
};
use bt_test_data::Scratch;
use bt_utils::IRect;
use glam::IVec2;

use common::{apply, editor_in, write_map};

fn opened(scratch: &Scratch) -> (Editor, DocumentId) {
    let map = write_map(scratch.path(), "a.tmx");
    let mut editor = editor_in(scratch);
    apply(&mut editor, Intent::Open(map));
    let doc = editor.current().unwrap();
    (editor, doc)
}

/// The base's map with tile `tile` of the first tileset at `at` on the ground layer.
fn painted(editor: &Editor, doc: DocumentId, at: IVec2, tile: u32, label: &str) -> ExternalEdit {
    let base = editor.prepare_external_edit(doc).unwrap();
    let mut map = bt_core::Map::clone(&base.painted);
    map.layer_mut(LayerId(1))
        .and_then(|layer| layer.as_tile_layer_mut())
        .unwrap()
        .set_cell(at, Cell::new(TilesetSlot::new(0).unwrap(), TileId(tile)))
        .unwrap();
    ExternalEdit {
        doc,
        base_version: base.version,
        painted: map,
        setup: base.setup.clone(),
        label: label.to_owned(),
    }
}

fn ground_tile(editor: &Editor, doc: DocumentId, at: IVec2) -> u32 {
    let map = editor.map_of(doc).unwrap();
    map.layer(LayerId(1))
        .and_then(|layer| layer.as_tile_layer())
        .unwrap()
        .cell(at)
        .tile()
        .0
}

#[test]
fn an_edit_lands_as_one_step_labelled_by_the_script_and_bt_undo_takes_it_back() {
    let scratch = Scratch::new("external-one-step").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let before = ground_tile(&editor, doc, IVec2::ZERO);
    let edit = painted(&editor, doc, IVec2::ZERO, 7, "widen the lake");
    editor.apply_external_edit(edit).unwrap();
    assert_eq!(ground_tile(&editor, doc, IVec2::ZERO), 7);
    let history = editor.document(doc).unwrap().history();
    assert_eq!(history.undoable().unwrap().label(), "widen the lake");
    assert_eq!(history.entries().len(), 1, "one step");
    let asset = editor.document(doc).unwrap().asset();
    assert!(
        editor.store().get(asset).unwrap().is_dirty(),
        "the edit is not saved"
    );

    assert_eq!(
        editor.external_undo(doc).unwrap(),
        ExternalUndo::Undone {
            label: "widen the lake".into(),
            earlier: 0
        }
    );
    assert_eq!(ground_tile(&editor, doc, IVec2::ZERO), before);
    assert_eq!(editor.external_undo(doc).unwrap(), ExternalUndo::Nothing);
}

#[test]
fn an_edit_of_a_map_changed_since_its_base_is_refused_as_a_conflict() {
    let scratch = Scratch::new("external-conflict").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let edit = painted(&editor, doc, IVec2::ZERO, 7, "script");
    editor.apply(Intent::SetLayerVisible {
        doc,
        layer: LayerId(2),
        visible: false,
    });
    assert_eq!(
        editor.apply_external_edit(edit).err(),
        Some(ExternalEditError::Conflict)
    );
    let history = editor.document(doc).unwrap().history();
    assert_eq!(history.undoable().unwrap().label(), "Hide Layer");

    // Run again on the new base, it lands.
    let edit = painted(&editor, doc, IVec2::ZERO, 7, "script");
    editor.apply_external_edit(edit).unwrap();
    assert_eq!(ground_tile(&editor, doc, IVec2::ZERO), 7);
}

#[test]
fn an_edit_waits_while_a_stroke_is_under_way() {
    let scratch = Scratch::new("external-busy").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let edit = painted(&editor, doc, IVec2::ZERO, 7, "script");
    editor.begin_gesture(doc, "Paint").unwrap();
    assert!(editor.is_stroking());
    assert_eq!(
        editor.apply_external_edit(edit.clone()).err(),
        Some(ExternalEditError::Busy)
    );
    assert!(matches!(
        editor.external_undo(doc),
        Err(ExternalUndoError::Busy)
    ));
    editor.end_gesture().unwrap();
    editor.apply_external_edit(edit).unwrap();
    assert_eq!(ground_tile(&editor, doc, IVec2::ZERO), 7);
}

#[test]
fn bt_undo_takes_back_the_latest_external_edit_only() {
    let scratch = Scratch::new("external-latest").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let first = painted(&editor, doc, IVec2::ZERO, 7, "first");
    editor.apply_external_edit(first).unwrap();
    let second = painted(&editor, doc, IVec2::new(1, 0), 8, "second");
    editor.apply_external_edit(second).unwrap();
    editor.apply(Intent::SetLayerVisible {
        doc,
        layer: LayerId(2),
        visible: false,
    });
    assert_eq!(
        editor.external_undo(doc),
        Err(ExternalUndoError::NotLatest {
            edit: "second".into(),
            latest: "Hide Layer".into(),
        })
    );
    assert_eq!(
        ground_tile(&editor, doc, IVec2::new(1, 0)),
        8,
        "nothing undone"
    );

    // Once the person's change is undone in the editor, the external edits go back one by one.
    editor.undo(doc).unwrap();
    assert_eq!(
        editor.external_undo(doc).unwrap(),
        ExternalUndo::Undone {
            label: "second".into(),
            earlier: 1
        }
    );
    assert_eq!(
        editor.external_undo(doc).unwrap(),
        ExternalUndo::Undone {
            label: "first".into(),
            earlier: 0
        }
    );
    assert_ne!(ground_tile(&editor, doc, IVec2::ZERO), 7);
}

#[test]
fn an_external_edit_undone_in_the_editor_is_said_to_be_undone_already() {
    let scratch = Scratch::new("external-undone").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let edit = painted(&editor, doc, IVec2::ZERO, 7, "script");
    editor.apply_external_edit(edit).unwrap();
    editor.undo(doc).unwrap();
    assert_eq!(
        editor.external_undo(doc).unwrap(),
        ExternalUndo::AlreadyUndone {
            label: "script".into()
        }
    );
    assert_eq!(editor.external_undo(doc).unwrap(), ExternalUndo::Nothing);
}

#[test]
fn the_base_holds_the_selection_the_tilesets_and_the_map_as_shown() {
    let scratch = Scratch::new("external-base").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let rect = IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 1));
    editor.apply(Intent::SelectTiles {
        doc,
        region: TileRegion::from_rect(rect),
        mode: SelectionMode::Replace,
    });
    let base = editor.prepare_external_edit(doc).unwrap();
    assert_eq!(base.selected_cells.rects(), [rect]);
    assert!(base.selected_objects.is_empty());
    assert_eq!(base.tilesets.len(), 1);
    assert!(base.tilesets[0].path.ends_with("tiles.tsx"));
    assert!(std::sync::Arc::ptr_eq(&base.painted, &base.shown));
    assert!(base.setup.is_none());
    assert!(base.path.unwrap().ends_with("a.tmx"));
    assert_eq!(editor.document_at(&scratch.join("a.tmx")), Some(doc));
}

#[test]
fn marks_replace_add_move_and_clear() {
    let scratch = Scratch::new("external-marks").unwrap();
    let (mut editor, doc) = opened(&scratch);
    let mark = |label: &str, cells: &[(i32, i32)]| MarkCells {
        label: label.to_owned(),
        cells: cells.iter().map(|&(x, y)| IVec2::new(x, y)).collect(),
    };
    editor.set_marks(doc, vec![mark("a", &[(1, 0), (0, 0), (1, 0)])], false);
    assert_eq!(
        editor.marks(doc)[0].cells,
        [IVec2::ZERO, IVec2::X],
        "sorted, once"
    );
    editor.set_marks(doc, vec![mark("b", &[(2, 1)])], true);
    let colors: Vec<u8> = editor.marks(doc).iter().map(|m| m.color).collect();
    assert_eq!(colors, [0, 1], "each mark takes the next colour");

    // Marking a label again moves that mark, keeping its colour.
    editor.set_marks(doc, vec![mark("a", &[(2, 0)])], true);
    let a = editor.marks(doc).iter().find(|m| m.label == "a").unwrap();
    assert_eq!(
        (a.cells.as_slice(), a.color),
        ([IVec2::new(2, 0)].as_slice(), 0)
    );
    assert_eq!(editor.marks(doc).len(), 2);

    // Without `add`, the marks given replace the others.
    editor.set_marks(doc, vec![mark("c", &[(0, 1)])], false);
    let labels: Vec<&str> = editor.marks(doc).iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, ["c"]);
    editor.set_marks(doc, Vec::new(), false);
    assert!(editor.marks(doc).is_empty());

    editor.set_marks(doc, vec![mark("d", &[(0, 0)])], false);
    apply(&mut editor, Intent::CloseDocument(doc));
    assert!(
        editor.marks(doc).is_empty(),
        "a closed document forgets them"
    );
}
