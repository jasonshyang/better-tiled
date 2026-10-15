//! What the editor's front door builds on (docs/design/12-godot-export.md): the Godot setup a map's
//! better-tiled.json gives, the starter level scene, and the project scan's cache.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use super::super::project_scan;
use super::super::setup::{self, Setup};
use super::super::starter;
use super::super::{Options, export};
use super::support::*;

#[test]
fn a_setup_names_the_scene_and_where_copies_go() {
    let dir = project("setup");
    let map = dir.join("maps/features.tmx");
    assert!(Setup::of(&map).unwrap().is_none(), "no project file");
    std::fs::write(
        dir.join("better-tiled.json"),
        r#"{"betterTiled": 1, "godot": {"images": "res://art/copies/"}}"#,
    )
    .unwrap();
    assert!(
        Setup::of(&map).unwrap().is_none(),
        "a setup names its Godot project"
    );
    std::fs::write(dir.join("better-tiled.json"), r#"{"betterTiled": 1, "godot": {"project": ".", "levels": "res://world/", "images": "res://art/copies/", "levelScene": true}}"#).unwrap();
    let found = Setup::of(&map).unwrap();
    assert_eq!(
        found.as_ref().unwrap().scene(&map),
        Some(dir.join("world/features.tscn"))
    );
    assert_eq!(
        found.as_ref().unwrap().images(),
        Some(dir.join("art/copies"))
    );
    assert!(found.as_ref().unwrap().level_scene());
    assert_eq!(setup::in_project(&dir, "res://"), Some(dir.clone()));
    assert_eq!(
        setup::in_project(&dir, "res://../up"),
        None,
        "never outside the project"
    );
    assert_eq!(setup::in_project(&dir, "levels/"), None);
    assert_eq!(setup::find_project(&map), Some(dir.clone()));

    // The copies of images from outside the project go to the setup's folder.
    let outside = std::env::temp_dir().join(format!("bt-godot-setup-art-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::copy(dir.join("maps/tiles.png"), outside.join("far.png")).unwrap();
    // And a collection of three sprites from outside the project, of which one object shows one.
    for sprite in ["a.png", "b.png", "c.png"] {
        std::fs::copy(dir.join("maps/tiles.png"), outside.join(sprite)).unwrap();
    }
    let sprites = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.11.2" name="sprites" tilewidth="64" tileheight="64" tilecount="3" columns="0">
 <grid orientation="orthogonal" width="1" height="1"/>
 <tile id="0"><image source="a.png" width="64" height="64"/></tile>
 <tile id="1"><image source="b.png" width="64" height="64"/></tile>
 <tile id="2"><image source="c.png" width="64" height="64"/></tile>
</tileset>
"#;
    std::fs::write(outside.join("sprites.tsx"), sprites).unwrap();
    let text = std::fs::read_to_string(dir.join("maps/level.tmx"))
        .unwrap()
        .replace(
            r#"<imagelayer id="4" name="sky" parallaxx="0.5">
  <image source="tiles.png""#,
            &format!(
                r#"<imagelayer id="4" name="sky" parallaxx="0.5">
  <image source="{}""#,
                outside.join("far.png").display()
            ),
        );
    let text = text.replace(
        r#"<tileset firstgid="1" source="tiles.tsx"/>"#,
        &format!(
            r#"<tileset firstgid="1" source="tiles.tsx"/>
 <tileset firstgid="100" source="{}"/>"#,
            outside.join("sprites.tsx").display()
        ),
    );
    let text = text.replace(
        "</map>",
        r#" <objectgroup id="90" name="sprites">
  <object id="900" gid="101" x="0" y="64" width="64" height="64"/>
 </objectgroup>
</map>"#,
    );
    std::fs::write(dir.join("maps/far.tmx"), text).unwrap();
    let map = dir.join("maps/far.tmx");
    // The export copies far.png and b.png: tiles.png is in the project, and the sprites no object
    // shows aren't copied (the setup sheet and bt godot setup counted all three, saying 4).
    assert_eq!(
        super::super::images_copied(
            &bt_tmx::Loader::builder()
                .build()
                .load_map(&map)
                .unwrap()
                .value,
            &map,
            &bt_core::CustomTypes::default(),
            &dir.join("world/far.tscn")
        )
        .unwrap(),
        2
    );
    export(
        &map,
        &mut loader(&dir),
        &dir.join("world/far.tscn"),
        &Options::default(),
    )
    .unwrap();
    fn files(dir: &Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                files(&entry.path(), found);
            } else if entry.file_name() != ".sources.json" {
                found.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    let mut copies = Vec::new();
    files(&dir.join("art/copies"), &mut copies);
    copies.sort();
    assert_eq!(copies, ["b.png", "far.png"]);
    let tscn = std::fs::read_to_string(dir.join("world/far.tscn")).unwrap();
    assert!(
        tscn.contains("path=\"res://art/copies/bt-godot-setup-art-"),
        "{tscn}"
    );
    assert!(dir.join("art/copies/.sources.json").is_file() && !dir.join("world/images").exists());
    std::fs::remove_dir_all(&outside).ok();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_starter_scene_is_written_once() {
    let dir = project("starter-once");
    let scene = dir.join("levels/features.tscn");
    export(
        &dir.join("maps/features.tmx"),
        &mut loader(&dir),
        &scene,
        &Options::default(),
    )
    .unwrap();
    let path = starter::write(&scene).unwrap().expect("written");
    assert_eq!(path, dir.join("levels/features_level.tscn"));
    let text = std::fs::read_to_string(&path).unwrap();
    let uid = super::super::read::parse_header(&std::fs::read_to_string(&scene).unwrap())
        .unwrap()
        .uid()
        .unwrap();
    assert!(
        text.starts_with("; A level scene for features.tscn, written once by better-tiled"),
        "{text}"
    );
    assert!(text.contains(&format!("[ext_resource type=\"PackedScene\" uid=\"{uid}\" path=\"res://levels/features.tscn\" id=\"1\"]")), "{text}");
    assert!(
        text.ends_with("[node name=\"features\" instance=ExtResource(\"1\")]\n"),
        "{text}"
    );
    let own = super::super::read::parse(&text).unwrap().uid().unwrap();
    assert_ne!(own, uid, "a uid of its own");
    // The scan sees it as the user's, not a generated file.
    let scan = project_scan::scan(&dir);
    assert!(scan.entries.iter().any(|e| e.path == path && !e.generated));

    assert_eq!(starter::write(&scene).unwrap(), None, "already written");
    std::fs::write(&path, "mine").unwrap();
    let taken = starter::write(&scene).unwrap_err();
    assert_eq!(
        taken,
        "res://levels/features_level.tscn is already there, a file of its own, not a scene that inherits features.tscn"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "mine",
        "never over a file"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A file the scan read before is read again only when its size or time changed.
#[test]
fn the_scan_reads_again_only_what_changed() {
    let dir = project("scan-cache");
    let file = dir.join("thing.gd.uid");
    let past = SystemTime::now() - Duration::from_secs(60);
    let write = |text: &str, time: SystemTime| {
        std::fs::write(&file, text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };
    let cache = std::cell::RefCell::new(project_scan::ScanCache::default());
    let uid = |dir: &Path| {
        cache
            .borrow_mut()
            .scan(dir)
            .entries
            .iter()
            .find(|e| e.path == dir.join("thing.gd"))
            .and_then(|e| e.uid)
    };
    write("uid://cyncsb\n", past);
    let first = uid(&dir).unwrap();
    // The same size and time: what was read before (a file isn't read again on every save).
    write("uid://cyncsc\n", past);
    assert_eq!(uid(&dir), Some(first));
    write("uid://cyncsc\n", past + Duration::from_secs(1));
    assert_ne!(uid(&dir), Some(first), "a new time is read again");
    // Just written: not kept, since the time may not have moved yet on a coarse file system.
    write("uid://cyncsd\n", SystemTime::now());
    let fresh = uid(&dir);
    write("uid://cyncse\n", SystemTime::now());
    assert_ne!(uid(&dir), fresh);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(uid(&dir), None);
    std::fs::remove_dir_all(&dir).ok();
}

/// Godot loads the starter as a scene inheriting the export, with all of its nodes.
#[test]
fn godot_loads_the_starter_as_an_inheritor_of_the_export() {
    let Some(godot) = godot() else { return };
    let dir = project("starter-godot");
    let scene = dir.join("levels/features.tscn");
    export(
        &dir.join("maps/features.tmx"),
        &mut loader(&dir),
        &scene,
        &Options::default(),
    )
    .unwrap();
    starter::write(&scene).unwrap().unwrap();
    let (_, import) = headless(&godot, &dir, &["--import"]);
    assert!(!import.contains("ERROR"), "{import}");
    std::fs::write(dir.join("check.gd"), CHECK).unwrap();
    let (stdout, stderr) = headless(&godot, &dir, &["--script", "res://check.gd"]);
    assert_eq!(stderr.trim(), "", "loads cleanly");
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| panic!("{stdout}"));
    let result: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(result["base"], "res://levels/features.tscn", "{result}");
    assert_eq!(
        (result["root"].as_str(), result["file"].as_str()),
        (Some("features"), Some("res://levels/features_level.tscn")),
        "{result}"
    );
    assert_eq!(result["children"], result["base_children"], "{result}");
    assert!(result["children"].as_i64().unwrap() > 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// What the starter is to Godot: whose state it inherits, and what an instance of it holds. Every
/// step is checked first, since a script error would leave Godot running.
const CHECK: &str = r#"extends SceneTree

func _init() -> void:
	var out := {}
	var packed := load("res://levels/features_level.tscn") as PackedScene
	var base := load("res://levels/features.tscn") as PackedScene
	if packed and base:
		var state := packed.get_state().get_base_scene_state()
		out["base"] = state.get_path() if state else ""
		var level := packed.instantiate()
		var plain := base.instantiate()
		if level and plain:
			out["root"] = String(level.name)
			out["file"] = level.scene_file_path
			out["children"] = level.get_child_count()
			out["base_children"] = plain.get_child_count()
			level.free()
			plain.free()
	print("BT-JSON-BEGIN", JSON.stringify(out), "BT-JSON-END")
	quit()
"#;

/// Runs Godot headless in `project`: (stdout, stderr). Stopped after two minutes.
fn headless(godot: &Path, project: &Path, args: &[&str]) -> (String, String) {
    let mut child = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(project)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = |mut from: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            from.read_to_string(&mut text).ok();
            text
        })
    };
    let (stdout, stderr) = (
        pipe(Box::new(child.stdout.take().unwrap())),
        pipe(Box::new(child.stderr.take().unwrap())),
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("Godot didn't finish {args:?}:\n{}", stdout.join().unwrap());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (stdout.join().unwrap(), stderr.join().unwrap())
}
