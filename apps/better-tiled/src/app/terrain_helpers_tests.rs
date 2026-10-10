//! The tileset editor's terrain helpers in the window, on tilesets drawn lane by lane: Layout…
//! finds a block in the image, Apply labels it and Label All labels every block found;
//! Auto-label… labels the chosen terrain from the tile clicked; each one step.

use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{TileId, Tileset, WangSetType};
use bt_editor::DocumentId;
use bt_terrain::TerrainColor;
use bt_terrain::layouts::LAYOUTS;
use bt_test_data::Scratch;
use bt_test_data::fixtures::{
    Atlas, MapBuilder, TerrainSetBuilder, blank_picture, corners, paint_lanes,
};
use bt_tmx::WriteRequest;
use egui::{Modifiers, PointerButton};
use glam::UVec2;

use crate::test_window::TestWindow;

/// The tile size of the drawn tilesets.
const TILE: u32 = 16;
const GRASS: [u8; 4] = [70, 170, 60, 255];
const DIRT: [u8; 4] = [120, 80, 40, 255];

/// A tileset `columns`×`rows` tiles of dirt with layout `name` drawn in grass from `origin`,
/// its image written into `scratch`.
fn drawn_tileset(
    scratch: &Scratch,
    columns: u32,
    rows: u32,
    name: &str,
    origin: UVec2,
) -> Arc<Tileset> {
    let mut picture = blank_picture(columns * TILE, rows * TILE);
    for row in 0..rows {
        for column in 0..columns {
            let at = UVec2::new(column, row) * TILE;
            paint_lanes(&mut picture, at, TILE, WangSetType::Mixed, [Some(DIRT); 9]);
        }
    }
    let layout = LAYOUTS.iter().find(|l| l.name == name).unwrap();
    let width = layout.size().x;
    for (i, slot) in layout.slots().into_iter().enumerate() {
        let Some(slot) = slot else { continue };
        let i = u32::try_from(i).unwrap();
        let tile = origin + UVec2::new(i % width, i / width);
        let mut colors = [Some(DIRT); 9];
        for (lane, color) in colors.iter_mut().enumerate().take(8) {
            if slot.terrain & (1 << lane) != 0 {
                *color = Some(GRASS);
            }
        }
        if slot.middle {
            colors[8] = Some(GRASS);
        }
        paint_lanes(&mut picture, tile * TILE, TILE, layout.kind, colors);
    }
    Atlas::builder()
        .name("drawn")
        .tile_size(TILE, TILE)
        .grid(columns, rows)
        .picture(picture)
        .build_in(scratch.path())
        .unwrap()
        .tileset
}

