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
fn preferences_change_settings_and_keys_and_are_saved() {
    let scratch = Scratch::new("window-preferences").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let data = scratch.join("data");
    let mut window = TestWindow::with_data(&data);
    window.drop_file(&path);
    let open_preferences = |window: &mut TestWindow| {
        window.press(command() | egui::Modifiers::SHIFT, egui::Key::P);
        window.type_text("preferences");
        window.press(egui::Modifiers::NONE, egui::Key::Enter);
        assert!(window.has_modal(), "the preferences are open");
    };
    open_preferences(&mut window);
    window.click("Interface");
    window.click("The mouse wheel zooms");
    window.click("OK");
    assert!(!window.editor().settings().behaviour.wheel_zooms);

    // A key recorded for Show Grid runs it.
    open_preferences(&mut window);
    window.click("Keyboard");
    window.type_text("show grid");
    window.click("Record");
    window.press(egui::Modifiers::NONE, egui::Key::F5);
    window.click("OK");
    let grid = window.editor().checked(ActionId::ShowGrid);
    window.press(egui::Modifiers::NONE, egui::Key::F5);
    assert_eq!(
        window.editor().checked(ActionId::ShowGrid),
        grid.map(|on| !on)
    );

    // Both were written to settings.toml, which the next start reads.
    window.exit();
    let saved = bt_editor::Settings::load(&data.join("settings.toml")).unwrap();
    assert!(!saved.behaviour.wheel_zooms);
    assert_eq!(saved.keymap.get("show_grid"), Some(&vec!["f5".to_owned()]));
}

