//! The window's flows (`06-application.md`, §15), driven through the test window: a dropped map
//! opens and saves unchanged, the Layers panel's toggles are undoable edits, shortcuts run
//! commands, closing with unsaved work asks first, the session brings the files back, and an
//! exit nobody answered leaves the work to recover at the next start.

use std::path::PathBuf;

use bt_core::LayerId;
use bt_editor::ActionId;
use bt_test_data::Scratch;
use glam::IVec2;

use crate::platform::SystemQuit;
use crate::test_window::TestWindow;

/// Tiled's desert example copied into `scratch`, or `None` when the reference data is missing
/// and may be skipped.
fn desert(scratch: &Scratch) -> Option<PathBuf> {
    let examples = bt_test_data::test_data("reference/tiled/examples")?;
    for name in ["desert.tmx", "desert.tsx", "tmw_desert_spacing.png"] {
        std::fs::copy(examples.join(name), scratch.join(name)).unwrap();
    }
    Some(scratch.join("desert.tmx"))
}

fn command() -> egui::Modifiers {
    egui::Modifiers::COMMAND
}

/// Whether the desert's ground layer is shown.
fn ground_visible(window: &TestWindow) -> bool {
    let doc = window.editor().current().unwrap();
    let map = window.editor().map_of(doc).unwrap();
    map.layer(LayerId(1)).unwrap().visible
}

#[test]
fn a_dropped_map_opens_and_saves_unchanged() {
    let scratch = Scratch::new("window-open-save").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let original = std::fs::read(&path).unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().expect("the dropped map is open");
    assert_eq!(window.editor().path_of(doc), Some(path.clone()));
    // Whatever is on disk now, Save writes the map as it was read.
    std::fs::write(&path, "not a map").unwrap();
    window.press(command(), egui::Key::S);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!window.editor().has_unsaved());
}

