//! The TileSet (docs/design/12-godot-export.md): every tile, source ids and names, probabilities, and animated
//! tiles in sources of their own, laid out the way Godot steps through them.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use bt_core::{Cell, CustomTypes, LayerKind, TilesetSlot};
use glam::IVec2;
use serde_json::{Value, json};

use super::super::animation::{Layout, pack, solve};
use super::super::report::Element;
use super::super::{Options, export, export_map, read};
use super::support::*;

/// Tile ids in notes are runs, and many are cut short: the notes say how many tiles there are,
/// and one listed 428 tiles, run after run.
#[test]
fn tile_ids_in_notes_are_runs_cut_short() {
    use super::super::tileset::id_runs;
    assert_eq!(id_runs(&[37]), "37");
    assert_eq!(id_runs(&[37, 41, 42, 43]), "37, 41–43");
    let many: Vec<u32> = (0..40).map(|i| i * 3).collect();
    assert_eq!(id_runs(&many), "0, 3, 6, 9, 12, 15, …");
    let six: Vec<u32> = (0..6).map(|i| i * 3).collect();
    assert_eq!(
        id_runs(&six),
        "0, 3, 6, 9, 12, 15",
        "six runs are all listed"
    );
}

/// Cells of a 4-column atlas, by tile id.
fn at(ids: &[u32]) -> Vec<IVec2> {
    ids.iter()
        .map(|&id| IVec2::new((id % 4) as i32, (id / 4) as i32))
        .collect()
}

#[test]
fn animations_are_laid_out_the_way_godot_steps_through_them() {
    let layout = |columns: i32, x: i32, y: i32| {
        Some(Layout {
            columns,
            separation: IVec2::new(x, y),
        })
    };
    assert_eq!(solve(&at(&[5])), layout(0, 0, 0), "one frame");
    assert_eq!(solve(&at(&[1, 2, 3])), layout(0, 0, 0), "a run along a row");
    // rpg/island's water, on its 36-column atlas.
    let island: Vec<IVec2> = [37, 46, 55, 64]
        .iter()
        .map(|id| IVec2::new(id % 36, id / 36))
        .collect();
    assert_eq!(solve(&island), layout(0, 8, 0), "a stride of 9 along a row");
    assert_eq!(solve(&at(&[2, 6, 10])), layout(1, 0, 0), "down a column");
    assert_eq!(solve(&at(&[1, 9])), layout(1, 0, 1), "every other row");
    assert_eq!(
        solve(&at(&[0, 1, 2, 3, 4, 5, 6])),
        layout(4, 0, 0),
        "a run from column 0 wrapping whole rows"
    );
    assert_eq!(solve(&at(&[5, 6, 9, 10])), layout(2, 0, 0), "a block");
    assert_eq!(
        solve(&at(&[0, 2, 8, 10])),
        layout(2, 1, 1),
        "a block of every other cell"
    );
    for (frames, why) in [
        (&[10, 3][..], "anywhere"),
        (&[2, 3, 4], "a run wrapping from a column after 0"),
        (&[0, 1, 0], "back and forth"),
        (&[15, 14], "backwards"),
        (&[1, 3, 4], "uneven"),
    ] {
        assert_eq!(solve(&at(frames)), None, "{why}");
    }
    // Each solved layout puts every frame where it is.
    for frames in [
        &[1, 2, 3][..],
        &[2, 6, 10],
        &[0, 1, 2, 3, 4, 5, 6],
        &[0, 2, 8, 10],
    ] {
        let cells = at(frames);
        let layout = solve(&cells).unwrap();
        assert!(
            cells
                .iter()
                .enumerate()
                .all(|(i, c)| layout.cell(cells[0], i) == *c),
            "{frames:?}"
        );
    }

    assert_eq!(
        pack(&[at(&[0, 1, 2]), at(&[1, 2])], &[]),
        [0, 1],
        "overlapping animations go in two sources"
    );
    assert_eq!(
        pack(&[at(&[4, 6]), at(&[5, 7]), at(&[8, 12])], &[]),
        [0, 0, 0],
        "interleaved ones share one"
    );
    assert_eq!(
        pack(&[at(&[0, 1]), at(&[1, 2]), at(&[3]), at(&[2, 3])], &[]),
        [0, 1, 0, 2]
    );
    // Where they were first, when they fit there; the others then fill in.
    assert_eq!(
        pack(
            &[at(&[0, 1]), at(&[1, 2]), at(&[2, 3])],
            &[None, Some(0), Some(1)]
        ),
        [1, 0, 1],
        "a new animation doesn't move the others"
    );
    assert_eq!(
        pack(&[at(&[0, 1]), at(&[1, 2])], &[Some(2), Some(2)]),
        [2, 0],
        "the second no longer fits where it was"
    );
}

