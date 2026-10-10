//! Terrain through the editor: choosing a terrain makes the Terrain tool current, a stroke paints
//! what its preview showed as one step, the tool says why it cannot paint, right-click takes the
//! terrain under the pointer, the fill tools fill with terrain until a brush is picked, and a
//! stroke that leaves cells unfilled says why.

#![cfg(test)]

use std::sync::Arc;

use bt_core::{
    Cell, Color, LayerId, Map, PlacedCell, TileId, Tileset, TilesetSlot, WangColor, WangId,
    WangSet, WangSetType,
};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Intent, NoTerrain, TerrainStroke, TilesetHome, ToolKind,
};
use bt_terrain::{BrushTarget, Terrain, TerrainColor};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use glam::{IVec2, UVec2};

const GROUND: LayerId = LayerId(1);
const THINGS: LayerId = LayerId(2);
const SAND: TerrainColor = TerrainColor(1);
const GRASS: TerrainColor = TerrainColor(2);

fn slot() -> TilesetSlot {
    TilesetSlot::new(0).unwrap()
}

fn color(name: &str) -> WangColor {
    WangColor {
        name: name.to_owned(),
        class: String::new(),
        color: Color::rgb(0, 0, 0),
        tile: None,
        probability: 1.0,
        properties: Default::default(),
    }
}

/// A tileset whose "Ground" corner set has sand and grass; with `transitions`, every
/// combination of the two (tile 0 all sand, tile 15 all grass), else just the two plain tiles.
fn sand_and_grass(transitions: bool) -> Tileset {
    let mut tileset = (*Atlas::builder()
        .grid(16, 16)
        .build_embedded()
        .unwrap()
        .tileset)
        .clone();
    let mut set = WangSet {
        name: "Ground".to_owned(),
        class: String::new(),
        kind: WangSetType::Corner,
        tile: None,
        colors: vec![color("Sand"), color("Grass")],
        tiles: Default::default(),
        properties: Default::default(),
    };
    let mut tile = 0;
    for a in 1..=2 {
        for b in 1..=2 {
            for c in 1..=2 {
                for d in 1..=2 {
                    let plain = a == b && b == c && c == d;
                    if transitions || plain {
                        set.tiles
                            .insert(TileId(tile), WangId::from_lanes([0, a, 0, b, 0, c, 0, d]));
                    }
                    tile += 1;
                }
            }
        }
    }
    tileset.wang_sets.push(set);
    tileset
}

/// A 6×6 map of all-sand ground under an object layer, open with the ground current.
fn editor_with(transitions: bool) -> (Editor, DocumentId) {
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(6))
        .tileset(Arc::new(sand_and_grass(transitions)))
        .tile_layer("ground", &["0 0 0 0 0 0"; 6].join("\n"))
        .object_layer("things", Vec::new())
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    editor.apply(Intent::SelectLayer { doc, layer: GROUND });
    (editor, doc)
}

fn choose(editor: &mut Editor, doc: DocumentId, color: TerrainColor) -> Vec<Effect> {
    editor.apply(Intent::ChooseTerrain {
        doc,
        slot: slot(),
        set: 0,
        color,
    })
}

fn map(editor: &Editor, doc: DocumentId) -> &Map {
    editor.map_of(doc).unwrap()
}

fn lanes_at(editor: &Editor, doc: DocumentId, p: IVec2) -> [u8; 8] {
    let map = map(editor, doc);
    let terrain = Terrain::new(map, slot(), 0).unwrap();
    let cell = map.layer(GROUND).unwrap().as_tile_layer().unwrap().cell(p);
    terrain.wang_of_cell(cell).lanes()
}

fn stroke(editor: &mut Editor, doc: DocumentId, targets: Vec<BrushTarget>) -> Vec<Effect> {
    let mut effects = editor.apply(Intent::TerrainStroke(TerrainStroke::Begin { doc }));
    effects.extend(editor.apply(Intent::TerrainStroke(TerrainStroke::Paint {
        targets,
        mirror: false,
    })));
    effects.extend(editor.apply(Intent::TerrainStroke(TerrainStroke::End)));
    effects
}