#[test]
fn a_pack_map_opens_and_saves_unchanged() {
    let Some(pack) = bt_test_data::test_data("assets/ERW - Ancient Ruins V 2.3") else {
        return;
    };
    let scratch = Scratch::new("window-pack-save").unwrap();
    let copy = scratch.mirror(&pack, "pack").unwrap();
    let path = copy.join("TiledMap Editor/Ancient Ruins example map.tmx");
    let original = std::fs::read(&path).unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().expect("the pack's map is open");
    assert!(!window.editor().map_of(doc).unwrap().layers.is_empty());
    std::fs::write(&path, "not a map").unwrap();
    window.press(command(), egui::Key::S);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn a_map_changed_on_disk_reloads_or_asks() {
    let scratch = Scratch::new("window-watch").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    window.poll_files();
    let rename = |from: &str, to: &str| {
        let text = std::fs::read_to_string(&path).unwrap();
        let renamed = text.replace(&format!("name=\"{from}\""), &format!("name=\"{to}\""));
        assert_ne!(text, renamed);
        std::fs::write(&path, renamed).unwrap();
    };
    let layer_name = |window: &TestWindow| {
        let doc = window.editor().current().unwrap();
        window.editor().map_of(doc).unwrap().layers[0].name.clone()
    };
    rename("Ground", "Sand");
    window.poll_files();
    assert_eq!(layer_name(&window), "Sand", "a clean map takes the change");
    assert!(!window.has_modal());

    // With an unsaved change, the window asks; keeping it leaves the map as it is here.
    window.click("👁");
    rename("Sand", "Dunes");
    window.poll_files();
    assert!(window.has_modal(), "a conflict asks");
    window.click("Keep My Changes");
    assert!(!window.has_modal());
    assert_eq!(layer_name(&window), "Sand");
    assert!(window.editor().has_unsaved());
}

#[test]
fn layer_toggles_are_undoable_edits() {
    let scratch = Scratch::new("window-layer-toggles").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    assert!(ground_visible(&window));
    window.click("👁");
    assert!(!ground_visible(&window));
    assert!(window.editor().has_unsaved());
    window.press(command(), egui::Key::Z);
    assert!(ground_visible(&window));
    assert!(!window.editor().has_unsaved());
}

#[test]
fn a_layer_is_renamed_in_place_and_the_rename_undoes() {
    let scratch = Scratch::new("window-rename-layer").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let name = |window: &TestWindow| {
        let doc = window.editor().current().unwrap();
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .layer(LayerId(1))
            .unwrap()
            .name
            .clone()
    };
    window.double_click("Ground");
    window.press(command(), egui::Key::A);
    window.type_text("Sand");
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(name(&window), "Sand");
    // Escape drops what was typed.
    window.double_click("Sand");
    window.type_text("stone");
    window.press(egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(name(&window), "Sand");
    window.press(command(), egui::Key::Z);
    assert_eq!(name(&window), "Ground");
}

#[test]
fn shortcuts_run_commands() {
    let scratch = Scratch::new("window-shortcuts").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let grid = window.editor().checked(ActionId::ShowGrid);
    window.press(command(), egui::Key::G);
    assert_eq!(
        window.editor().checked(ActionId::ShowGrid),
        grid.map(|on| !on)
    );
    window.press(command(), egui::Key::W);
    assert_eq!(window.editor().current(), None);
}

#[test]
fn the_palette_finds_and_runs_a_command() {
    let scratch = Scratch::new("window-palette").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let grid = window.editor().checked(ActionId::ShowGrid);
    window.press(command() | egui::Modifiers::SHIFT, egui::Key::P);
    assert!(window.has_modal(), "the palette is open");
    window.type_text("show grid");
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
    assert!(!window.has_modal());
    assert_eq!(
        window.editor().checked(ActionId::ShowGrid),
        grid.map(|on| !on)
    );
    // Escape closes it without running anything.
    window.press(command() | egui::Modifiers::SHIFT, egui::Key::P);
    window.press(egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!window.has_modal());
}

#[test]
fn a_map_and_a_tileset_are_made_from_scratch() {
    let scratch = Scratch::new("window-new-map").unwrap();
    let image = bt_test_data::fixtures::Atlas::builder()
        .name("terrain")
        .tile_size(32, 32)
        .grid(4, 2)
        .build_in(scratch.path())
        .unwrap();
    let image_path = scratch.join("terrain.png");
    let _ = image;
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.press(command(), egui::Key::N);
    assert!(window.has_modal(), "New Map asks");
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
    assert!(!window.has_modal());
    let doc = window.editor().current().expect("the new map is open");
    let map = window.editor().map_of(doc).unwrap();
    assert_eq!((map.width, map.height, map.tile_width), (30, 20, 32));
    assert_eq!(map.layers[0].name, "Tile Layer 1");
    assert!(window.editor().has_unsaved(), "a new map is unsaved");

    window.press(command() | egui::Modifiers::SHIFT, egui::Key::P);
    window.type_text("new tileset");
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
    assert!(window.has_modal(), "New Tileset asks");
    window.click("Browse…");
    window.answer_file_dialog(bt_editor::FileRequest::TilesetImage, &image_path);
    window.click("OK");
    assert!(!window.has_modal());
    let map = window.editor().map_of(doc).unwrap();
    assert_eq!(map.tilesets.len(), 1);
    assert_eq!(
        map.tilesets[0].tileset.name, "terrain",
        "named after the image"
    );
    assert_eq!(map.tilesets[0].tileset.tile_count(), 8);
}

#[test]
fn the_selection_tools_select_on_the_map() {
    let scratch = Scratch::new("window-select").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let selected = |window: &TestWindow| {
        let doc = window.editor().current().unwrap();
        window
            .editor()
            .document(doc)
            .unwrap()
            .selection()
            .tiles
            .clone()
    };
    window.press(egui::Modifiers::NONE, egui::Key::R);
    assert_eq!(
        window.editor().tools().tool(),
        bt_editor::ToolKind::RectSelect
    );
    window.drag_cells(IVec2::new(2, 3), IVec2::new(5, 6), egui::Modifiers::NONE);
    assert_eq!(selected(&window).len(), 16);
    assert_eq!(
        selected(&window).bounds(),
        Some(bt_utils::IRect {
            x: 2,
            y: 3,
            w: 4,
            h: 4
        })
    );
    // Shift adds another rectangle; a click without dragging clears.
    window.drag_cells(
        IVec2::new(10, 10),
        IVec2::new(11, 10),
        egui::Modifiers::SHIFT,
    );
    assert_eq!(selected(&window).len(), 18);
    window.click_cell(
        IVec2::new(20, 20),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert!(selected(&window).is_empty());

    // The wand takes what it previewed: the connected cells like the one clicked.
    window.press(egui::Modifiers::NONE, egui::Key::W);
    let doc = window.editor().current().unwrap();
    let expected = window.editor().wand_area(doc, IVec2::new(0, 0), &[]);
    assert!(!expected.is_empty());
    window.click_cell(
        IVec2::new(0, 0),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
    );
    assert_eq!(selected(&window), expected);
}

#[test]
fn closing_with_unsaved_work_asks_first() {
    let scratch = Scratch::new("window-close-unsaved").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    window.click("👁");
    window.request_close();
    assert!(window.has_modal() && !window.is_closing());
    window.click("Cancel");
    assert!(!window.has_modal() && !window.is_closing());
    window.request_close();
    window.click("Don't Save");
    assert!(window.is_closing());
}

#[test]
fn a_system_quit_asks_while_something_is_unsaved() {
    let scratch = Scratch::new("window-system-quit").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    assert_eq!(window.system_quit(), SystemQuit::Proceed, "nothing unsaved");
    window.click("👁");
    assert_eq!(window.system_quit(), SystemQuit::Cancel);
    assert!(
        window.has_modal() && !window.is_closing(),
        "the Quit command asks"
    );
    window.click("Don't Save");
    assert!(window.is_closing());
    assert_eq!(
        window.system_quit(),
        SystemQuit::Proceed,
        "quitting goes ahead now"
    );
}

#[test]
fn the_session_brings_back_the_open_files() {
    let scratch = Scratch::new("window-session").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let data = scratch.join("data");
    let mut window = TestWindow::with_data(&data);
    window.drop_file(&path);
    window.exit();
    let window = TestWindow::with_data(&data);
    let doc = window
        .editor()
        .current()
        .expect("the session's file is open again");
    assert_eq!(window.editor().path_of(doc), Some(path));
    assert!(window.editor().camera_of(doc).is_some(), "with its camera");
}

#[test]
fn an_unanswered_exit_leaves_the_work_to_recover() {
    let scratch = Scratch::new("window-recovery").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let data = scratch.join("data");
    let mut window = TestWindow::run_as(&data, 41);
    window.drop_file(&path);
    window.click("👁");
    // The system ends the run: nobody was asked.
    window.exit();
    let mut window = TestWindow::run_as(&data, 42);
    assert!(window.has_modal(), "the next start offers the work back");
    window.click("Recover");
    assert!(!ground_visible(&window));
    assert!(window.editor().has_unsaved());
}
