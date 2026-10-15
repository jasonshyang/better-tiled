//! Project creation, browsing and settings through the window; edits use project history.

use crate::test_window::TestWindow;
use bt_core::Value;
use bt_editor::{ActionId, FileRequest, Intent};
use bt_test_data::{Scratch, fixtures::MapBuilder};
use egui::{Key, Modifiers};
use glam::UVec2;

fn replace(window: &mut TestWindow, label: &str, value: &str) {
    window.focus(label);
    window.press(Modifiers::COMMAND, Key::A);
    window.type_text(value);
    window.press(Modifiers::NONE, Key::Enter);
}

#[test]
fn projects_create_browse_edit_cancel_undo_save_and_add_folders() {
    let scratch = Scratch::new("window-project-browser").unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
        .tile_layer("ground", ".")
        .build()
        .unwrap();
    for name in ["map2.tmx", "map10.tmx"] {
        bt_tmx::WriteRequest::for_map(&map, &scratch.join(name))
            .write()
            .unwrap();
    }
    std::fs::write(scratch.join("rules.txt"), "# No rules yet\n").unwrap();
    let mut window = TestWindow::with_gpu(&scratch.join("data"));
    window.apply(Intent::Run(ActionId::NewProject));
    let path = scratch.join("game.tiled-project");
    window.answer_file_dialog(FileRequest::NewProject, &path);
    let project = window.editor().project_asset().unwrap();
    window.click("map2.tmx");
    assert_eq!(
        window.editor().path_of(window.editor().current().unwrap()),
        Some(scratch.join("map2.tmx"))
    );
    window.click("Project");
    window.apply(Intent::Run(ActionId::ProjectProperties));
    replace(&mut window, "AutoMap Rules File", "discarded.txt");
    window.click("Cancel");
    assert!(
        window
            .editor()
            .store()
            .project(project)
            .unwrap()
            .automapping_rules_file
            .is_none()
    );
    window.apply(Intent::Run(ActionId::ProjectProperties));
    replace(&mut window, "AutoMap Rules File", "rules.txt");
    window.click("Add Property");
    window.type_text("biome");
    window.click("Add");
    replace(&mut window, "biome", "forest");
    if let Some(path) = std::env::var_os("BT_PROJECT_SCREENSHOT") {
        window.save_picture(std::path::Path::new(&path));
    }
    window.click("OK");
    assert_eq!(
        window.editor().store().project(project).unwrap().properties["biome"].value,
        Value::String("forest".into())
    );
    window.apply(Intent::Run(ActionId::Undo));
    assert!(
        window
            .editor()
            .store()
            .project(project)
            .unwrap()
            .properties
            .is_empty()
    );
    window.apply(Intent::Run(ActionId::Redo));
    window.apply(Intent::Run(ActionId::Save));
    let saved = bt_tmx::parse_project(&std::fs::read_to_string(&path).unwrap(), &path)
        .unwrap()
        .value;
    assert_eq!(
        saved.properties["biome"].value,
        Value::String("forest".into())
    );
    let extra = scratch.join("extra");
    std::fs::create_dir_all(&extra).unwrap();
    window.apply(Intent::Run(ActionId::AddProjectFolder));
    window.answer_file_dialog(FileRequest::ProjectFolder(project), &extra);
    assert_eq!(
        window
            .editor()
            .store()
            .project(project)
            .unwrap()
            .folders
            .len(),
        2
    );
    window.apply(Intent::RemoveProjectFolder {
        project,
        path: extra.clone(),
    });
    assert!(
        extra.exists(),
        "removing a browser root must keep its files"
    );
    window.apply(Intent::Run(ActionId::SaveAll));
    window.exit();
}

#[test]
fn a_project_draft_does_not_overwrite_a_newer_project_root() {
    let scratch = Scratch::new("window-project-stale-draft").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.apply(Intent::Run(ActionId::NewProject));
    window.answer_file_dialog(FileRequest::NewProject, &scratch.join("game.tiled-project"));
    let project = window.editor().project_asset().unwrap();
    let doc = window.editor().current().unwrap();
    window.apply(Intent::Run(ActionId::ProjectProperties));
    let mut value = (**window.editor().store().project(project).unwrap()).clone();
    value
        .properties
        .insert("external".into(), bt_core::Property::new(Value::Int(42)));
    window.apply(Intent::SetProject {
        doc,
        project,
        value: Box::new(value),
    });
    assert!(window.shows(
        "The project changed while this draft was open. Cancel and reopen Project Properties."
    ));
    window.press(Modifiers::NONE, Key::Enter);
    assert_eq!(
        window.editor().store().project(project).unwrap().properties["external"].value,
        Value::Int(42)
    );
    window.click("Cancel");
    window.apply(Intent::Run(ActionId::SaveAll));
    window.exit();
}
