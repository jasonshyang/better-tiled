//! Template bases use the object Properties panel and the real canvas renderer.

use crate::test_window::TestWindow;
use bt_core::{MapObject, ObjectTemplate, Shape};
use bt_editor::{ActionId, Intent};
use bt_test_data::Scratch;
use egui::{Key, Modifiers};
use glam::DVec2;

#[test]
fn template_properties_edit_save_and_undo_the_definition() {
    let scratch = Scratch::new("window-template-base").unwrap();
    let path = scratch.join("chest.tx");
    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "Chest".into(),
            class: "Treasure".into(),
            size: DVec2::new(64.0, 48.0),
            shape: Shape::Ellipse,
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    bt_tmx::WriteRequest::for_template(&template, &path)
        .write()
        .unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&path);
    assert!(window.shows("Template Base"));
    assert!(!window.shows("This map has no tilesets"));
    window.focus("Name");
    window.press(Modifiers::COMMAND, Key::A);
    window.type_text("Golden Chest");
    window.press(Modifiers::NONE, Key::Enter);
    let doc = window.editor().current().unwrap();
    assert_eq!(
        window
            .editor()
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .name,
        "Golden Chest"
    );
    if let Some(path) = std::env::var_os("BT_TEMPLATE_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.apply(Intent::Run(ActionId::Save));
    assert_eq!(
        bt_tmx::Loader::default()
            .load_template(&path)
            .unwrap()
            .value
            .object
            .as_ref()
            .unwrap()
            .name,
        "Golden Chest"
    );
    window.apply(Intent::Run(ActionId::Undo));
    assert_eq!(
        window
            .editor()
            .template_of(doc)
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .name,
        "Chest"
    );
    window.exit();
}
