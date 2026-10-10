//! The tile tools through the editor: picking tiles makes the brush, strokes paint and erase as
//! one step each, previews are what gets painted, fills, the picker and capture, the clipboard,
//! and the Layer menu's commands.

#![cfg(test)]

use bt_core::{Cell, LayerId, PlacedCell, TileId, TilesetSlot};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Enabled, Intent, Stroke, StrokeKind, TileRegion, ToolKind,
};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

fn tile(id: u32) -> Cell {
    Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
}

/// An editor with a 6×4 map: "ground" (all 1s), "top" (empty), "things" (objects).
fn editor() -> (Editor, DocumentId) {
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(6, 4))
        .tileset(atlas.tileset)
        .tile_layer(
            "ground",
            "1 1 1 1 1 1\n1 1 1 1 1 1\n1 1 1 1 1 1\n1 1 1 1 1 1",
        )
        .tile_layer("top", ". . . . . .\n. . . . . .\n. . . . . .\n. . . . . .")
        .object_layer("things", Vec::new())
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

fn cell(editor: &Editor, doc: DocumentId, layer: u32, at: IVec2) -> Cell {
    editor
        .map_of(doc)
        .unwrap()
        .layer(LayerId(layer))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .cell(at)
}

fn pick(editor: &mut Editor, doc: DocumentId, id: u32) {
    editor.apply(Intent::PickTiles {
        doc,
        cells: vec![PlacedCell {
            at: IVec2::ZERO,
            cell: tile(id),
        }],
    });
}

#[test]
fn a_picked_tile_is_the_brush_and_a_stroke_is_one_step() {
    let (mut editor, doc) = editor();
    pick(&mut editor, doc, 5);
    assert_eq!(editor.tools().tool(), ToolKind::Brush);
    let preview = editor.brush_preview(doc, &[IVec2::new(2, 1)], false);
    assert_eq!(
        preview,
        [PlacedCell {
            at: IVec2::new(2, 1),
            cell: tile(5)
        }]
    );
    editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Paint,
    }));
    for x in 2..5 {
        editor.apply(Intent::Stroke(Stroke::Paint {
            points: vec![IVec2::new(x, 1)],
            line: false,
        }));
    }
    editor.apply(Intent::Stroke(Stroke::End));
    for x in 2..5 {
        assert_eq!(cell(&editor, doc, 1, IVec2::new(x, 1)), tile(5));
    }
    assert_eq!(editor.tools().last_paint(), Some(IVec2::new(4, 1)));
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(cell(&editor, doc, 1, IVec2::new(3, 1)), tile(1), "one step");
}

#[test]
fn the_preview_is_what_a_random_brush_paints() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::PickTiles {
        doc,
        cells: (0..4)
            .map(|i| PlacedCell {
                at: IVec2::new(i, 0),
                cell: tile(4 + i as u32),
            })
            .collect(),
    });
    editor.apply(Intent::Run(ActionId::RandomMode));
    let points: Vec<IVec2> = (0..6).map(|x| IVec2::new(x, 2)).collect();
    let preview = editor.brush_preview(doc, &points, true);
    editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Paint,
    }));
    editor.apply(Intent::Stroke(Stroke::Paint {
        points: points.clone(),
        line: true,
    }));
    editor.apply(Intent::Stroke(Stroke::End));
    for placed in preview {
        assert_eq!(cell(&editor, doc, 1, placed.at), placed.cell);
    }
}

#[test]
fn the_eraser_leaves_locked_layers_and_says_so() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: true,
    });
    editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Erase,
    }));
    editor.apply(Intent::Stroke(Stroke::Erase {
        points: vec![IVec2::new(0, 0)],
        all_layers: false,
    }));
    let effects = editor.apply(Intent::Stroke(Stroke::End));
    assert_eq!(cell(&editor, doc, 1, IVec2::ZERO), tile(1));
    assert!(
        matches!(&effects[..], [Effect::Notify(n)] if n.text == "“ground” is locked: its tiles were left as they were"),
        "{effects:?}"
    );
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: false,
    });
    editor.apply(Intent::Run(ActionId::BrushSizeUp));
    editor.apply(Intent::Run(ActionId::BrushSizeUp));
    editor.apply(Intent::Stroke(Stroke::Begin {
        doc,
        kind: StrokeKind::Erase,
    }));
    editor.apply(Intent::Stroke(Stroke::Erase {
        points: vec![IVec2::new(2, 2)],
        all_layers: false,
    }));
    editor.apply(Intent::Stroke(Stroke::End));
    // A size of 3 erases the square around the point.
    for at in IRect::from_min_size(IVec2::new(1, 1), IVec2::splat(3)).points() {
        assert!(cell(&editor, doc, 1, at).is_empty(), "{at}");
    }
    assert_eq!(cell(&editor, doc, 1, IVec2::new(4, 1)), tile(1));
}