/// A 4×4 atlas of animations: a run along a row, one overlapping it, two interleaved strides, a
/// column, two Godot can't lay out (the second showing another tile first), and a tile with a
/// probability.
const PROBE_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="probe" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="0">
  <animation>
   <frame tileid="0" duration="100"/>
   <frame tileid="1" duration="100"/>
   <frame tileid="2" duration="100"/>
  </animation>
 </tile>
 <tile id="1">
  <animation>
   <frame tileid="1" duration="200"/>
   <frame tileid="2" duration="200"/>
  </animation>
 </tile>
 <tile id="4">
  <animation>
   <frame tileid="4" duration="100"/>
   <frame tileid="6" duration="100"/>
  </animation>
 </tile>
 <tile id="5">
  <animation>
   <frame tileid="5" duration="100"/>
   <frame tileid="7" duration="300"/>
  </animation>
 </tile>
 <tile id="8">
  <animation>
   <frame tileid="8" duration="100"/>
   <frame tileid="12" duration="100"/>
  </animation>
 </tile>
 <tile id="9">
  <animation>
   <frame tileid="9" duration="100"/>
   <frame tileid="13" duration="100"/>
   <frame tileid="10" duration="100"/>
  </animation>
 </tile>
 <tile id="14">
  <animation>
   <frame tileid="15" duration="100"/>
   <frame tileid="14" duration="100"/>
  </animation>
 </tile>
 <tile id="15" probability="0.25"/>
</tileset>
"#;

/// The probe atlas painted with each of its animated tiles, a frame tile and the tile with a
/// probability; an atlas tileset it doesn't paint; an image collection it paints, with two
/// animated tiles (one with frames of two sizes, one with frames in a row of one image); and one
/// it doesn't paint.
const PROBE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="8" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="tiles.tsx"/>
 <tileset firstgid="5" source="probe.tsx"/>
 <tileset firstgid="21" name="things" tilewidth="32" tileheight="32" tilecount="5" columns="0">
  <grid orientation="orthogonal" width="1" height="1"/>
  <tile id="0">
   <image source="tiles.png" width="64" height="64"/>
   <animation>
    <frame tileid="3" duration="100"/>
    <frame tileid="0" duration="100"/>
   </animation>
  </tile>
  <tile id="3" x="32" y="0" width="32" height="32">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
  <tile id="4" x="0" y="32" width="16" height="16">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
  <tile id="5" x="32" y="32" width="16" height="16">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
  <tile id="6" x="0" y="32" width="16" height="16">
   <image source="tiles.png" width="64" height="64"/>
   <animation>
    <frame tileid="4" duration="100"/>
    <frame tileid="5" duration="200"/>
   </animation>
  </tile>
 </tileset>
 <tileset firstgid="28" name="unused" tilewidth="64" tileheight="64" tilecount="1" columns="0">
  <grid orientation="orthogonal" width="1" height="1"/>
  <tile id="0">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
 </tileset>
 <layer id="1" name="cells" width="8" height="2">
  <data encoding="csv">
5,6,7,9,10,13,14,19,
20,21,24,27,0,0,0,0
</data>
 </layer>
</map>
"#;

fn probe(name: &str) -> std::path::PathBuf {
    let dir = project(name);
    std::fs::write(dir.join("maps/probe.tsx"), PROBE_TSX).unwrap();
    std::fs::write(dir.join("maps/probe.tmx"), PROBE).unwrap();
    dir
}

