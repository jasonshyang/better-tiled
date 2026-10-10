//! The Custom Types editor's flows through the test window: opened from View, a class is added,
//! named and given a member with a default; an object of that class shows the member in the
//! Properties panel; saving the map writes the project; an enum gets values.

use bt_core::{ObjectId, Project, Property, Value};
use bt_test_data::Scratch;
use bt_tmx::WriteRequest;
use egui::{Key, Modifiers};

use crate::test_window::TestWindow;

fn command() -> Modifiers {
    Modifiers::COMMAND
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

#[test]
fn custom_types_are_made_in_their_editor_and_saved_with_the_project() {
    let scratch = Scratch::new("window-custom-types").unwrap();
    let folder = scratch.join("game");
    std::fs::create_dir_all(&folder).unwrap();
    let project_path = folder.join("game.tiled-project");
    WriteRequest::for_project(&Project::default(), &project_path)
        .write()
        .unwrap();
    let map = {
        use bt_core::MapObject;
        use bt_test_data::fixtures::MapBuilder;
        use glam::{DVec2, UVec2};
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 4))
            .object_layer(
                "things",
                vec![MapObject {
                    name: "chest".to_owned(),
                    pos: DVec2::splat(16.0),
                    size: DVec2::splat(16.0),
                    ..MapObject::default()
                }],
            )
            .build()
            .unwrap()
    };
    let path = folder.join("level.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();

    run_command(&mut window, "custom types editor");
    assert!(window.has_modal());
    window.click("Add Class");
    type_into(&mut window, "Name", "Chest");
    window.click("Add Member");
    window.type_text("gold");
    window.click("of type");
    window.click("int");
    window.click("Add");
    type_into(&mut window, "gold", "25");
    window.click("Add Enum");
    window.click("Add Value");
    window.click("Close");
    assert!(!window.has_modal());

    let types = window.editor().custom_types();
    let chest = types.value_class("Chest").expect("the class");
    assert_eq!(chest.members["gold"], Property::new(Value::Int(25)));
    assert_eq!(types.enum_type("Enum0").unwrap().values, ["Enum0_0"]);

    // An object of the class shows its member, inherited, in the Properties panel.
    window.click_last("Objects");
    window.click_holding("chest", Modifiers::NONE);
    type_into(&mut window, "Class", "Chest");
    let map = window.editor().map_of(doc).unwrap();
    assert_eq!(map.object(ObjectId(1)).unwrap().class, "Chest");
    assert!(window.shows("gold"), "the class's member");

    // Saving the map writes the project.
    window.press(command(), Key::S);
    let saved = bt_tmx::load_project(&project_path).unwrap().value;
    let chest = saved.custom_types.value_class("Chest").unwrap();
    assert_eq!(chest.members["gold"], Property::new(Value::Int(25)));
    assert!(!window.editor().has_unsaved());
}