#[test]
fn fills_paint_what_they_preview_and_shape_fills_keep_to_the_map() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::SelectLayer {
        doc,
        layer: LayerId(2),
    });
    pick(&mut editor, doc, 7);
    let preview = editor.fill_preview(doc, IVec2::new(0, 0), &[], false);
    assert_eq!(preview.area.len(), 24, "the empty layer floods everywhere");
    editor.apply(Intent::Fill {
        doc,
        start: IVec2::ZERO,
        extra: Vec::new(),
        whole_selection: false,
    });
    for placed in preview.cells {
        assert_eq!(cell(&editor, doc, 2, placed.at), placed.cell);
    }
    pick(&mut editor, doc, 9);
    editor.apply(Intent::ShapeFill {
        doc,
        region: TileRegion::from_rect(IRect::from_min_size(IVec2::new(4, 2), IVec2::splat(5))),
    });
    assert_eq!(cell(&editor, doc, 2, IVec2::new(5, 3)), tile(9));
    assert_eq!(cell(&editor, doc, 2, IVec2::new(3, 3)), tile(7));
}

#[test]
fn the_picker_and_capture_make_brushes() {
    let (mut editor, doc) = editor();
    editor.apply(Intent::Run(ActionId::EraserTool));
    editor.apply(Intent::PickFromMap {
        doc,
        at: IVec2::new(3, 3),
    });
    assert_eq!(editor.tools().tool(), ToolKind::Brush);
    assert_eq!(
        editor.brush_preview(doc, &[IVec2::ZERO], false)[0].cell,
        tile(1)
    );
    editor.apply(Intent::Capture {
        doc,
        rect: IRect::from_min_size(IVec2::ZERO, IVec2::new(2, 2)),
        cut: true,
    });
    assert!(cell(&editor, doc, 1, IVec2::ZERO).is_empty(), "cut");
    assert_eq!(
        editor.brush_preview(doc, &[IVec2::new(3, 3)], false).len(),
        4,
        "a 2×2 brush"
    );
}

#[test]
fn copy_paste_in_place_and_delete_follow_the_selection_and_locks() {
    let (mut editor, doc) = editor();
    assert_eq!(
        editor.enabled(ActionId::Paste),
        Enabled::Disabled(Some("Nothing was copied".to_owned()))
    );
    editor.apply(Intent::SelectTiles {
        doc,
        region: TileRegion::from_rect(IRect::from_min_size(IVec2::new(1, 1), IVec2::new(2, 1))),
        mode: bt_editor::SelectionMode::Replace,
    });
    editor.apply(Intent::Run(ActionId::Copy));
    editor.apply(Intent::Run(ActionId::Delete));
    assert!(cell(&editor, doc, 1, IVec2::new(1, 1)).is_empty());
    editor.apply(Intent::Run(ActionId::PasteInPlace));
    assert_eq!(cell(&editor, doc, 1, IVec2::new(1, 1)), tile(1));
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: LayerId(1),
        locked: true,
    });
    assert_eq!(
        editor.enabled(ActionId::Delete),
        Enabled::Disabled(Some(
            "“ground” is locked: unlock it to delete its tiles".to_owned()
        ))
    );
    editor.apply(Intent::Run(ActionId::Paste));
    assert_eq!(
        editor.tools().tool(),
        ToolKind::Brush,
        "Paste makes the brush"
    );
}

#[test]
fn layer_commands_select_what_they_make_or_leave() {
    let (mut editor, doc) = editor();
    let layers = |editor: &Editor| {
        let mut names = Vec::new();
        editor
            .map_of(doc)
            .unwrap()
            .for_each_layer(|layer, _| names.push(layer.name.clone()));
        names
    };
    let current = |editor: &Editor| editor.document(doc).unwrap().selection().current_layer;
    // Named as Tiled names them: by how many tile layers the map then has.
    editor.apply(Intent::Run(ActionId::NewTileLayer));
    assert_eq!(layers(&editor), ["ground", "Tile Layer 3", "top", "things"]);
    let made = current(&editor).unwrap();
    assert_eq!(
        editor.map_of(doc).unwrap().layer(made).unwrap().name,
        "Tile Layer 3"
    );
    editor.apply(Intent::Run(ActionId::RemoveLayers));
    assert_eq!(layers(&editor), ["ground", "top", "things"]);
    assert_eq!(current(&editor), Some(LayerId(1)), "the layer below");
    editor.apply(Intent::Run(ActionId::DuplicateLayers));
    assert_eq!(layers(&editor), ["ground", "ground Copy", "top", "things"]);
    editor.apply(Intent::Run(ActionId::MergeLayerDown));
    assert_eq!(layers(&editor), ["ground", "top", "things"]);
    editor.apply(Intent::Run(ActionId::SelectNextLayer));
    assert_eq!(current(&editor), Some(LayerId(2)));
    editor.apply(Intent::Run(ActionId::GroupLayers));
    assert_eq!(layers(&editor), ["ground", "Group 1", "top", "things"]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(layers(&editor), ["ground", "top", "things"]);
}
