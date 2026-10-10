//! Carried selections through the editor: dragging the selected tiles moves them as one step,
//! what they cover shows again when they move on, X, Y and Z mirror and turn them with a tool
//! that selects tiles, Delete drops them, any other edit or selection lets them go where they
//! are, and undo brings the carry back with the selection.

#![cfg(test)]

use bt_core::{Cell, Flip, LayerId, TileId, TilesetSlot};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Intent, SelectionMode, Stroke, StrokeKind, TileRegion,
};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

fn tile(id: u32) -> Cell {
    Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
}

/// An editor with a 4×2 map whose "ground" is `1 2 3 4` over `5 6 7 8`, current.
fn editor() -> (Editor, DocumentId) {
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
        .tileset(atlas.tileset)
        .tile_layer("ground", "1 2 3 4\n5 6 7 8")
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    editor.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(1),
    });
    (editor, doc)
}

/// The ground's cells as tile ids, `0` for none, row by row.
fn ground(editor: &Editor, doc: DocumentId) -> Vec<Vec<u32>> {
    let layer = editor.map_of(doc).unwrap().layers[0]
        .as_tile_layer()
        .unwrap();
    (0..2)
        .map(|y| {
            (0..4)
                .map(|x| {
                    let cell = layer.cell(IVec2::new(x, y));
                    if cell.is_empty() { 0 } else { cell.tile().0 }
                })
                .collect()
        })
        .collect()
}

fn select(editor: &mut Editor, doc: DocumentId, rect: IRect) {
    editor.apply(Intent::SelectTiles {
        doc,
        region: TileRegion::from_rect(rect),
        mode: SelectionMode::Replace,
    });
}

fn cells(x: i32, y: i32, w: i32, h: i32) -> IRect {
    IRect::from_min_size(IVec2::new(x, y), IVec2::new(w, h))
}

/// Drags the selected tiles by each of `deltas` in turn, as one stroke.
fn drag(editor: &mut Editor, doc: DocumentId, deltas: &[IVec2]) -> Vec<Effect> {
    let mut effects = editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Move,
    }));
    for delta in deltas {
        effects.extend(editor.apply(Intent::Stroke(Stroke::Move { delta: *delta })));
    }
    effects.extend(editor.apply(Intent::Stroke(Stroke::End)));
    effects
}

fn selection(editor: &Editor, doc: DocumentId) -> bt_editor::Selection {
    editor.document(doc).unwrap().selection().clone()
}

#[test]
fn a_dragged_selection_carries_its_tiles_as_one_step() {
    let (mut editor, doc) = editor();
    select(&mut editor, doc, cells(0, 0, 2, 1));
    drag(
        &mut editor,
        doc,
        &[IVec2::new(1, 0), IVec2::new(1, 1), IVec2::new(2, 1)],
    );
    assert_eq!(ground(&editor, doc), [[0, 0, 3, 4], [5, 6, 1, 2]]);
    let carried = selection(&editor, doc);
    assert!(carried.carries_tiles());
    assert_eq!(carried.tiles, TileRegion::from_rect(cells(2, 1, 2, 1)));

    // A second drag goes on from there; what the tiles covered shows again.
    drag(&mut editor, doc, &[IVec2::new(-1, 0)]);
    assert_eq!(ground(&editor, doc), [[0, 0, 3, 4], [5, 1, 2, 8]]);

    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(ground(&editor, doc), [[0, 0, 3, 4], [5, 6, 1, 2]]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(ground(&editor, doc), [[1, 2, 3, 4], [5, 6, 7, 8]]);
    assert_eq!(
        selection(&editor, doc).tiles,
        TileRegion::from_rect(cells(0, 0, 2, 1))
    );
    assert!(
        !selection(&editor, doc).carries_tiles(),
        "as before the lift"
    );
    editor.apply(Intent::Run(ActionId::Redo));
    assert!(
        selection(&editor, doc).carries_tiles(),
        "redo carries again"
    );
    assert_eq!(ground(&editor, doc), [[0, 0, 3, 4], [5, 6, 1, 2]]);
}

#[test]
fn x_y_and_z_mirror_and_turn_the_selected_tiles_with_a_selection_tool() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::Run(ActionId::RectSelectTool));
    select(&mut editor, doc, cells(0, 0, 2, 1));
    editor.apply(Intent::Run(ActionId::FlipHorizontal));
    let layer = |editor: &Editor| {
        editor.map_of(doc).unwrap().layers[0]
            .as_tile_layer()
            .unwrap()
            .clone()
    };
    assert_eq!(layer(&editor).cell(IVec2::ZERO), tile(2).with_flip(Flip::H));
    assert_eq!(
        layer(&editor).cell(IVec2::new(1, 0)),
        tile(1).with_flip(Flip::H)
    );
    assert!(selection(&editor, doc).carries_tiles());
    editor.apply(Intent::Run(ActionId::FlipHorizontal));
    assert_eq!(ground(&editor, doc), [[1, 2, 3, 4], [5, 6, 7, 8]]);
    assert_eq!(layer(&editor).cell(IVec2::ZERO), tile(1), "flags as lifted");

    // Four quarter turns give the tiles back; each is a step.
    for _ in 0..4 {
        editor.apply(Intent::Run(ActionId::RotateRight));
    }
    assert_eq!(layer(&editor).cell(IVec2::ZERO), tile(1));
    assert_eq!(layer(&editor).cell(IVec2::new(1, 0)), tile(2));
    editor.apply(Intent::Run(ActionId::Undo));
    assert_ne!(ground(&editor, doc), [[1, 2, 3, 4], [5, 6, 7, 8]]);

    // With the brush, X mirrors the brush and the map stays.
    editor.apply(Intent::Run(ActionId::Copy));
    editor.apply(Intent::Run(ActionId::Paste));
    let before = ground(&editor, doc);
    editor.apply(Intent::Run(ActionId::FlipHorizontal));
    assert_eq!(ground(&editor, doc), before);
}