#[test]
fn the_picker_brush_and_eraser_paint_on_the_map() {
    let scratch = Scratch::new("window-paint").unwrap();
    let Some(path) = desert(&scratch) else {
        return;
    };
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let cell_at = |window: &TestWindow, at: IVec2| {
        let doc = window.editor().current().unwrap();
        window.editor().map_of(doc).unwrap().layers[0]
            .as_tile_layer()
            .unwrap()
            .cell(at)
    };
    // A tile found nowhere on the row the stroke paints.
    let row: Vec<_> = (10..=14)
        .map(|x| cell_at(&window, IVec2::new(x, 10)))
        .collect();
    let source = (0..40)
        .flat_map(|y| (0..40).map(move |x| IVec2::new(x, y)))
        .find(|at| !row.contains(&cell_at(&window, *at)))
        .expect("the desert has more than one kind of tile");
    let picked = cell_at(&window, source);
    window.press(egui::Modifiers::NONE, egui::Key::I);
    window.click_cell(source, egui::PointerButton::Primary, egui::Modifiers::NONE);
    assert_eq!(window.editor().tools().tool(), bt_editor::ToolKind::Brush);
    window.drag_cells(
        IVec2::new(10, 10),
        IVec2::new(14, 10),
        egui::Modifiers::NONE,
    );
    for x in 10..=14 {
        assert_eq!(cell_at(&window, IVec2::new(x, 10)), picked, "{x}");
    }
    window.press(command(), egui::Key::Z);
    assert_ne!(
        cell_at(&window, IVec2::new(12, 10)),
        picked,
        "the stroke is one step"
    );

    window.press(egui::Modifiers::NONE, egui::Key::E);
    window.drag_cells(IVec2::new(3, 3), IVec2::new(5, 3), egui::Modifiers::NONE);
    for x in 3..=5 {
        assert!(cell_at(&window, IVec2::new(x, 3)).is_empty());
    }
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

/// Runs a command by its name through the palette.
fn run_command(window: &mut TestWindow, name: &str) {
    window.press(command() | egui::Modifiers::SHIFT, egui::Key::P);
    window.type_text(name);
    window.press(egui::Modifiers::NONE, egui::Key::Enter);
}

#[test]
fn objects_are_drawn_selected_moved_and_removed_on_the_map() {
    use bt_core::{MapObject, Shape};
    use bt_editor::ToolKind;
    use egui::{Key, Modifiers, PointerButton};
    use glam::DVec2;

    let scratch = Scratch::new("window-objects").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.press(command(), Key::N);
    window.press(Modifiers::NONE, Key::Enter);
    run_command(&mut window, "new object layer");
    run_command(&mut window, "snap to grid");
    let doc = window.editor().current().unwrap();
    let objects = |window: &TestWindow| -> Vec<MapObject> {
        let map = window.editor().map_of(doc).unwrap();
        map.layers
            .iter()
            .filter_map(|layer| layer.as_object_layer())
            .flat_map(|layer| layer.objects.iter().cloned())
            .collect()
    };
    let selected = |window: &TestWindow| {
        window
            .editor()
            .document(doc)
            .unwrap()
            .selection()
            .objects
            .clone()
    };
    let v = DVec2::new;
    let click = |window: &mut TestWindow, at: DVec2| {
        window.click_world(at, PointerButton::Primary, Modifiers::NONE);
    };

    // R draws a rectangle from corner to corner, on the grid; the new object is selected.
    window.press(Modifiers::NONE, Key::R);
    assert_eq!(window.editor().tools().tool(), ToolKind::InsertRectangle);
    window.drag_world(v(70.0, 100.0), v(170.0, 200.0), Modifiers::NONE);
    let [box_] = &objects(&window)[..] else {
        panic!("one rectangle: {:?}", objects(&window));
    };
    assert_eq!((box_.pos, box_.size), (v(64.0, 96.0), v(96.0, 96.0)));
    assert_eq!(selected(&window), [box_.id]);

    // S: a click on empty space clears the selection, a click on the object selects it, and a
    // drag moves it on the grid as one step.
    window.press(Modifiers::NONE, Key::S);
    click(&mut window, v(600.0, 500.0));
    assert!(selected(&window).is_empty());
    click(&mut window, v(100.0, 130.0));
    assert_eq!(selected(&window), [box_.id]);
    window.drag_world(v(100.0, 130.0), v(140.0, 130.0), Modifiers::NONE);
    assert_eq!(objects(&window)[0].pos, v(96.0, 96.0));
    window.press(command(), Key::Z);
    assert_eq!(
        objects(&window)[0].pos,
        v(64.0, 96.0),
        "the move is one step"
    );

    // A rubber band selects what it touches; Delete removes it, undoably.
    click(&mut window, v(600.0, 500.0));
    window.drag_world(v(10.0, 10.0), v(80.0, 110.0), Modifiers::NONE);
    assert_eq!(selected(&window), [box_.id]);
    window.press(Modifiers::NONE, Key::Delete);
    assert!(objects(&window).is_empty());
    window.press(command(), Key::Z);
    assert_eq!(objects(&window).len(), 1);

    // P: clicks add points, and clicking the first again closes a polygon...
    window.press(Modifiers::NONE, Key::P);
    for at in [
        v(40.0, 40.0),
        v(120.0, 40.0),
        v(120.0, 120.0),
        v(33.0, 33.0),
    ] {
        click(&mut window, at);
    }
    let polygon = objects(&window).last().cloned().unwrap();
    assert_eq!(polygon.pos, v(32.0, 32.0));
    assert_eq!(
        polygon.shape,
        Shape::Polygon(vec![DVec2::ZERO, v(96.0, 0.0), v(96.0, 96.0)])
    );
    // ...while Enter finishes a polyline.
    for at in [v(200.0, 40.0), v(300.0, 40.0)] {
        click(&mut window, at);
    }
    window.press(Modifiers::NONE, Key::Enter);
    let line = objects(&window).last().cloned().unwrap();
    assert_eq!(line.pos, v(192.0, 32.0));
    assert_eq!(line.shape, Shape::Polyline(vec![DVec2::ZERO, v(96.0, 0.0)]));
    assert_eq!(objects(&window).len(), 3);

    // Escape with nothing under way goes back to Select Objects, as in Tiled.
    window.press(Modifiers::NONE, Key::Escape);
    assert_eq!(window.editor().tools().tool(), ToolKind::SelectObjects);
}

#[test]
fn tools_measure_from_the_current_layers_offset() {
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use egui::{Key, Modifiers, PointerButton};
    use glam::{DVec2, UVec2};

    let scratch = Scratch::new("window-offset").unwrap();
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let empty_row = [". "; 8].concat();
    let ground = format!("1 . . . . . . .\n{}", vec![empty_row; 5].join("\n"));
    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(8, 6))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", &ground)
        .object_layer("things", Vec::new())
        .build()
        .unwrap();
    map.layers[0].offset = DVec2::new(16.0, 16.0);
    map.layers[1].offset = DVec2::new(8.0, 8.0);
    let path = scratch.join("offset.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let primary = PointerButton::Primary;

    // The object layer is current: a point placed at (40, 40) on screen is at (32, 32) in it.
    window.press(Modifiers::NONE, Key::I);
    window.click_world(DVec2::new(40.0, 40.0), primary, Modifiers::NONE);
    let map = window.editor().map_of(doc).unwrap();
    let things = map.layers[1].as_object_layer().unwrap();
    assert_eq!(
        things.objects.iter().map(|o| o.pos).collect::<Vec<_>>(),
        [DVec2::new(32.0, 32.0)]
    );

    // On the ground, one cell off, the picker and the brush take the cell drawn under the
    // pointer.
    window.press(command(), Key::PageDown);
    window.press(Modifiers::NONE, Key::I);
    window.click_world(DVec2::new(24.0, 24.0), primary, Modifiers::NONE);
    window.click_world(DVec2::new(56.0, 56.0), primary, Modifiers::NONE);
    let map = window.editor().map_of(doc).unwrap();
    let ground = map.layers[0].as_tile_layer().unwrap();
    let first = ground.cell(IVec2::ZERO);
    assert!(!first.is_empty());
    assert_eq!(
        ground.cell(IVec2::new(2, 2)),
        first,
        "painted where it shows"
    );
    assert!(ground.cell(IVec2::new(3, 3)).is_empty());
}

#[test]
fn a_selection_drags_mirrors_and_lets_go_of_its_tiles() {
    use bt_core::Flip;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use egui::{Key, Modifiers};
    use glam::UVec2;

    let scratch = Scratch::new("window-carry").unwrap();
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(6, 4))
        .tileset(atlas.tileset.clone())
        .tile_layer(
            "ground",
            "1 2 3 4 5 6\n7 8 9 10 11 12\n1 2 3 4 5 6\n7 8 9 10 11 12",
        )
        .build()
        .unwrap();
    let path = scratch.join("carry.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let at = |window: &TestWindow, x: i32, y: i32| {
        window.editor().map_of(doc).unwrap().layers[0]
            .as_tile_layer()
            .unwrap()
            .cell(IVec2::new(x, y))
    };
    let first = at(&window, 0, 0);
    let second = at(&window, 1, 0);
    let carries = |window: &TestWindow| {
        window
            .editor()
            .document(doc)
            .unwrap()
            .selection()
            .carries_tiles()
    };

    // R, then a drag selects the first two cells; a drag from inside them moves their tiles.
    window.press(Modifiers::NONE, Key::R);
    window.drag_cells(IVec2::new(0, 0), IVec2::new(1, 0), Modifiers::NONE);
    window.drag_cells(IVec2::new(0, 0), IVec2::new(2, 2), Modifiers::NONE);
    assert_eq!((at(&window, 2, 2), at(&window, 3, 2)), (first, second));
    assert!(at(&window, 0, 0).is_empty() && at(&window, 1, 0).is_empty());
    assert!(carries(&window));

    // X mirrors the carried tiles in place; one undo takes the mirror back.
    window.press(Modifiers::NONE, Key::X);
    assert_eq!(at(&window, 2, 2), second.with_flip(Flip::H));
    window.press(command(), Key::Z);
    assert_eq!(at(&window, 2, 2), first);
    assert!(carries(&window), "undo brings the carry back");

    // Escape lets the tiles go where they are, the selection with them.
    window.press(Modifiers::NONE, Key::Escape);
    assert!(!carries(&window));
    assert!(
        window
            .editor()
            .document(doc)
            .unwrap()
            .selection()
            .tiles
            .is_empty()
    );
    assert_eq!(at(&window, 2, 2), first);
}

