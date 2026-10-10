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

#[test]
fn live_automap_binds_the_rules_to_the_layer_painted_on_from_the_panel() {
    let scratch = Scratch::new("window-live-automap").unwrap();
    // As in the Highlands pack: the rules read and write `walls`, the map paints on `plat1`.
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("walls", ". . .\n. . ."), ("plat1", "0 . 0\n. . .")],
    );
    write_tiles_map(
        scratch.path(),
        "platforms.tmx",
        UVec2::new(1, 1),
        &[("input_walls", "0"), ("output_walls", "6")],
    );
    std::fs::write(scratch.join("rules.txt"), "platforms.tmx\n").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&map);
    let doc = window.editor().current().unwrap();

    window.click("AutoMap");
    window.click("Live AutoMap");
    assert!(window.editor().document(doc).unwrap().automap().is_some());
    assert!(window.shows("reads and writes walls"));
    assert!(window.shows("empty"), "the rules read an empty layer");

    window.click_last("reads and writes walls");
    window.click_last("plat1");
    let shown = window.editor().shown_map(doc).unwrap();
    let plat1 = shown.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    assert_eq!(
        plat1.cell(IVec2::new(0, 0)).tile().0,
        6,
        "the rules ran on plat1"
    );
    assert!(!window.shows("empty"));
    let painted = window.editor().map_of(doc).unwrap();
    let plat1 = painted.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    assert_eq!(
        plat1.cell(IVec2::new(0, 0)).tile().0,
        0,
        "what was painted stays"
    );
}

#[test]
fn the_explain_tool_opens_the_automap_panel_on_the_cell_clicked() {
    use bt_editor::ToolKind;

    let scratch = Scratch::new("window-explain-tool").unwrap();
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
    assert!(window.shows("Layers"));

    window.press(egui::Modifiers::NONE, egui::Key::A);
    assert_eq!(window.editor().tools().tool(), ToolKind::Explain);
    window.click_cell(
        IVec2::new(2, 1),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert_eq!(window.editor().explained_cell(doc), Some(IVec2::new(2, 1)));
    assert!(
        window.shows("Explaining cell 2, 1"),
        "the AutoMap panel is shown"
    );
    assert!(window.shows("If AutoMap ran now"));
    assert!(window.shows("✔ rule 1 of flowers.tmx applied at 2, 1"));

    window.click("Open");
    let opened = window.editor().current().unwrap();
    assert_ne!(opened, doc, "the rule map opens in a tab of its own");
    assert!(
        window
            .editor()
            .path_of(opened)
            .is_some_and(|path| path.ends_with("flowers.tmx"))
    );
}
