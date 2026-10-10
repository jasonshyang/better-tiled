//! The tileset editor's terrain helpers through the editor, on tilesets drawn lane by lane:
//! Layout… finds the blocks `bt-terrain` finds and labels them as its layouts and Label All
//! do, each Apply and each Label All one step that undoes, a new set made in the same step.

#![cfg(test)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use bt_core::{Color, TileId, Tileset, TilesetSlot, WangId, WangSet, WangSetType};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Intent, LabelTerrain, LayoutSearch, LayoutTarget,
    Severity, TilesetMode,
};
use bt_terrain::TerrainColor;
use bt_terrain::layouts::{Block, Fit, KnownTerrains, LAYOUTS, Layout, Samples, detect};
use bt_test_data::fixtures::{Atlas, MapBuilder, blank_picture, paint_lanes};
use glam::UVec2;

use common::apply;

/// The tile size of the drawn tilesets.
const TILE: u32 = 16;
const GRASS: [u8; 4] = [70, 170, 60, 255];
const SAND: [u8; 4] = [220, 200, 120, 255];
const DIRT: [u8; 4] = [120, 80, 40, 255];

fn layout(name: &str) -> (usize, &'static Layout) {
    LAYOUTS
        .iter()
        .enumerate()
        .find(|(_, l)| l.name == name)
        .unwrap()
}

/// A tileset `columns`×`rows` tiles, all `under`, with `layout` drawn at each origin in its
/// terrain's colour.
fn drawn_tileset(
    columns: u32,
    rows: u32,
    under: [u8; 4],
    blocks: &[(&str, UVec2, [u8; 4])],
) -> Arc<Tileset> {
    let mut picture = blank_picture(columns * TILE, rows * TILE);
    for row in 0..rows {
        for column in 0..columns {
            let at = UVec2::new(column, row) * TILE;
            paint_lanes(&mut picture, at, TILE, WangSetType::Mixed, [Some(under); 9]);
        }
    }
    for &(name, origin, terrain) in blocks {
        let (_, layout) = layout(name);
        let width = layout.size().x;
        for (i, slot) in layout.slots().into_iter().enumerate() {
            let Some(slot) = slot else { continue };
            let i = u32::try_from(i).unwrap();
            let tile = origin + UVec2::new(i % width, i / width);
            let mut colors = [Some(under); 9];
            for (lane, color) in colors.iter_mut().enumerate().take(8) {
                if slot.terrain & (1 << lane) != 0 {
                    *color = Some(terrain);
                }
            }
            if slot.middle {
                colors[8] = Some(terrain);
            }
            paint_lanes(&mut picture, tile * TILE, TILE, layout.kind, colors);
        }
    }
    Atlas::builder()
        .name("drawn")
        .tile_size(TILE, TILE)
        .grid(columns, rows)
        .picture(picture)
        .build_embedded()
        .unwrap()
        .tileset
}

