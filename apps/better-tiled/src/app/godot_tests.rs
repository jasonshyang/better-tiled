//! Export, remember/undo and automatic refusal through the window's real action dispatch.
use crate::test_window::TestWindow;
use bt_editor::{ActionId, Intent};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::UVec2;

#[test]
fn export_uses_the_shown_map_remember_is_undoable_and_save_refusals_stay_in_issues() {
    let scratch = Scratch::new("window-godot").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .tileset(atlas.tileset)
        .tile_layer("ground", "0 1")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let project = scratch.join("game");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("project.godot"), "config_version=5\n").unwrap();
    let locks = scratch.join("locks");
    std::fs::create_dir_all(&locks).unwrap();
    bt_godot::setup::write(
        &scratch.join("better-tiled.json"),
        bt_project::GodotSetup {
            project: Some("game".into()),
            export_on_save: Some(true),
            ..bt_project::GodotSetup::default()
        },
        &locks,
    )
    .unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    window.apply(Intent::Run(ActionId::Export));
    let target = project.join("levels/map.tscn");
    assert!(target.is_file());
    assert_eq!(
        window.editor().map_of(doc).unwrap().file.export.format,
        "tscn"
    );
    window.apply(Intent::Run(ActionId::Undo));
    assert_eq!(window.editor().map_of(doc).unwrap().file.export.format, "");
    window.apply(Intent::Run(ActionId::Redo));
    assert_eq!(
        window.editor().map_of(doc).unwrap().file.export.format,
        "tscn"
    );
    let report = bt_godot::export_map(
        window.editor().shown_map(doc).unwrap(),
        &path,
        window.editor().custom_types(),
        &target,
        &bt_godot::Options::default(),
    )
    .unwrap();
    assert!(
        report.up_to_date(),
        "The window writes the shared exporter's bytes"
    );
    let edited = std::fs::read_to_string(&target)
        .unwrap()
        .replace("texture_filter = 1", "texture_filter = 2");
    std::fs::write(&target, &edited).unwrap();
    window.apply(Intent::Run(ActionId::Save));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), edited);
    assert!(
        !window.editor().has_unsaved(),
        "the map save succeeded despite the refused export"
    );
    assert!(
        !window.has_modal(),
        "an automatic export does not ask a question"
    );
    assert!(window.shows("1 issue"));
    window.click("1 issue");
    assert!(window.shows("Issues"));
    window.pause(12.0);
    assert!(
        window.shows("1 issue"),
        "the issue survives the status flash"
    );
}

#[test]
fn export_as_uses_the_system_file_request_and_can_be_repeated() {
    let scratch = Scratch::new("window-godot-export-as").unwrap();
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .tile_layer("ground", ". .")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    std::fs::write(scratch.join("project.godot"), "config_version=5\n").unwrap();
    let mut window = TestWindow::with_data(&scratch.join("data"));
    window.drop_file(&path);
    let doc = window.editor().current().unwrap();
    window.apply(Intent::Run(ActionId::ExportAs));
    let source = window.editor().path_of(doc).unwrap();
    let target = scratch.join("chosen.tscn");
    window.answer_file_dialog(
        bt_editor::FileRequest::GodotScene {
            doc,
            suggested: source.with_extension("tscn"),
        },
        &target,
    );
    assert!(target.is_file());
    let before = std::fs::metadata(&target).unwrap().modified().unwrap();
    window.apply(Intent::Run(ActionId::Export));
    assert_eq!(
        std::fs::metadata(&target).unwrap().modified().unwrap(),
        before
    );
}