#[test]
fn delete_drops_the_carried_tiles_and_shows_what_they_covered() {
    let (mut editor, doc) = editor();
    select(&mut editor, doc, cells(0, 0, 1, 1));
    drag(&mut editor, doc, &[IVec2::new(1, 0)]);
    assert_eq!(ground(&editor, doc), [[0, 1, 3, 4], [5, 6, 7, 8]]);
    editor.apply(Intent::Run(ActionId::Copy));
    editor.apply(Intent::Run(ActionId::Delete));
    assert_eq!(ground(&editor, doc), [[0, 2, 3, 4], [5, 6, 7, 8]]);
    assert!(!selection(&editor, doc).carries_tiles());
    // Copy took the carried tile, not what it covered.
    editor.apply(Intent::Run(ActionId::PasteInPlace));
    assert_eq!(ground(&editor, doc), [[0, 1, 3, 4], [5, 6, 7, 8]]);
}

#[test]
fn another_edit_or_selection_lets_the_tiles_go_where_they_are() {
    let (mut editor, doc) = editor();
    select(&mut editor, doc, cells(0, 0, 1, 1));
    drag(&mut editor, doc, &[IVec2::new(1, 0)]);
    // Painting elsewhere lets go: a drag now lifts what is there, the covered 2 stays gone.
    editor.apply(Intent::PickTiles {
        doc,
        cells: vec![bt_core::PlacedCell {
            at: IVec2::ZERO,
            cell: tile(9),
        }],
    });
    editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Paint,
    }));
    editor.apply(Intent::Stroke(Stroke::Paint {
        points: vec![IVec2::new(1, 0)],
        line: false,
    }));
    editor.apply(Intent::Stroke(Stroke::End));
    assert!(!selection(&editor, doc).carries_tiles());
    assert_eq!(ground(&editor, doc), [[0, 9, 3, 4], [5, 6, 7, 8]]);
    // Undo brings the carry back with the selection.
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(selection(&editor, doc).carries_tiles());
    drag(&mut editor, doc, &[IVec2::new(1, 0)]);
    assert_eq!(ground(&editor, doc), [[0, 2, 1, 4], [5, 6, 7, 8]]);

    // Selecting other cells lets go too; Select None keeps the tiles where they are.
    editor.apply(Intent::Run(ActionId::SelectNone));
    assert!(!selection(&editor, doc).carries_tiles());
    assert_eq!(ground(&editor, doc), [[0, 2, 1, 4], [5, 6, 7, 8]]);
}

#[test]
fn locked_layers_keep_their_tiles_and_say_so() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: true,
    });
    select(&mut editor, doc, cells(0, 0, 1, 1));
    let effects = drag(&mut editor, doc, &[IVec2::new(1, 0)]);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Notify(notice) if notice.text.contains("“ground” is locked")
        )),
        "{effects:?}"
    );
    assert_eq!(ground(&editor, doc), [[1, 2, 3, 4], [5, 6, 7, 8]]);
    assert!(!selection(&editor, doc).carries_tiles());
    assert!(!editor.in_gesture(), "no move began");
}