#[test]
fn new_objects_follow_the_pointer_and_command_toggles_snapping() {
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use egui::{Key, Modifiers, PointerButton};
    use glam::{DVec2, UVec2};

    let scratch = Scratch::new("window-insert").unwrap();
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let empty_row = [". "; 8].concat();
    let ground = format!("1 . . . . . . .\n{}", vec![empty_row; 5].join("\n"));
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(8, 6))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", &ground)
        .object_layer("things", Vec::new())
        .build()
        .unwrap();
    let path = scratch.join("insert.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let primary = PointerButton::Primary;

    // The ground's first tile becomes the brush, through the picker.
    window.press(command(), Key::PageDown);
    window.press(Modifiers::NONE, Key::I);
    window.click_cell(IVec2::ZERO, primary, Modifiers::NONE);
    window.press(command(), Key::PageUp);

    // T places the tile with its image centred on the pointer: bottom-left at (32, 48).
    window.press(Modifiers::NONE, Key::T);
    window.click_world(DVec2::new(40.0, 40.0), primary, Modifiers::NONE);
    // I with Command snaps to the grid, though the preference is not to snap.
    window.press(Modifiers::NONE, Key::I);
    window.click_world(DVec2::new(37.0, 43.0), primary, command());
    let map = window.editor().map_of(doc).unwrap();
    let things = map.layers[1].as_object_layer().unwrap();
    let placed: Vec<_> = things
        .objects
        .iter()
        .map(|o| (o.pos, o.size, o.tile.is_some()))
        .collect();
    assert_eq!(
        placed,
        [
            (DVec2::new(32.0, 48.0), DVec2::splat(16.0), true),
            (DVec2::new(32.0, 48.0), DVec2::ZERO, false),
        ]
    );
}