/// A map whose embedded `tileset` is open in its Terrain mode.
fn editor_with(tileset: Arc<Tileset>) -> (Editor, DocumentId) {
    let map = MapBuilder::new(UVec2::splat(TILE), UVec2::splat(4))
        .tileset(tileset)
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

fn tileset(editor: &Editor, doc: DocumentId) -> &Tileset {
    editor.edited_tileset(doc).unwrap()
}

fn steps(editor: &Editor, doc: DocumentId) -> Vec<String> {
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

/// Looks for layouts, running the read as a worker would; the blocks found.
fn find_layouts(editor: &mut Editor, doc: DocumentId) -> Vec<Fit> {
    let effects = apply(editor, Intent::FindLayouts { doc });
    assert!(effects.is_empty(), "{effects:?}");
    match editor.layout_search(doc) {
        Some(LayoutSearch::Found(found)) => found.fits().to_vec(),
        other => panic!("no layouts found: {other:?}"),
    }
}

/// What `bt-terrain` finds in the tileset's image, and what Label All gives the blocks of `kind`
/// in a set with no terrains yet.
fn terrain_crate_finds(tileset: &Tileset, kind: WangSetType) -> (Vec<Fit>, Vec<Block>) {
    let bt_core::TilesetImage::Atlas { image, .. } = &tileset.image else {
        panic!("an atlas");
    };
    let bt_core::ImageSource::Embedded(embedded) = &image.source else {
        panic!("an embedded image");
    };
    let decoded = bt_image::decode_bytes(&embedded.bytes, &embedded.format, None).unwrap();
    let samples = Samples::from_tileset(tileset, &decoded);
    let fits = detect(&samples);
    let of_kind: Vec<Fit> = fits
        .iter()
        .filter(|f| LAYOUTS[f.layout].kind == kind)
        .cloned()
        .collect();
    let empty = WangSet {
        name: String::new(),
        class: String::new(),
        kind,
        tile: None,
        colors: Vec::new(),
        tiles: BTreeMap::new(),
        properties: Default::default(),
    };
    let blocks = KnownTerrains::from_set(&samples, &empty, &[]).label_all(&samples, &of_kind);
    (fits, blocks)
}

/// The labels a set has after `blocks` are labelled in it from scratch.
fn labels_of(blocks: &[Block], columns: u32) -> BTreeMap<TileId, WangId> {
    let mut labels = BTreeMap::new();
    for block in blocks {
        for (tile, label) in block.labels(columns) {
            match label {
                Some(label) => labels.insert(tile, label),
                None => labels.remove(&tile),
            };
        }
    }
    labels
}

#[test]
fn apply_labels_one_block_in_a_new_set_as_one_step() {
    let (island, _) = layout("3×3 island");
    let drawn = drawn_tileset(3, 3, DIRT, &[("3×3 island", UVec2::ZERO, GRASS)]);
    let (mut editor, doc) = editor_with(drawn.clone());
    let fits = find_layouts(&mut editor, doc);
    let (expected, _) = terrain_crate_finds(&drawn, WangSetType::Corner);
    assert_eq!(fits, expected, "what bt-terrain finds");
    assert_eq!(
        fits,
        [Fit {
            layout: island,
            origin: UVec2::ZERO,
            over_nothing: false
        }]
    );
    let into = LayoutTarget::NewSet(WangSetType::Corner);
    let guess = editor.layout_guess(doc, into, &fits[0]).unwrap();
    assert_eq!(
        (guess.terrain, guess.under),
        (TerrainColor(1), TerrainColor(2)),
        "both new"
    );

    let effects = apply(
        &mut editor,
        Intent::ApplyLayout {
            doc,
            into,
            block: guess,
        },
    );
    assert!(effects.is_empty(), "{effects:?}");
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(
        (set.name.as_str(), set.kind),
        ("Unnamed Set", WangSetType::Corner)
    );
    let names: Vec<&str> = set.colors.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["Terrain 1", "Terrain 2"],
        "named as new terrains are"
    );
    assert_eq!(
        set.colors[1].color,
        Color::rgb(0, 255, 0),
        "and coloured: Tiled's second"
    );
    assert_eq!(set.tiles, labels_of(&[guess], 3), "the layout's labels");
    assert_eq!(
        set.colors[0].tile,
        Some(TileId(4)),
        "the middle is all grass"
    );
    assert_eq!((set.colors[1].tile, set.tile), (None, Some(TileId(4))));
    assert_eq!(
        editor
            .document(doc)
            .unwrap()
            .tileset_editing()
            .unwrap()
            .label_terrain(),
        Some(LabelTerrain {
            set: 0,
            color: TerrainColor(1)
        }),
        "the new set is labelled with"
    );
    assert_eq!(steps(&editor, doc), ["Apply Layout"]);

    editor.apply(Intent::Run(ActionId::Undo));
    assert!(tileset(&editor, doc).wang_sets.is_empty(), "undone at once");
}

#[test]
fn label_all_labels_every_block_found_as_one_step() {
    let drawn = drawn_tileset(
        7,
        3,
        DIRT,
        &[
            ("3×3 island", UVec2::ZERO, GRASS),
            ("3×3 island", UVec2::new(4, 0), SAND),
        ],
    );
    let (mut editor, doc) = editor_with(drawn.clone());
    let fits = find_layouts(&mut editor, doc);
    let (expected_fits, expected_blocks) = terrain_crate_finds(&drawn, WangSetType::Corner);
    assert_eq!(fits, expected_fits);
    assert_eq!(fits.len(), 2, "both islands: {fits:?}");
    let into = LayoutTarget::NewSet(WangSetType::Corner);
    assert_eq!(editor.label_all_blocks(doc, into), expected_blocks);
    let terrains: Vec<[u8; 2]> = expected_blocks
        .iter()
        .map(|b| [b.terrain.0, b.under.0])
        .collect();
    assert_eq!(terrains, [[1, 2], [3, 2]], "the dirt is one ground");

    let effects = apply(&mut editor, Intent::LabelAllLayouts { doc, into });
    let [Effect::Notify(notice)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(notice.text, "Labelled 2 blocks, adding 3 terrains");
    let set = &tileset(&editor, doc).wang_sets[0];
    assert_eq!(set.colors.len(), 3);
    assert_eq!(set.tiles, labels_of(&expected_blocks, 7));
    assert_eq!(steps(&editor, doc), ["Label All Layouts"]);

    // Again into the set it made: the terrains are known now, so none is added and nothing
    // changes.
    let again = apply(
        &mut editor,
        Intent::LabelAllLayouts {
            doc,
            into: LayoutTarget::Set(0),
        },
    );
    let [Effect::Notify(notice)] = again.as_slice() else {
        panic!("{again:?}");
    };
    assert_eq!(notice.text, "Labelled 2 blocks");
    assert_eq!(
        steps(&editor, doc),
        ["Label All Layouts"],
        "no change, no step"
    );

    editor.apply(Intent::Run(ActionId::Undo));
    assert!(tileset(&editor, doc).wang_sets.is_empty());
}

#[test]
fn blocks_that_cannot_be_labelled_are_refused_with_the_reason() {
    let (island, _) = layout("3×3 island");
    let (blob, _) = layout("Blob 7×7");
    let drawn = drawn_tileset(3, 3, DIRT, &[("3×3 island", UVec2::ZERO, GRASS)]);
    let (mut editor, doc) = editor_with(drawn);
    let block = |layout: usize, x: u32, terrain: u8, under: u8| Block {
        layout,
        origin: UVec2::new(x, 0),
        terrain: TerrainColor(terrain),
        under: TerrainColor(under),
    };
    let refusal = |editor: &mut Editor, into: LayoutTarget, block: Block| {
        let effects = editor.apply(Intent::ApplyLayout { doc, into, block });
        match effects.as_slice() {
            [Effect::Notify(notice)] if notice.severity == Severity::Warning => notice.text.clone(),
            other => panic!("{other:?}"),
        }
    };
    let corner = LayoutTarget::NewSet(WangSetType::Corner);
    assert_eq!(
        refusal(&mut editor, corner, block(blob, 0, 1, 0)),
        "“Blob 7×7” labels a mixed set, not a corner set: choose a mixed set or a new one"
    );
    assert_eq!(
        refusal(&mut editor, corner, block(island, 1, 1, 2)),
        "“3×3 island” is 3×3 tiles: from column 1, row 0 it goes past the tileset's 3×3"
    );
    assert_eq!(
        refusal(&mut editor, corner, block(island, 0, 1, 1)),
        "A block's terrain and what it is drawn over must be different terrains"
    );
    assert_eq!(
        refusal(&mut editor, LayoutTarget::Set(0), block(island, 0, 1, 2)),
        "the tileset has no terrain set 0"
    );
    assert!(tileset(&editor, doc).wang_sets.is_empty());
    assert!(steps(&editor, doc).is_empty());

    // Label All before anything was looked for says what to do first.
    let effects = editor.apply(Intent::LabelAllLayouts { doc, into: corner });
    let [Effect::Notify(notice)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(
        notice.text.starts_with("Look for layouts"),
        "{}",
        notice.text
    );
}

#[test]
fn a_search_belongs_to_the_tileset_it_was_made_for() {
    let drawn = drawn_tileset(3, 3, DIRT, &[("3×3 island", UVec2::ZERO, GRASS)]);
    let other = Atlas::builder().name("other").build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::splat(TILE), UVec2::splat(4))
        .tileset(drawn)
        .tileset(other.tileset)
        .tile_layer("ground", "")
        .build()
        .unwrap();
    let mut editor = Editor::new();
    let doc = editor.open_new_map(map);
    let slot = |index: usize| TilesetSlot::new(index).unwrap();
    editor.apply(Intent::EditTileset { doc, slot: slot(0) });
    let effects = editor.apply(Intent::FindLayouts { doc });
    let [Effect::ReadTilesetImage(job)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(job.doc(), doc);
    assert!(matches!(
        editor.layout_search(doc),
        Some(LayoutSearch::Searching)
    ));
    let read = job.run();
    // Another tileset is edited before the read comes back: the search is not its.
    editor.apply(Intent::EditTileset { doc, slot: slot(1) });
    assert!(editor.layout_search(doc).is_none());
    editor.apply(Intent::TilesetImageRead(Box::new(read)));
    assert!(editor.layout_search(doc).is_none());
    editor.apply(Intent::EditTileset { doc, slot: slot(0) });
    assert!(
        matches!(editor.layout_search(doc), Some(LayoutSearch::Found(found)) if found.fits().len() == 1),
        "the first tileset's"
    );

    // A collection of images has no grid.
    let collection = Arc::new(Tileset {
        name: "props".into(),
        tile_width: 16,
        tile_height: 16,
        ..Tileset::default()
    });
    let (mut editor, doc) = editor_with(collection);
    let effects = editor.apply(Intent::FindLayouts { doc });
    let [Effect::Notify(notice)] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        notice.text,
        "A collection of images has no grid: no autotile layout fits it"
    );
}