/// A map with `tileset` embedded, written into `scratch`.
fn map_with(scratch: &Scratch, tileset: Arc<Tileset>) -> PathBuf {
    let map = MapBuilder::new(UVec2::splat(TILE), UVec2::splat(4))
        .tileset(tileset)
        .tile_layer("ground", "")
        .build()
        .unwrap();
    let path = scratch.join("helpers.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// The window with `path` open and its tileset in the tileset editor's Terrain mode.
fn terrain_mode(scratch: &Scratch, path: &std::path::Path) -> (TestWindow, DocumentId) {
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(path);
    let doc = window.editor().current().unwrap();
    window.click("Edit Tileset");
    window.click("Terrain");
    (window, doc)
}

fn steps(window: &TestWindow, doc: DocumentId) -> Vec<String> {
    window
        .editor()
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .filter(|e| e.done)
        .map(|e| e.label)
        .collect()
}

fn edited(window: &TestWindow, doc: DocumentId) -> Arc<Tileset> {
    window.editor().edited_tileset(doc).unwrap().clone()
}

#[test]
fn layout_finds_a_block_and_labels_it_with_apply_and_label_all() {
    let scratch = Scratch::new("window-terrain-layouts").unwrap();
    let tileset = drawn_tileset(&scratch, 4, 3, "3×3 island", UVec2::ZERO);
    let path = map_with(&scratch, tileset);
    let (mut window, doc) = terrain_mode(&scratch, &path);

    window.click("Layout…");
    assert!(
        window.shows("3×3 island at column 0, row 0"),
        "the block is found in the image"
    );
    window.click("Apply");
    let set = edited(&window, doc).wang_sets[0].clone();
    assert_eq!(set.kind, WangSetType::Corner, "a new corner set");
    assert_eq!(set.colors.len(), 2, "grass and dirt, both new");
    let island = LAYOUTS.iter().find(|l| l.name == "3×3 island").unwrap();
    let expected: std::collections::BTreeMap<TileId, bt_core::WangId> = island
        .labels(UVec2::ZERO, 4, TerrainColor(1), TerrainColor(2))
        .into_iter()
        .filter_map(|(tile, label)| Some((tile, label?)))
        .collect();
    assert_eq!(set.tiles, expected, "the layout's labels");
    assert_eq!(steps(&window, doc), ["Apply Layout"]);

    window.press(Modifiers::COMMAND, egui::Key::Z);
    assert!(edited(&window, doc).wang_sets.is_empty(), "one step");

    // A layout chosen by hand starts where a tile is clicked.
    window.click("2×2 hole");
    let corner = window.tileset_tile(3);
    window.click_on_screen(corner, PointerButton::Primary, Modifiers::NONE);
    assert!(window.shows("“2×2 hole” from column 3, row 0"));
    // A click on the block found chooses it again.
    let middle = window.tileset_tile(5);
    window.click_on_screen(middle, PointerButton::Primary, Modifiers::NONE);
    assert!(window.shows("“3×3 island” from column 0, row 0"));

    window.click("Label All");
    let set = edited(&window, doc).wang_sets[0].clone();
    assert_eq!(set.tiles, expected, "every block found: the one island");
    assert_eq!(steps(&window, doc), ["Label All Layouts"]);
}

#[test]
fn auto_label_labels_the_chosen_terrain_from_the_tile_clicked() {
    let scratch = Scratch::new("window-terrain-auto-label").unwrap();
    // Over dirt: all grass, grass at the top-left, all dirt, grass along the top.
    let grass = [
        [true; 4],
        [false, false, false, true],
        [false; 4],
        [true, false, false, true],
    ];
    let mut picture = blank_picture(4 * TILE, TILE);
    for (column, on) in (0..).zip(grass) {
        let mut colors = [None; 9];
        for (lane, on) in [1, 3, 5, 7].into_iter().zip(on) {
            colors[lane] = Some(if on { GRASS } else { DIRT });
        }
        let at = UVec2::new(column * TILE, 0);
        paint_lanes(&mut picture, at, TILE, WangSetType::Corner, colors);
    }
    let atlas = Atlas::builder()
        .name("strip")
        .tile_size(TILE, TILE)
        .grid(4, 1)
        .picture(picture)
        .build_in(scratch.path())
        .unwrap();
    let tileset = TerrainSetBuilder::new(WangSetType::Corner)
        .terrains(&["Grass", "Dirt"])
        .added_to((*atlas.tileset).clone());
    let path = map_with(&scratch, Arc::new(tileset));
    let (mut window, doc) = terrain_mode(&scratch, &path);

    window.click("Ground");
    window.click("Auto-label…");
    let first = window.tileset_tile(0);
    window.click_on_screen(first, PointerButton::Primary, Modifiers::NONE);
    let labels = edited(&window, doc).wang_sets[0].tiles.clone();
    let expected = [
        (TileId(0), corners([1, 1, 1, 1])),
        (TileId(1), corners([0, 0, 0, 1])),
        (TileId(3), corners([1, 0, 0, 1])),
    ];
    assert_eq!(labels, expected.into_iter().collect());
    assert_eq!(steps(&window, doc), ["Auto-label Terrain"]);
}
