//! The tileset editor's Terrain mode through the editor: a new terrain set is Tiled's, a
//! labelling stroke is one step on the tileset, clearing a label's last lane removes it,
//! removing or moving a terrain relabels the tiles, and the terrain labelling paints follows the
//! sets' edits.

#![cfg(test)]

use bt_core::{Color, Lane, TileId, TilesetSlot, WangId, WangSetType};
use bt_editor::{
    ActionId, DocumentId, Editor, Intent, LabelMark, LabelStroke, LabelTerrain, TerrainSetEdit,
    TilesetMode,
};
use bt_terrain::TerrainColor;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use glam::UVec2;

/// A map whose embedded tileset is open in its Terrain mode.
fn editor() -> (Editor, DocumentId) {
    let atlas = Atlas::builder().grid(4, 4).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
        .tileset(atlas.tileset)
        .tile_layer("ground", "")
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    let slot = TilesetSlot::new(0).unwrap();
    editor.apply(Intent::EditTileset { doc, slot });
    editor.apply(Intent::SetTilesetMode {
        doc,
        mode: TilesetMode::Terrain,
    });
    (editor, doc)
}

fn edit(editor: &mut Editor, doc: DocumentId, edit: TerrainSetEdit) {
    let effects = editor.apply(Intent::EditTerrainSets { doc, edit });
    assert!(effects.is_empty(), "{effects:?}");
}

fn tileset(editor: &Editor, doc: DocumentId) -> &bt_core::Tileset {
    editor.edited_tileset(doc).unwrap()
}

fn label(editor: &Editor, doc: DocumentId) -> Option<LabelTerrain> {
    editor
        .document(doc)
        .unwrap()
        .tileset_editing()
        .unwrap()
        .label_terrain()
}

fn mark(tile: u32, lane: Lane) -> LabelMark {
    LabelMark {
        tile: TileId(tile),
        lane,
        erase: false,
    }
}

fn stroke(editor: &mut Editor, doc: DocumentId, marks: Vec<LabelMark>) {
    for step in [
        LabelStroke::Begin { doc },
        LabelStroke::Paint { marks },
        LabelStroke::End,
    ] {
        let effects = editor.apply(Intent::LabelStroke(step));
        assert!(effects.is_empty(), "{effects:?}");
    }
}

fn labels(editor: &Editor, doc: DocumentId) -> Vec<String> {
    editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .filter(|e| e.done)
        .map(|e| e.label)
        .collect()
}

#[test]
fn a_new_terrain_set_is_tileds_and_is_labelled_with() {
    let (mut editor, doc) = editor();
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::AddSet {
            kind: WangSetType::Corner,
        },
    );
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(
        (set.name.as_str(), set.kind),
        ("Unnamed Set", WangSetType::Corner)
    );
    assert_eq!(set.colors.len(), 1);
    assert_eq!(
        set.colors[0].color,
        Color::rgb(255, 0, 0),
        "Tiled's first colour"
    );
    assert_eq!(
        label(&editor, doc),
        Some(LabelTerrain {
            set: 0,
            color: TerrainColor(1)
        })
    );
    edit(&mut editor, doc, TerrainSetEdit::AddTerrain { set: 0 });
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(set.colors[1].color, Color::rgb(0, 255, 0), "and its second");
    assert_eq!(labels(&editor, doc), ["Add Terrain Set", "Add Terrain"]);
    editor.apply(Intent::Run(ActionId::Undo));
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(tileset(&editor, doc).wang_sets.is_empty());
}

#[test]
fn a_labelling_stroke_is_one_step_and_an_emptied_label_goes() {
    let (mut editor, doc) = editor();
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::AddSet {
            kind: WangSetType::Corner,
        },
    );
    stroke(
        &mut editor,
        doc,
        vec![
            mark(5, Lane::TopLeft),
            mark(5, Lane::TopRight),
            mark(6, Lane::BottomLeft),
        ],
    );
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(
        set.tiles[&TileId(5)],
        WangId::from_lanes([0, 1, 0, 0, 0, 0, 0, 1])
    );
    assert_eq!(
        set.tiles[&TileId(6)],
        WangId::from_lanes([0, 0, 0, 0, 0, 1, 0, 0])
    );
    assert_eq!(labels(&editor, doc), ["Add Terrain Set", "Label Terrain"]);

    let erase = LabelMark {
        erase: true,
        ..mark(6, Lane::BottomLeft)
    };
    stroke(&mut editor, doc, vec![erase]);
    assert!(
        !tileset(&editor, doc).wang_sets[0]
            .tiles
            .contains_key(&TileId(6)),
        "a label of no terrain at all is removed, as Tiled does"
    );
    editor.apply(Intent::Run(ActionId::Undo));
    assert!(
        tileset(&editor, doc).wang_sets[0]
            .tiles
            .contains_key(&TileId(6))
    );
}

