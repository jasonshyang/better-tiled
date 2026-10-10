//! The Properties panel's flows through the test window: it shows the map, then the objects
//! selected, then the map again from Map ▸ Map Properties; custom properties are added, typed
//! into and given files; attributes are typed into, each run of typing one step; and it stays
//! beside the tileset editor, showing its tileset and tiles.

use std::path::PathBuf;

use bt_core::{MapObject, ObjectId, TileId, Value};
use bt_editor::{FileRequest, Inspected, Step};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use egui::{Key, Modifiers};
use glam::{DVec2, UVec2};

use crate::test_window::TestWindow;

fn command() -> Modifiers {
    Modifiers::COMMAND
}

/// A map with a tile layer and an object layer holding a chest and a door, written in
/// `scratch`.
fn map_with_objects(scratch: &Scratch) -> PathBuf {
    let object = |name: &str, x: f64| MapObject {
        name: name.to_owned(),
        pos: DVec2::new(x, 16.0),
        size: DVec2::splat(16.0),
        ..MapObject::default()
    };
    let atlas = Atlas::builder()
        .name("tiles")
        .build_in(scratch.path())
        .unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(6, 4))
        .tileset(atlas.tileset)
        .tile_layer(
            "ground",
            "1 1 1 1 1 1\n1 1 1 1 1 1\n1 1 1 1 1 1\n1 1 1 1 1 1",
        )
        .object_layer("things", vec![object("chest", 16.0), object("door", 64.0)])
        .build()
        .unwrap();
    let path = scratch.join("level.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// Runs a command by its name through the palette.
fn run_command(window: &mut TestWindow, name: &str) {
    window.press(command() | Modifiers::SHIFT, Key::P);
    window.type_text(name);
    window.press(Modifiers::NONE, Key::Enter);
}

/// Replaces what the field labelled `label` holds with `text`, and leaves it with Enter.
fn type_into(window: &mut TestWindow, label: &str, text: &str) {
    window.focus(label);
    window.press(command(), Key::A);
    window.type_text(text);
    window.press(Modifiers::NONE, Key::Enter);
}

/// Adds a custom property through the Add form.
fn add_property(window: &mut TestWindow, name: &str, type_name: &str) {
    window.click("Add Property");
    window.type_text(name);
    window.click("of type");
    window.click(type_name);
    window.click("Add");
}

#[test]
fn the_properties_panel_edits_the_map_and_the_selected_objects() {
    let scratch = Scratch::new("window-properties").unwrap();
    let path = map_with_objects(&scratch);
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    let map_property = |window: &TestWindow, name: &str| {
        let map = window.editor().map_of(doc).unwrap();
        map.properties
            .get(name)
            .map(|property| property.value.clone())
    };
    assert!(window.shows("Tile Render Order"), "the map's attributes");

    // A property is added with a name and a type, then typed into; the typing is one step.
    add_property(&mut window, "hp", "int");
    assert_eq!(map_property(&window, "hp"), Some(Value::Int(0)));
    type_into(&mut window, "hp", "42");
    assert_eq!(map_property(&window, "hp"), Some(Value::Int(42)));
    window.press(command(), Key::Z);
    assert_eq!(map_property(&window, "hp"), Some(Value::Int(0)));

    // A file is chosen through the file dialog, and kept relative to the map.
    add_property(&mut window, "script", "file");
    window.click("…");
    window.answer_file_dialog(
        FileRequest::PropertyFile {
            doc,
            inspected: Inspected::Map,
            path: vec![Step::Member("script".to_owned())],
        },
        &scratch.join("door.lua"),
    );
    assert_eq!(
        map_property(&window, "script"),
        Some(Value::File("door.lua".to_owned()))
    );

    // Selecting an object shows it; its name is typed in place, and the typing undoes as one.
    window.click_last("Objects");
    window.click_holding("chest", Modifiers::NONE);
    assert_eq!(
        window.editor().inspected(doc),
        Some(Inspected::Objects(vec![ObjectId(1)]))
    );
    let name = |window: &TestWindow| {
        let map = window.editor().map_of(doc).unwrap();
        map.object(ObjectId(1)).unwrap().name.clone()
    };
    type_into(&mut window, "Name", "big box");
    assert_eq!(name(&window), "big box");
    window.press(command(), Key::Z);
    assert_eq!(name(&window), "chest");
    assert!(!window.shows("Tile Render Order"));

    // Map Properties brings the map back.
    run_command(&mut window, "map properties");
    assert_eq!(window.editor().inspected(doc), Some(Inspected::Map));
    assert!(window.shows("Tile Render Order"));
}

#[test]
fn the_properties_panel_stays_beside_the_tileset_editor() {
    let scratch = Scratch::new("window-properties-tileset").unwrap();
    let path = map_with_objects(&scratch);
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();

    window.click("Edit Tileset");
    assert_eq!(window.editor().inspected(doc), Some(Inspected::Tileset));
    assert!(window.shows("Object Alignment"), "the tileset's attributes");
    let at = window.tileset_tile(3);
    window.click_on_screen(at, egui::PointerButton::Primary, Modifiers::NONE);
    assert_eq!(
        window.editor().inspected(doc),
        Some(Inspected::Tiles(vec![TileId(3)]))
    );
    type_into(&mut window, "Probability", "0.5");
    let tileset = window.editor().edited_tileset(doc).unwrap();
    assert_eq!(tileset.tile(TileId(3)).unwrap().probability, 0.5);
}
