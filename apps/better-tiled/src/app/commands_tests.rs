//! Edit Commands drafts, project persistence and command dispatch through the real window.

#![cfg(unix)]

use crate::test_window::TestWindow;
use bt_editor::{ActionId, Intent};
use bt_test_data::{Scratch, fixtures::MapBuilder};
use glam::UVec2;

fn replace(window: &mut TestWindow, label: &str, value: &str) {
    window.focus(label);
    window.press(egui::Modifiers::COMMAND, egui::Key::A);
    window.type_text(value);
}

#[test]
fn command_drafts_cancel_apply_undo_and_run_from_their_shortcuts() {
    let scratch = Scratch::new("window-commands").unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
        .tile_layer("ground", ".")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.drop_file(&path);
    window.apply(Intent::Run(ActionId::EditCommands));
    window.click("Add Command");
    replace(&mut window, "Name", "Discarded");
    window.click("Cancel");
    assert!(window.editor().project_commands().is_empty());
    window.apply(Intent::Run(ActionId::EditCommands));
    window.click("Add Command");
    replace(&mut window, "Name", "Print Map");
    replace(&mut window, "Executable", "/bin/echo");
    replace(&mut window, "Arguments", "%mapfile");
    replace(&mut window, "Shortcut", "Ctrl+Alt+F8");
    if let Some(path) = std::env::var_os("BT_COMMANDS_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click("OK");
    assert_eq!(window.editor().project_commands().len(), 1);
    window.apply(Intent::Run(ActionId::Undo));
    assert!(window.editor().project_commands().is_empty());
    window.apply(Intent::Run(ActionId::Redo));
    window.click_holding("Layers", egui::Modifiers::NONE);
    window.press(
        egui::Modifiers::COMMAND | egui::Modifiers::ALT,
        egui::Key::F8,
    );
    assert!(window.shows("Console"));
    assert!(window.shows("Exited 0"));
    assert!(window.shows(&format!("{}\n", path.display())));
    window.click("Hide Console");
    window.apply(Intent::Run(ActionId::ShowConsole));
    assert!(window.shows("Exited 0"));
    window.exit();
}