#[test]
fn a_tilesets_collision_shapes_and_animations_are_edited_in_place_of_the_map() {
    use bt_core::{Frame, TileId};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_tmx::WriteRequest;
    use egui::{Key, Modifiers, PointerButton};
    use glam::{DVec2, UVec2};

    let scratch = Scratch::new("window-tileset-editor").unwrap();
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "1 1 1 1\n1 1 1 1\n1 1 1 1")
        .build()
        .unwrap();
    let path = scratch.join("edit-tileset.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let tile = |window: &TestWindow| {
        window
            .editor()
            .edited_tileset(doc)
            .and_then(|tileset| tileset.tile(TileId(3)).cloned())
    };

    window.click("Edit Tileset");
    assert!(window.editor().edited_tileset(doc).is_some());
    let at = window.tileset_tile(3);
    window.click_on_screen(at, PointerButton::Primary, Modifiers::NONE);
    let editing = window
        .editor()
        .document(doc)
        .unwrap()
        .tileset_editing()
        .cloned()
        .unwrap();
    assert_eq!(editing.current_tile(), Some(TileId(3)));

    // Collision: R draws a rectangle on the tile.
    window.click("Collision");
    window.press(Modifiers::NONE, Key::R);
    let from = window.collision_point(DVec2::new(2.0, 2.0));
    let to = window.collision_point(DVec2::new(10.0, 10.0));
    window.drag_on_screen(from, to, Modifiers::NONE);
    let shapes = tile(&window).and_then(|data| data.collision).unwrap();
    let rect = &shapes.layer.objects[0];
    assert!(
        (rect.pos - DVec2::new(2.0, 2.0)).length() < 0.01,
        "{}",
        rect.pos
    );
    assert!(
        (rect.size - DVec2::new(8.0, 8.0)).length() < 0.01,
        "{}",
        rect.size
    );
    assert_eq!(
        window.editor().map_of(doc).unwrap().layers.len(),
        1,
        "the map behind is untouched"
    );

    // Animation: tiles clicked in the palette become frames.
    window.click("Animation");
    for frame in [0, 1] {
        let at = window.palette_tile(frame);
        window.click_on_screen(at, PointerButton::Primary, Modifiers::NONE);
    }
    let frames: Vec<u32> = tile(&window)
        .unwrap()
        .animation
        .iter()
        .map(|f: &Frame| f.tile.0)
        .collect();
    assert_eq!(frames, [0, 1]);

    // Back to the map, which holds the changes, unsaved.
    window.click("◀ Map");
    assert!(
        window
            .editor()
            .document(doc)
            .unwrap()
            .tileset_editing()
            .is_none()
    );
    assert!(window.editor().has_unsaved());
    let reference = &window.editor().map_of(doc).unwrap().tilesets[0];
    assert_eq!(
        reference.tileset.tile(TileId(3)).unwrap().animation.len(),
        2
    );
}