#[test]
fn removing_or_moving_a_terrain_relabels_the_tiles() {
    let (mut editor, doc) = editor();
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::AddSet {
            kind: WangSetType::Corner,
        },
    );
    for _ in 0..2 {
        edit(&mut editor, doc, TerrainSetEdit::AddTerrain { set: 0 });
    }
    // Tile 0: 1, 2, 3, 3 on its corners; tile 1: 2 at the top-right only.
    for (tile, lane, color) in [
        (0, Lane::TopRight, 1),
        (0, Lane::BottomRight, 2),
        (0, Lane::BottomLeft, 3),
        (0, Lane::TopLeft, 3),
        (1, Lane::TopRight, 2),
    ] {
        editor.apply(Intent::ChooseLabelTerrain {
            doc,
            label: LabelTerrain {
                set: 0,
                color: TerrainColor(color),
            },
        });
        stroke(&mut editor, doc, vec![mark(tile, lane)]);
    }
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::MoveTerrain {
            set: 0,
            from: TerrainColor(3),
            to: TerrainColor(1),
        },
    );
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(set.tiles[&TileId(0)].lanes(), [0, 2, 0, 3, 0, 1, 0, 1]);
    assert_eq!(set.colors[0].name, "Terrain 3", "the list moved with it");
    // Terrain 2, the one labelled with last, moved down a place.
    assert_eq!(label(&editor, doc).unwrap().color, TerrainColor(3));

    edit(
        &mut editor,
        doc,
        TerrainSetEdit::RemoveTerrain {
            set: 0,
            color: TerrainColor(3),
        },
    );
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(set.colors.len(), 2);
    assert_eq!(set.tiles[&TileId(0)].lanes(), [0, 2, 0, 0, 0, 1, 0, 1]);
    assert!(
        !set.tiles.contains_key(&TileId(1)),
        "its only lane was the removed terrain"
    );
}

#[test]
fn sets_and_terrains_are_renamed_recoloured_weighted_and_removed() {
    let (mut editor, doc) = editor();
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::AddSet {
            kind: WangSetType::Edge,
        },
    );
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::AddSet {
            kind: WangSetType::Mixed,
        },
    );
    let first = |editor: &Editor| tileset(editor, doc).wang_sets[0].clone();
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::RenameSet {
            set: 0,
            name: "Paths".to_owned(),
        },
    );
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::RenameTerrain {
            set: 0,
            color: TerrainColor(1),
            name: "Gravel".to_owned(),
        },
    );
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::RecolorTerrain {
            set: 0,
            color: TerrainColor(1),
            rgb: Color::rgb(1, 2, 3),
        },
    );
    edit(
        &mut editor,
        doc,
        TerrainSetEdit::SetProbability {
            set: 0,
            color: TerrainColor(1),
            probability: 0.25,
        },
    );
    let paths = first(&editor);
    assert_eq!(paths.name, "Paths");
    assert_eq!(
        (
            paths.colors[0].name.as_str(),
            paths.colors[0].color,
            paths.colors[0].probability
        ),
        ("Gravel", Color::rgb(1, 2, 3), 0.25)
    );
    let refused = editor.apply(Intent::EditTerrainSets {
        doc,
        edit: TerrainSetEdit::SetProbability {
            set: 0,
            color: TerrainColor(1),
            probability: -1.0,
        },
    });
    assert_eq!(refused.len(), 1, "a negative weight is refused");

    // The labelling was on the mixed set; removing the first keeps it there.
    assert_eq!(label(&editor, doc).unwrap().set, 1);
    edit(&mut editor, doc, TerrainSetEdit::DuplicateSet { set: 0 });
    assert_eq!(tileset(&editor, doc).wang_sets[1].name, "Copy of Paths");
    edit(&mut editor, doc, TerrainSetEdit::RemoveSet { set: 0 });
    edit(&mut editor, doc, TerrainSetEdit::RemoveSet { set: 0 });
    assert_eq!(tileset(&editor, doc).wang_sets.len(), 1);
    assert_eq!(tileset(&editor, doc).wang_sets[0].kind, WangSetType::Mixed);
    assert_eq!(label(&editor, doc).unwrap().set, 0);
}