/// The TileSet's atlas sources: each one's name and its other lines, in order.
fn atlas_sources(tscn: &str) -> Vec<(String, Vec<(String, String)>)> {
    let resource = read::parse(tscn).unwrap();
    resource
        .sub_resources()
        .filter(|(sub, _)| sub.class == "TileSetAtlasSource")
        .map(|(_, section)| {
            let name = read::string(section.get("resource_name").unwrap()).unwrap();
            (
                name,
                section
                    .props
                    .iter()
                    .filter(|(k, _)| k != "resource_name")
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn the_tileset_holds_every_tile_with_animated_tiles_in_sources_of_their_own() {
    let dir = probe("tileset");
    let target = dir.join("probe.tscn");
    let report = export(
        &dir.join("maps/probe.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let sources = atlas_sources(&tscn);
    let names: Vec<&str> = sources.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "maps/tiles.tsx",
            "maps/probe.tsx",
            "maps/probe.tsx#anim1",
            "maps/probe.tsx#anim2",
            "maps/probe.tmx::things#0",
            "maps/probe.tmx::things#3",
            "maps/probe.tmx::things#4",
            "maps/probe.tmx::things#5",
            "maps/probe.tmx::things#6",
        ],
        "in tileset order, each atlas's animation sources after it, an image collection by tile id; the collection nothing paints is left out"
    );
    assert!(
        tscn.contains("sources/8 = SubResource(\"TileSetAtlasSource_8\")\n")
            && !tscn.contains("sources/9")
    );
    let tiles = |source: usize| {
        sources[source]
            .1
            .iter()
            .filter(|(k, _)| k.split('/').count() == 2 && k.ends_with("/0"))
            .count()
    };
    assert_eq!(
        (0..9).map(tiles).collect::<Vec<_>>(),
        [4, 16, 6, 1, 1, 1, 1, 1, 1],
        "every tile, painted or not"
    );
    assert_eq!((report.counts.tiles, report.counts.sources), (32, 9));

    // Cells: animated tiles at their first frame's cell in an animation source, frame tiles and
    // others in their tileset's source.
    let s = sections(&tscn);
    assert_eq!(
        cells(value(node(&s, "cells", "."), "tile_map_data")),
        [
            [0, 0, 2, 0, 0, 0],
            [1, 0, 3, 1, 0, 0],
            [2, 0, 1, 2, 0, 0],
            [3, 0, 2, 0, 1, 0],
            [4, 0, 2, 1, 1, 0],
            [5, 0, 2, 0, 2, 0],
            [6, 0, 2, 1, 2, 0],
            [7, 0, 2, 3, 3, 0],
            [0, 1, 1, 3, 3, 0],
            [1, 1, 4, 0, 0, 0],
            [2, 1, 5, 0, 0, 0],
            [3, 1, 8, 0, 0, 0],
        ]
    );

    // The layout before the durations, which Godot applies as it reads them; no frame count.
    let lines = |source: usize| {
        sources[source]
            .1
            .iter()
            .filter(|(k, _)| k.starts_with(|c: char| c.is_ascii_digit()))
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lines(2),
        [
            "0:0/animation_frame_0/duration = 0.1",
            "0:0/animation_frame_1/duration = 0.1",
            "0:0/animation_frame_2/duration = 0.1",
            "0:0/0 = 0",
            "0:1/animation_separation = Vector2i(1, 0)",
            "0:1/animation_frame_0/duration = 0.1",
            "0:1/animation_frame_1/duration = 0.1",
            "0:1/0 = 0",
            "0:2/animation_columns = 1",
            "0:2/animation_frame_0/duration = 0.1",
            "0:2/animation_frame_1/duration = 0.1",
            "0:2/0 = 0",
            "1:1/animation_separation = Vector2i(1, 0)",
            "1:1/animation_frame_0/duration = 0.1",
            "1:1/animation_frame_1/duration = 0.3",
            "1:1/0 = 0",
            "1:2/0 = 0",
            "3:3/0 = 0",
        ]
    );
    assert_eq!(
        lines(3),
        [
            "1:0/animation_frame_0/duration = 0.2",
            "1:0/animation_frame_1/duration = 0.2",
            "1:0/0 = 0"
        ]
    );
    assert!(
        !tscn.contains("animation_frames_count") && !tscn.contains("/0/animation"),
        "{tscn}"
    );
    assert!(
        lines(1).contains(&"3:3/0/probability = 0.25".to_owned())
            && lines(1).iter().all(|l| !l.contains("animation"))
    );

    // An image collection's animated tiles: with frames of two sizes, the first frame (tile 3's
    // part of the image); with frames in a row of one image, that row.
    let grid = |source: usize| {
        sources[source]
            .1
            .iter()
            .filter(|(k, _)| matches!(k.as_str(), "margins" | "separation" | "texture_region_size"))
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(grid(4), ["Vector2i(32, 0)", "Vector2i(32, 32)"]);
    assert_eq!(grid(5), grid(4));
    assert_eq!(
        grid(8),
        ["Vector2i(0, 32)", "Vector2i(16, 0)", "Vector2i(16, 16)"]
    );
    assert_eq!(
        lines(8),
        [
            "0:0/animation_frame_0/duration = 0.1",
            "0:0/animation_frame_1/duration = 0.2",
            "0:0/0 = 0"
        ]
    );

    let notes: Vec<_> = report
        .notes
        .iter()
        .filter(|n| n.code.as_str() == "godot/animation-layout")
        .collect();
    assert_eq!(notes.len(), 2, "{:?}", report.warnings());
    assert_eq!(
        notes[0].message,
        "tileset probe: Godot shows the first frame of 2 animated tiles (9, 14): it animates only frames evenly spaced along a row or column of the image, or filling whole rows"
    );
    assert_eq!(
        notes[0].elements,
        [
            Element::Tile { tileset: 1, id: 9 },
            Element::Tile { tileset: 1, id: 14 }
        ]
    );
    assert_eq!(
        notes[1].message,
        "tileset things: Godot shows the first frame of 1 animated tile (0): it animates only frames of one size evenly spaced in one image"
    );
    assert_eq!(report.warnings().len(), 2, "{:?}", report.warnings());

    // The same bytes again.
    export(
        &dir.join("maps/probe.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), tscn);
    std::fs::remove_dir_all(&dir).ok();
}

/// Tilesets embedded in the map are named after it; `#` in a name can't be taken for a suffix.
#[test]
fn sources_are_named_after_their_tilesets() {
    let dir = project("source-names");
    let map = PROBE.replace(r#"name="things""#, r##"name="#1 things""##);
    std::fs::write(dir.join("maps/probe.tsx"), PROBE_TSX).unwrap();
    std::fs::write(dir.join("maps/probe.tmx"), map).unwrap();
    let target = dir.join("probe.tscn");
    export(
        &dir.join("maps/probe.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    let names: Vec<String> = atlas_sources(&std::fs::read_to_string(&target).unwrap())
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        names[4..6],
        [
            "maps/probe.tmx::%231 things#0",
            "maps/probe.tmx::%231 things#3"
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn desert_has_all_48_tiles() {
    let Some(map) = bt_test_data::test_data("reference/tiled/examples/desert.tmx") else {
        return;
    };
    let dir = project("desert");
    let target = dir.join("desert.tscn");
    let report = export(
        &map,
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        (report.counts.tiles, report.counts.sources),
        (48, 1),
        "the map paints 40 of them"
    );
    let tscn = std::fs::read_to_string(&target).unwrap();
    assert_eq!(tscn.lines().filter(|l| l.ends_with("/0 = 0")).count(), 48);
    assert!(
        tscn.contains("resource_name = \"") && tscn.contains("desert.tsx\"\n"),
        "named after its file"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Loads a scene in Godot (headless) and describes its tiles: how many cells each layer paints
/// and how many of them have no tile data; with `tiles`, every source's tiles and each frame's
/// region of the image; with `cells`, every cell.
const TILES: &str = r#"extends SceneTree

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	var result := {}
	var scene = load(args[0])
	if scene:
		var room: Node = (scene as PackedScene).instantiate()
		var tileset: TileSet = null
		var total := 0
		var missing := 0
		var cells := []
		for node: Node in room.find_children("*", "TileMapLayer", true, false):
			var layer := node as TileMapLayer
			tileset = layer.tile_set
			for c: Vector2i in layer.get_used_cells():
				total += 1
				if layer.get_cell_tile_data(c) == null:
					missing += 1
				if args.has("cells"):
					var a := layer.get_cell_atlas_coords(c)
					cells.append([String(room.get_path_to(layer)), c.x, c.y, layer.get_cell_source_id(c), a.x, a.y])
		result = {"total": total, "missing": missing, "cells": cells}
		var sources := []
		if tileset and args.has("tiles"):
			for i in tileset.get_source_count():
				var id := tileset.get_source_id(i)
				var source := tileset.get_source(id) as TileSetAtlasSource
				var tiles := []
				for t in source.get_tiles_count():
					var coords := source.get_tile_id(t)
					var regions := []
					for f in source.get_tile_animation_frames_count(coords):
						var r := source.get_tile_texture_region(coords, f)
						regions.append([r.position.x, r.position.y, r.size.x, r.size.y])
					tiles.append({"atlas": [coords.x, coords.y], "regions": regions})
				sources.append({"id": id, "name": source.resource_name, "tiles": tiles})
		result["sources"] = sources
		room.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// Imports the project, then runs `TILES` on `scene`: its JSON and what Godot printed on stderr
/// while loading it. Gives up after two minutes rather than hang on a script error.
fn godot_tiles(godot: &Path, dir: &Path, scene: &str, flags: &[&str]) -> (Value, String) {
    let import = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .arg("--import")
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    std::fs::write(dir.join("tiles.gd"), TILES).unwrap();
    let (out, err) = (dir.join("tiles.out"), dir.join("tiles.err"));
    let mut child = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .args(["--script", "res://tiles.gd", "--", scene])
        .args(flags)
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(120) {
            child.kill().ok();
            panic!(
                "Godot didn't finish: {}",
                std::fs::read_to_string(&err).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let (stdout, stderr) = (
        std::fs::read_to_string(&out).unwrap(),
        std::fs::read_to_string(&err).unwrap(),
    );
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| panic!("no result from Godot:\n{stdout}\n{stderr}"));
    (serde_json::from_str(json).unwrap(), stderr)
}

/// The regions Godot shows a source's tiles' frames from, by atlas cell.
fn regions(source: &Value) -> HashMap<(i64, i64), Vec<[i64; 4]>> {
    let int = |v: &Value| v.as_f64().unwrap() as i64;
    source["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                (int(&t["atlas"][0]), int(&t["atlas"][1])),
                t["regions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| [int(&r[0]), int(&r[1]), int(&r[2]), int(&r[3])])
                    .collect(),
            )
        })
        .collect()
}

/// Godot loads the probe without a complaint and steps through each animation's frames where
/// Tiled has them; Godot shows the two it can't lay out at their first frame.
#[test]
fn godot_steps_through_the_probe_animations() {
    let Some(godot) = godot() else { return };
    let dir = probe("tileset-godot");
    export(
        &dir.join("maps/probe.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("probe.tscn"),
        &Options::default(),
    )
    .unwrap();
    let (scene, stderr) = godot_tiles(&godot, &dir, "res://probe.tscn", &["tiles"]);
    assert_eq!(stderr, "", "Godot loads it cleanly");
    assert_eq!(
        (scene["total"].as_i64(), scene["missing"].as_i64()),
        (Some(12), Some(0))
    );
    let sources = scene["sources"].as_array().unwrap();
    assert_eq!(
        sources
            .iter()
            .map(|s| s["id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(sources[2]["name"], "maps/probe.tsx#anim1");
    // A probe tile's region of tiles.png.
    let tile = |id: i64| [id % 4 * 16, id / 4 * 16, 16, 16];
    let anim1 = regions(&sources[2]);
    assert_eq!(anim1.len(), 6);
    assert_eq!(anim1[&(0, 0)], [tile(0), tile(1), tile(2)]);
    assert_eq!(anim1[&(0, 1)], [tile(4), tile(6)]);
    assert_eq!(anim1[&(1, 1)], [tile(5), tile(7)]);
    assert_eq!(anim1[&(0, 2)], [tile(8), tile(12)]);
    assert_eq!(anim1[&(1, 2)], [tile(9)], "the first frame");
    assert_eq!(
        anim1[&(3, 3)],
        [tile(15)],
        "the first frame, another tile's"
    );
    assert_eq!(regions(&sources[3])[&(1, 0)], [tile(1), tile(2)]);
    let base = regions(&sources[1]);
    assert!(
        base.len() == 16 && base.iter().all(|(&(x, y), r)| r == &[tile(y * 4 + x)]),
        "frame tiles stay plain: {base:?}"
    );
    assert_eq!(regions(&sources[4])[&(0, 0)], [[32, 0, 32, 32]]);
    assert_eq!(
        regions(&sources[8])[&(0, 0)],
        [[0, 32, 16, 16], [32, 32, 16, 16]],
        "frames in a row of one image"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// rpg/island's water: 33 animations whose frames interleave along the atlas rows. Godot loads
/// them all without an error, each with all its frames where Tiled has them, and a cell painted
/// with one of the frames shows that tile, still.
#[test]
fn godot_animates_island_water() {
    let Some(godot) = godot() else { return };
    let Some(path) = bt_test_data::test_data("reference/tiled/examples/rpg/island.tmx") else {
        return;
    };
    let dir = project("island");
    let mut map = bt_tmx::Loader::builder()
        .build()
        .load_map(&path)
        .unwrap()
        .value;
    let ts = map.tilesets[0].tileset.clone();
    let over = map.layers.iter_mut().find(|l| l.name == "Over").unwrap();
    over.as_tile_layer_mut()
        .unwrap()
        .set_cell(
            IVec2::ZERO,
            Cell::new(TilesetSlot::new(0).unwrap(), bt_core::TileId(46)),
        )
        .unwrap();
    let report = export_map(
        &map,
        &path,
        &CustomTypes::default(),
        &dir.join("island.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert!(report.warnings().is_empty(), "{:?}", report.warnings());
    assert_eq!((report.counts.tiles, report.counts.sources), (936 + 33, 2));

    let (scene, stderr) = godot_tiles(&godot, &dir, "res://island.tscn", &["tiles", "cells"]);
    assert_eq!(stderr, "", "Godot loads it cleanly");
    assert_eq!(scene["missing"], 0);
    let sources = scene["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(
        sources[0]["name"]
            .as_str()
            .unwrap()
            .ends_with("rpg/beach_tileset.tsx")
            && sources[1]["name"]
                .as_str()
                .unwrap()
                .ends_with("rpg/beach_tileset.tsx#anim1")
    );
    let rect = |id: u32| {
        let r = ts.tile_image(bt_core::TileId(id)).unwrap().rect;
        [r.x as i64, r.y as i64, r.w as i64, r.h as i64]
    };
    let animated = regions(&sources[1]);
    let mut heads = HashSet::new();
    for (&id, data) in ts.tiles.iter().filter(|(_, d)| !d.animation.is_empty()) {
        let head = (id.0 as i64 % 36, id.0 as i64 / 36);
        heads.insert(head);
        let expected: Vec<[i64; 4]> = data.animation.iter().map(|f| rect(f.tile.0)).collect();
        assert_eq!(animated.get(&head), Some(&expected), "tile {id}");
    }
    assert_eq!(heads.len(), 33);
    assert_eq!(animated.len(), 33);
    // Painted with the frame tile 46: its own source, still.
    let painted: Vec<&Value> = scene["cells"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c[0] == "Over" && c[1] == 0 && c[2] == 0)
        .collect();
    assert_eq!(painted, [&json!(["Over", 0, 0, 0, 10, 1])]);
    assert_eq!(regions(&sources[0])[&(10, 1)], [rect(46)]);
    // Painted with the animated tile 38: its animation in the animation source.
    let ground = map
        .layers
        .iter()
        .find(|l| l.name == "Ground")
        .and_then(|l| l.as_tile_layer())
        .unwrap();
    let water = ground.cells().find(|p| p.cell.tile().0 == 38).unwrap().at;
    let cell = scene["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c[0] == "Ground" && c[1] == water.x && c[2] == water.y)
        .unwrap();
    assert_eq!(cell, &json!(["Ground", water.x, water.y, 1, 2, 1]));
    std::fs::remove_dir_all(&dir).ok();
}

/// forest's squirrel: an image collection's tile whose frames are a column of parts of one image.
#[test]
fn godot_animates_the_forest_squirrel() {
    let Some(godot) = godot() else { return };
    let Some(path) = bt_test_data::test_data("reference/tiled/examples/forest/forest.tmx") else {
        return;
    };
    let dir = project("forest");
    let report = export(
        &path,
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("forest.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert!(
        report
            .notes
            .iter()
            .all(|n| n.code.as_str() != "godot/animation-layout"),
        "{:?}",
        report.warnings()
    );
    let (scene, stderr) = godot_tiles(&godot, &dir, "res://forest.tscn", &["tiles"]);
    assert_eq!(stderr, "", "Godot loads it cleanly");
    let sources = scene["sources"].as_array().unwrap();
    let squirrel = sources
        .iter()
        .find(|s| {
            s["name"]
                .as_str()
                .unwrap()
                .ends_with("forest/forest.tsx#13")
        })
        .unwrap();
    assert_eq!(
        regions(squirrel)[&(0, 0)],
        [[116, 824, 25, 25], [116, 850, 25, 25]]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The Highlands Sample scene: Godot finds tile data for every cell, the cells of fully
/// transparent tiles included.
#[test]
fn godot_finds_tile_data_for_every_highlands_cell() {
    let Some(godot) = godot() else { return };
    let Some(path) = bt_test_data::test_data(
        "assets/Epic RPG World - Highlands V1.4.1/TiledMap Editor/Sample scene.tmx",
    ) else {
        return;
    };
    let dir = project("highlands");
    let map = bt_tmx::Loader::builder()
        .build()
        .load_map(&path)
        .unwrap()
        .value;
    // The painted tiles with nothing to see.
    let mut images = HashMap::new();
    let mut transparent = HashSet::new();
    let mut cells = 0;
    for layer in &map.layers {
        let LayerKind::Tile(tiles) = &layer.kind else {
            continue;
        };
        for bt_core::PlacedCell { cell, .. } in tiles.cells() {
            let slot = cell.slot().unwrap().index();
            let Some(bt_core::TileImage { image, rect }) =
                map.tilesets[slot].tileset.tile_image(cell.tile())
            else {
                continue;
            };
            let pixels = images
                .entry(image.path().unwrap().to_path_buf())
                .or_insert_with(|| image::open(image.path().unwrap()).unwrap().to_rgba8());
            let clear = (rect.y..rect.y + rect.h).all(|y| {
                (rect.x..rect.x + rect.w).all(|x| pixels.get_pixel(x as u32, y as u32)[3] == 0)
            });
            if clear {
                transparent.insert((slot, cell.tile()));
                cells += 1;
            }
        }
    }
    // Tiled's exporter leaves these out: it writes a tile only when something of it shows.
    assert_eq!((transparent.len(), cells), (11, 11));
    export(
        &path,
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("sample.tscn"),
        &Options::default(),
    )
    .unwrap();
    let (scene, stderr) = godot_tiles(&godot, &dir, "res://sample.tscn", &[]);
    assert_eq!(stderr, "", "Godot loads it cleanly");
    assert!(scene["total"].as_i64().unwrap() > 0);
    assert_eq!(scene["missing"], 0, "cells without tile data");
    std::fs::remove_dir_all(&dir).ok();
}
