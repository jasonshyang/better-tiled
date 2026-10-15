//! The CLI's observable Godot contract, including read-only checks and refusal exits.
use crate::folders::Folders;
use crate::{exit::Status, run::Run};
use bt_godot::Options;
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::UVec2;

#[test]
fn setup_export_check_status_replace_and_remember_share_the_exporter() {
    let scratch = Scratch::new("bt-godot-cli").unwrap();
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
    let run_command = |args: Vec<String>| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let run = Run::of_args(
            &args,
            Folders {
                locks: Some(locks.clone()),
                ..Folders::default()
            },
        );
        crate::run::run(&run).unwrap()
    };
    let saved = run_command(vec![
        "godot".into(),
        "setup".into(),
        path.display().to_string(),
        "--godot-project".into(),
        project.display().to_string(),
        "--level-scene".into(),
    ]);
    assert_eq!(saved.status, Status::Done);
    let invoke = |options: &[&str]| {
        run_command(
            [
                vec!["godot".into(), "export".into(), path.display().to_string()],
                options.iter().map(|s| s.to_string()).collect(),
            ]
            .concat(),
        )
    };
    assert_eq!(invoke(&["--check"]).status, Status::OutOfDate);
    let target = project.join("levels/map.tscn");
    assert!(!target.exists());
    let result = invoke(&["--json", "--remember"]);
    assert_eq!(result.status, Status::Done, "{}", result.text);
    let json: serde_json::Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(json["schema"], "bt godot export 1");
    let loaded = bt_tmx::Loader::builder().build().load_map(&path).unwrap();
    assert_eq!(loaded.value.file.export.format, "tscn");
    let library = bt_godot::export_map(
        &loaded.value,
        &path,
        &bt_core::CustomTypes::default(),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert!(library.up_to_date());
    assert_eq!(invoke(&["--check"]).status, Status::Done);
    assert!(project.join("levels/map_level.tscn").is_file());
    let edited = std::fs::read_to_string(&target)
        .unwrap()
        .replace("texture_filter = 1", "texture_filter = 2");
    std::fs::write(&target, &edited).unwrap();
    assert_eq!(invoke(&[]).status, Status::Refused);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), edited);
    let result = invoke(&["--replace"]);
    assert_eq!(result.status, Status::Done, "{}", result.text);
    assert!(target.with_extension("tscn.bak").is_file());
}
