//! `bt godot verify` (src/godot/verify.rs): finding Godot, reading what it prints, and what it
//! makes of an export (headless).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::super::verify::{self, Cell, Message, Step, Verdict};
use super::super::{Options, export};
use super::support::*;

#[test]
fn godot_is_looked_for_in_godot_then_the_app_then_the_path() {
    let dir = std::env::temp_dir().join(format!("bt-godot-find-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for folder in ["named", "app", "bin"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    let (named, app, on_path) = (
        dir.join("named/Godot_v4.7"),
        dir.join("app/Godot"),
        dir.join("bin")
            .join(if cfg!(windows) { "godot.exe" } else { "godot" }),
    );
    for file in [&named, &app, &on_path] {
        std::fs::write(file, "").unwrap();
    }
    let path = Some(std::env::join_paths([dir.join("nowhere"), dir.join("bin")]).unwrap());
    let none = dir.join("none");
    assert_eq!(
        verify::find(Some(named.clone().into()), &app, path.clone()),
        Some(named.clone())
    );
    assert_eq!(verify::find(None, &app, path.clone()), Some(app.clone()));
    assert_eq!(
        verify::find(Some(none.clone().into()), &none, path.clone()),
        Some(on_path.clone()),
        "GODOT naming nothing is passed over"
    );
    assert_eq!(verify::find(None, &none, None), None);
    assert!(
        verify::NO_GODOT.contains("set GODOT to its executable")
            && verify::NO_GODOT.contains("/Applications/Godot.app")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn what_godot_prints_on_stderr_becomes_messages() {
    let stderr = "ERROR: Corrupted tile map data: tiles might be missing.\n   at: set_tile_map_data_from_array (scene/2d/tile_map_layer.cpp:3244)\n   GDScript backtrace (most recent call first):\n       [0] _initialize (/tmp/verify.gd:8)\n\u{1b}[1;33mWARNING:\u{1b}[0;93m res://a.png: something\u{1b}[0m\nERROR: Corrupted tile map data: tiles might be missing.\n   at: set_tile_map_data_from_array (scene/2d/tile_map_layer.cpp:3244)\n\nSCRIPT ERROR: Parse Error: x\n          at: GDScript::reload (res://x.gd:3)\nhandle_crash: Program crashed with signal 11\n";
    let mut messages = Vec::new();
    verify::said(&mut messages, stderr, Step::Load);
    let m = |error: bool, text: &str, at: Option<&str>, times: usize| Message {
        step: Step::Load,
        error,
        text: text.to_owned(),
        at: at.map(str::to_owned),
        times,
    };
    assert_eq!(
        messages,
        [
            m(
                true,
                "Corrupted tile map data: tiles might be missing.",
                Some("set_tile_map_data_from_array (scene/2d/tile_map_layer.cpp:3244)"),
                2
            ),
            m(false, "res://a.png: something", None, 1),
            m(
                true,
                "Parse Error: x",
                Some("GDScript::reload (res://x.gd:3)"),
                1
            ),
            m(
                true,
                "handle_crash: Program crashed with signal 11",
                None,
                1
            ),
        ]
    );
    assert_eq!(
        verify::version("Godot Engine v4.7.2.stable.official.ed1daf0bf - https://godotengine.org")
            .as_deref(),
        Some("4.7.2")
    );
    assert_eq!(
        verify::version("Godot Engine v4.3.stable.official"),
        Some("4.3".to_owned())
    );
    assert_eq!(verify::version("[   0% ] first_scan_filesystem"), None);
}

#[test]
fn a_verdict_says_what_godot_found() {
    let mut verdict = Verdict {
        scene: PathBuf::from("/game/levels/level.tscn"),
        res: "res://levels/level.tscn".to_owned(),
        godot: PathBuf::from("/Applications/Godot.app/Contents/MacOS/Godot"),
        version: Some("4.7.2".to_owned()),
        messages: Vec::new(),
        loaded: true,
        layers: 2,
        cells: 1200,
        listed: Vec::new(),
        missing: 0,
    };
    assert!(verdict.clean());
    assert_eq!(
        verdict.text(),
        "res://levels/level.tscn: Godot 4.7.2 loads it cleanly (2 tile layers, 1,200 cells, each with its tile)\n"
    );
    assert_eq!(verdict.json()["result"], "clean");

    let cell = |x: i64, source: i64, atlas: [i64; 2], alternative: i64, lacks: &str| Cell {
        layer: "ground".to_owned(),
        cell: [x, -1],
        source,
        atlas,
        alternative,
        lacks: lacks.to_owned(),
    };
    verdict.listed = vec![
        cell(0, 0, [9, 9], 0, "tile"),
        cell(1, 5, [0, 0], 0, "source"),
        cell(2, 0, [1, 0], 4096 | 3, "alternative"),
    ];
    verdict.missing = 3;
    verdict.messages.push(Message {
        step: Step::Import,
        error: false,
        text: "res://images/a.png: big".to_owned(),
        at: Some("load (core/x.cpp:1)".to_owned()),
        times: 2,
    });
    assert!(!verdict.clean());
    assert_eq!(
        verdict.text(),
        "res://levels/level.tscn: Godot 4.7.2 finds 1 warning and 3 cells without tile data\n  warning importing the project: res://images/a.png: big (2 times)\n  ground: 3 cells without tile data, so Godot draws nothing there: (0, -1) uses tile (9, 9) of source 0, which it doesn't have; (1, -1) uses source 5, which the TileSet doesn't have; (2, -1) uses alternative 3 of tile (1, 0) in source 0, which it doesn't have\n"
    );
    let json = verdict.json();
    assert_eq!(
        (
            json["result"].as_str(),
            json["cells_without_tile_data"].as_u64(),
            json["warnings"][0]["times"].as_u64()
        ),
        (Some("problems"), Some(3), Some(2))
    );
    assert_eq!(
        json["listed_cells"][2],
        serde_json::json!({"layer": "ground", "cell": [2, -1], "source": 0, "atlas": [1, 0], "alternative": 4099, "lacks": "alternative"})
    );
}

/// Every file in the project but Godot's own (its `.godot/` folder and `.import` files).
fn files(dir: &Path) -> BTreeSet<PathBuf> {
    fn walk(dir: &Path, out: &mut BTreeSet<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.ends_with(".godot") {
                    walk(&path, out);
                }
            } else if path.extension().is_none_or(|e| e != "import") {
                out.insert(path);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, &mut out);
    out
}

/// Godot loads the desert export with nothing to say, and a tile for every cell; verifying
/// leaves nothing of its own in the project.
#[test]
fn godot_finds_nothing_wrong_with_the_desert_export() {
    let Some(godot) = godot() else { return };
    let Some(path) = bt_test_data::test_data("reference/tiled/examples/desert.tmx") else {
        return;
    };
    let dir = project("verify-desert");
    let scene = dir.join("levels/desert.tscn");
    export(
        &path,
        &mut bt_tmx::Loader::builder().build(),
        &scene,
        &Options::default(),
    )
    .unwrap();
    let map = bt_tmx::Loader::builder()
        .build()
        .load_map(&path)
        .unwrap()
        .value;
    let mut painted = 0;
    map.for_each_layer(|layer, _| {
        painted += layer
            .as_tile_layer()
            .map_or(0, |t| t.cells().filter(|p| !p.cell.is_empty()).count())
    });

    let before = files(&dir);
    let verdict = verify::verify(&godot, &scene).unwrap();
    assert!(verdict.clean(), "{}", verdict.text());
    assert_eq!(
        (
            verdict.loaded,
            verdict.layers,
            verdict.cells,
            verdict.missing
        ),
        (true, 1, painted, 0)
    );
    assert!(
        verdict
            .version
            .as_deref()
            .is_some_and(|v| v.starts_with("4.")),
        "{:?}",
        verdict.version
    );
    let text = verdict.text();
    assert!(
        text.starts_with("res://levels/desert.tscn: Godot 4.")
            && text.contains(" loads it cleanly (1 tile layer, ")
            && text.lines().count() == 1,
        "{text}"
    );
    assert_eq!(
        files(&dir),
        before,
        "nothing of better-tiled's is left in the project"
    );

    // A file that isn't a scene Godot can read: said, not hung on.
    std::fs::write(
        dir.join("levels/broken.tscn"),
        "[gd_scene format=3]\n\n[node name=\"x\" type=\"NoSuchClass\" parent=\"nowhere\"]\n",
    )
    .unwrap();
    let verdict = verify::verify(&godot, &dir.join("levels/broken.tscn")).unwrap();
    assert!(!verdict.loaded && verdict.errors() == 2, "{verdict:?}");
    assert!(verdict.text().contains("\n  error loading the scene: Invalid scene: root node x cannot specify a parent node.\n  error: Godot 4."), "{}", verdict.text());
    // Not a scene at all, or not in a project: it can't be checked.
    assert!(
        verify::verify(&godot, &dir.join("maps/tiles.png"))
            .unwrap_err()
            .contains("ends in .tscn")
    );
    assert!(
        verify::verify(&godot, &dir.join("levels/gone.tscn"))
            .unwrap_err()
            .starts_with("there is no scene ")
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The scene is in the tree for a few physics frames, as in a game, so what Godot says only then
/// is heard: here, that it can't build the merged collision of a physics quadrant holding mirrored
/// tall tiles, once the export's physics_quadrant_size is taken out.
#[test]
fn godot_is_heard_while_the_scene_is_in_the_tree() {
    let Some(godot) = godot() else { return };
    let dir = project("verify-quadrants");
    let path = super::layers::mirrored_tall_tiles(&dir);
    let scene = dir.join("levels/mirrored.tscn");
    export(&path, &mut loader(&dir), &scene, &Options::default()).unwrap();
    let verdict = verify::verify(&godot, &scene).unwrap();
    assert!(verdict.clean(), "{}", verdict.text());

    let tscn = std::fs::read_to_string(&scene).unwrap();
    assert!(tscn.contains("\nphysics_quadrant_size = 1\n"), "{tscn}");
    std::fs::write(&scene, tscn.replace("\nphysics_quadrant_size = 1\n", "\n")).unwrap();
    let verdict = verify::verify(&godot, &scene).unwrap();
    assert!(verdict.loaded && !verdict.clean(), "{}", verdict.text());
    assert_eq!(
        verdict
            .messages
            .iter()
            .map(|m| (m.step, m.error, m.text.as_str()))
            .collect::<Vec<_>>(),
        [(Step::Load, true, "Convex decomposing failed!")],
        "{}",
        verdict.text()
    );
    std::fs::remove_dir_all(&dir).ok();
}
