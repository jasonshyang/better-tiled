//! AutoMap in the window: the rules beside a dropped map are read on their own, the AutoMap
//! panel says what they are, ⌘M runs them as one step, and the panel then says what the run did.

use std::path::{Path, PathBuf};

use bt_core::{FileRef, LayerId, TilesetRef};
use bt_editor::ActionId;
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use glam::{IVec2, UVec2};

use crate::test_window::TestWindow;

/// A map or rule map of tile layers given as grids over the folder's `tiles.tsx`.
fn write_tiles_map(folder: &Path, name: &str, size: UVec2, layers: &[(&str, &str)]) -> PathBuf {
    let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
    let tileset_path = folder.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let mut builder = MapBuilder::new(UVec2::new(16, 16), size).tileset(atlas.tileset.clone());
    for (layer, cells) in layers {
        builder = builder.tile_layer(layer, cells);
    }
    let mut map = builder.build().unwrap();
    map.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", &tileset_path)),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    let path = folder.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

#[test]
fn automap_runs_from_its_key_and_the_panel_says_what_it_did() {
    let scratch = Scratch::new("window-automap").unwrap();
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("ground", "0 0 0\n0 0 0"), ("top", ". . .\n. . .")],
    );
    write_tiles_map(
        scratch.path(),
        "flowers.tmx",
        UVec2::new(1, 1),
        &[("input_ground", "0"), ("output_top", "6")],
    );
    std::fs::write(scratch.join("rules.txt"), "flowers.tmx\n").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&map);
    let doc = window.editor().current().unwrap();

    window.click("AutoMap");
    assert!(
        window.shows("Rules: rules.txt"),
        "where the rules come from"
    );
    assert!(window.shows("1 rule map, 1 rule"));
    assert_eq!(
        window
            .editor()
            .keymap()
            .key_text(ActionId::AutoMap)
            .as_deref(),
        Some(if cfg!(target_os = "macos") {
            "⌘M"
        } else {
            "Ctrl+M"
        })
    );

    window.press(egui::Modifiers::COMMAND, egui::Key::M);
    let top = |window: &TestWindow, at: IVec2| {
        let map = window.editor().map_of(doc).unwrap();
        let layer = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
        layer.cell(at).slot().is_some()
    };
    assert!(top(&window, IVec2::new(0, 0)) && top(&window, IVec2::new(2, 1)));
    assert!(window.shows("6 matches over the whole map"));
    assert!(window.shows("flowers.tmx: 6 matches"));

    window.press(egui::Modifiers::COMMAND, egui::Key::Z);
    assert!(!top(&window, IVec2::new(0, 0)), "one step to undo");
}