#[test]
fn select_objects_resizes_by_its_handles_and_a_second_click_rotates() {
    use egui::{Key, Modifiers, PointerButton};
    use glam::DVec2;

    let scratch = Scratch::new("window-handles").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.press(command(), Key::N);
    window.press(Modifiers::NONE, Key::Enter);
    run_command(&mut window, "new object layer");
    let doc = window.editor().current().unwrap();
    let object = |window: &TestWindow| {
        window
            .editor()
            .map_of(doc)
            .unwrap()
            .layers
            .iter()
            .find_map(|layer| layer.as_object_layer())
            .and_then(|layer| layer.objects.first().cloned())
            .unwrap()
    };
    let near = |a: DVec2, b: DVec2| (a - b).length() < 0.5;

    window.press(Modifiers::NONE, Key::R);
    window.drag_world(
        DVec2::new(32.0, 32.0),
        DVec2::new(64.0, 64.0),
        Modifiers::NONE,
    );
    window.press(Modifiers::NONE, Key::S);

    // The bottom-right handle follows the pointer; the top-left corner stays.
    window.drag_world(
        DVec2::new(64.0, 64.0),
        DVec2::new(96.0, 80.0),
        Modifiers::NONE,
    );
    let resized = object(&window);
    assert!(near(resized.pos, DVec2::new(32.0, 32.0)), "{}", resized.pos);
    assert!(
        near(resized.size, DVec2::new(64.0, 48.0)),
        "{}",
        resized.size
    );
    window.press(command(), Key::Z);
    assert!(
        near(object(&window).size, DVec2::new(32.0, 32.0)),
        "one step"
    );

    // A click on the selected object shows rotate handles; a quarter turn of one turns it.
    let inside = window.screen_of_world(DVec2::new(48.0, 48.0));
    window.click_on_screen(inside, PointerButton::Primary, Modifiers::NONE);
    let frame = window.editor().selection_handles(doc).unwrap();
    let centre = window.screen_of_world(frame.centre());
    let corner = window.screen_of_world(frame.corners[2]);
    let out = (corner - centre).normalized() * 14.0;
    let start = corner + out;
    let arm = start - centre;
    let end = centre + egui::vec2(-arm.y, arm.x);
    window.drag_on_screen(start, end, Modifiers::NONE);
    let turned = object(&window);
    assert!(
        (turned.rotation - 90.0).abs() < 1.0,
        "turned {} degrees",
        turned.rotation
    );
}