fn notices(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Notify(notice) => Some(notice.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn choosing_a_terrain_makes_the_terrain_tool_current() {
    let (mut editor, doc) = editor_with(true);
    assert!(choose(&mut editor, doc, GRASS).is_empty());
    assert_eq!(editor.tools().tool(), ToolKind::Terrain);
    let choice = editor.tools().terrain().unwrap();
    let asset = editor.document(doc).unwrap().asset();
    assert_eq!(
        choice.home,
        TilesetHome::Embedded {
            map: asset,
            slot: slot()
        }
    );
    assert_eq!((choice.set, choice.color), (0, GRASS));
    let in_use = editor.terrain_in_use(doc).unwrap();
    assert_eq!(
        (in_use.layer, in_use.name(GRASS).as_str()),
        (GROUND, "Grass")
    );
}

#[test]
fn a_stroke_paints_what_its_preview_showed_as_one_step() {
    let (mut editor, doc) = editor_with(true);
    choose(&mut editor, doc, GRASS);
    let targets = vec![BrushTarget::Corner(IVec2::splat(3))];
    let preview = editor.terrain_preview(doc, &targets, false).unwrap();
    assert!(preview.invalid.is_empty());
    let effects = stroke(&mut editor, doc, targets);
    assert!(notices(&effects).is_empty(), "{effects:?}");
    let layer = map(&editor, doc)
        .layer(GROUND)
        .unwrap()
        .as_tile_layer()
        .unwrap();
    for placed in &preview.cells {
        assert_eq!(layer.cell(placed.at), placed.cell);
    }
    assert_eq!(
        lanes_at(&editor, doc, IVec2::splat(3)),
        [0, 1, 0, 1, 0, 1, 0, 2]
    );
    let labels: Vec<String> = editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .filter(|e| e.done)
        .map(|e| e.label)
        .collect();
    assert_eq!(labels, ["Paint Terrain"]);
    editor.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        lanes_at(&editor, doc, IVec2::splat(3)),
        [0, 1, 0, 1, 0, 1, 0, 1]
    );
}

#[test]
fn the_terrain_tool_says_why_it_cannot_paint() {
    let (mut editor, doc) = editor_with(true);
    assert_eq!(
        editor.terrain_in_use(doc).unwrap_err(),
        NoTerrain::NotChosen
    );
    choose(&mut editor, doc, GRASS);
    editor.apply(Intent::SelectLayer { doc, layer: THINGS });
    assert_eq!(
        editor.terrain_in_use(doc).unwrap_err(),
        NoTerrain::NotTileLayer("things".to_owned())
    );
    editor.apply(Intent::SelectLayer { doc, layer: GROUND });
    editor.apply(Intent::SetLayerLocked {
        doc,
        layer: GROUND,
        locked: true,
    });
    let reason = NoTerrain::Locked("ground".to_owned());
    assert_eq!(editor.terrain_in_use(doc).unwrap_err(), reason);
    assert!(
        editor
            .terrain_preview(doc, &[BrushTarget::Tile(IVec2::ONE)], false)
            .is_none()
    );
    let effects = editor.apply(Intent::TerrainStroke(TerrainStroke::Begin { doc }));
    assert_eq!(notices(&effects), [reason.to_string()]);
}

#[test]
fn right_click_takes_the_terrain_under_the_pointer() {
    let (mut editor, doc) = editor_with(true);
    choose(&mut editor, doc, GRASS);
    stroke(&mut editor, doc, vec![BrushTarget::Corner(IVec2::splat(3))]);
    choose(&mut editor, doc, SAND);
    editor.apply(Intent::CaptureTerrain {
        doc,
        target: BrushTarget::Corner(IVec2::splat(3)),
    });
    assert_eq!(editor.tools().terrain().unwrap().color, GRASS);
    editor.apply(Intent::CaptureTerrain {
        doc,
        target: BrushTarget::Corner(IVec2::ZERO),
    });
    assert_eq!(editor.tools().terrain().unwrap().color, SAND);
}

#[test]
fn the_fill_tools_fill_with_terrain_until_a_brush_is_picked() {
    let (mut editor, doc) = editor_with(true);
    choose(&mut editor, doc, GRASS);
    editor.apply(Intent::SetTerrainFill(true));
    let preview = editor.fill_preview(doc, IVec2::splat(2), &[], false);
    assert_eq!(preview.area.len(), 36, "the whole sand layer");
    assert!(preview.invalid.is_empty());
    editor.apply(Intent::Fill {
        doc,
        start: IVec2::splat(2),
        extra: Vec::new(),
        whole_selection: false,
    });
    assert_eq!(
        lanes_at(&editor, doc, IVec2::splat(2)),
        [0, 2, 0, 2, 0, 2, 0, 2]
    );
    editor.apply(Intent::PickTiles {
        doc,
        cells: vec![PlacedCell {
            at: IVec2::ZERO,
            cell: Cell::new(slot(), TileId(0)),
        }],
    });
    assert!(!editor.tools().terrain_fill(), "a brush ends terrain fill");
}

#[test]
fn a_stroke_that_leaves_cells_unfilled_says_why() {
    let (mut editor, doc) = editor_with(false);
    choose(&mut editor, doc, GRASS);
    let effects = stroke(&mut editor, doc, vec![BrushTarget::Corner(IVec2::splat(3))]);
    assert_eq!(
        notices(&effects),
        ["No tile fits 4 cells: “Ground” has no tile joining Sand and Grass"]
    );
}
