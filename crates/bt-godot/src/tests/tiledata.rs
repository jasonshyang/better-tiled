//! Tile data (docs/design/12-godot-export.md): tiles' properties as custom data layers of exact types, their
//! Godot settings, and their collision shapes on the physics layers the setup describes.

use std::path::Path;

use bt_core::{LayerKind, MapGeometry, Orientation, Property, Value};
use glam::{DVec2, IVec2};
use serde_json::Value as Json;

use super::super::grid::tiled_centre;
use super::super::tiledata::{Kind, in_layer, layers};
use super::super::{Options, Report, export, read};
use super::support::*;
use bt_core::rotated as rotate;

#[test]
fn layers_take_their_first_tiles_type_and_ints_with_floats_make_floats() {
    let (int, float, text, object) = (
        Value::Int(3),
        Value::Float(2.5),
        Value::String("x".into()),
        Value::Object(bt_core::ObjectId(7)),
    );
    let tiles = [
        vec![("a", &int), ("b", &text), ("c", &float), ("d", &object)],
        vec![("a", &float), ("b", &int), ("c", &int), ("e", &text)],
    ];
    let found = layers(tiles.iter().map(|t| t.iter().copied()));
    let kinds: Vec<(&str, Kind)> = found.iter().map(|(n, k)| (n.as_str(), *k)).collect();
    assert_eq!(
        kinds,
        [
            ("a", Kind::Float),
            ("b", Kind::String),
            ("c", Kind::Float),
            ("d", Kind::Int),
            ("e", Kind::String)
        ],
        "sorted by name"
    );
    assert!(found.iter().all(|(_, k)| *k as i32 != 0), "never nil");

    let mut file = |f: &str| format!("res://{f}");
    assert_eq!(
        in_layer(&int, Kind::Float, &mut file).as_deref(),
        Some("3.0"),
        "an int in a float layer is a float"
    );
    assert_eq!(
        in_layer(&object, Kind::Float, &mut file).as_deref(),
        Some("7.0")
    );
    assert_eq!(
        in_layer(&int, Kind::String, &mut file),
        None,
        "Godot would make it null"
    );
    assert_eq!(
        in_layer(&float, Kind::Int, &mut file),
        None,
        "Godot would make it 2"
    );
    assert_eq!(
        in_layer(&Value::File("a.png".into()), Kind::String, &mut file).as_deref(),
        Some("\"res://a.png\"")
    );
    let list = Value::List(vec![
        Property::new(Value::Int(1)),
        Property::new(Value::File("b.png".into())),
        Property::new(Value::Color(None)),
    ]);
    assert_eq!(
        in_layer(&list, Kind::Array, &mut file).as_deref(),
        Some("[1, \"res://b.png\", Color(0, 0, 0, 0)]")
    );
    let class = Value::Class(
        [
            ("n".to_owned(), Property::new(Value::Bool(true))),
            (
                "o".to_owned(),
                Property::new(Value::Object(bt_core::ObjectId(4))),
            ),
        ]
        .into(),
    );
    assert_eq!(
        in_layer(&class, Kind::Dictionary, &mut file).as_deref(),
        Some("{\"n\": true, \"o\": 4}")
    );
}

/// Tiles with a property of each of Tiled's types, mixed and conflicting types, classes, Godot
/// settings and collision shapes of every kind.
const DATA_TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="data" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="0" type="Crate">
  <properties>
   <property name="art" type="file" value="tiles.png"/>
   <property name="count" type="int" value="3"/>
   <property name="dir" propertytype="Dir" value="East"/>
   <property name="flag" type="bool" value="true"/>
   <property name="godotYSortOrigin" type="int" value="-4"/>
   <property name="godotZIndex" type="int" value="5"/>
   <property name="items" type="list">
    <item type="int" value="1"/>
    <item type="string" value="two"/>
    <item type="float" value="3.5"/>
   </property>
   <property name="label" value="hi &quot;there&quot;"/>
   <property name="loot" type="class" propertytype="Loot">
    <properties>
     <property name="gold" type="int" value="5"/>
    </properties>
   </property>
   <property name="outside" type="file" value="../../outside.png"/>
   <property name="rarity" type="int" propertytype="Rarity" value="2"/>
   <property name="speed" type="float" value="1.5"/>
   <property name="target" type="object" value="7"/>
   <property name="tint" type="color" value="#80ff0000"/>
   <property name="zIndex" type="int" value="9"/>
  </properties>
 </tile>
 <tile id="1" type="Gate">
  <properties>
   <property name="class" value="special"/>
   <property name="count" type="float" value="2.5"/>
   <property name="flag" value="yes"/>
  </properties>
 </tile>
 <tile id="2">
  <properties>
   <property name="godotYSortOrigin" value="high"/>
   <property name="godotZIndex" type="int" value="5000"/>
  </properties>
 </tile>
 <tile id="3">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0" width="16" height="8">
    <properties>
     <property name="godotOneWay" type="bool" value="true"/>
     <property name="godotOneWayMargin" type="float" value="2"/>
     <property name="godotPhysicsLayer" type="int" value="1"/>
    </properties>
   </object>
   <object id="2" x="0" y="8" width="16" height="8"/>
   <object id="3" x="4" y="4" width="8" height="8">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
    </properties>
    <ellipse/>
   </object>
   <object id="4" x="2" y="2">
    <point/>
   </object>
   <object id="5" x="0" y="0">
    <polyline points="0,0 8,8"/>
   </object>
  </objectgroup>
 </tile>
</tileset>
"##;

/// The data tiles in a row, and a tile bigger than the grid with data of its own, painted flipped
/// (so it gets an alternative tile) and plain.
const DATA_TMX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="6" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="data.tsx"/>
 <tileset firstgid="17" name="big" tilewidth="32" tileheight="32" tilecount="4" columns="2">
  <image source="tiles.png" width="64" height="64"/>
  <tile id="0">
   <properties>
    <property name="kind" value="tall"/>
   </properties>
   <objectgroup draworder="index">
    <object id="1" x="0" y="16" width="32" height="16">
     <properties>
      <property name="godotOneWay" type="bool" value="true"/>
     </properties>
    </object>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="cells" width="6" height="2">
  <data encoding="csv">
1,2,3,4,0,0,
0,2147483665,0,17,0,0
</data>
 </layer>
</map>
"#;

/// Physics layer 1 with its own bits; layer 0 as Godot has it.
const SETUP: &str = r#"{"betterTiled": 1, "godot": {"physicsLayers": [{"collisionLayer": 1}, {"collisionLayer": 4, "collisionMask": 6}]}}"#;

/// The data map in a scratch project, with the setup when `setup` is true.
fn data_project(name: &str, setup: bool) -> std::path::PathBuf {
    let dir = project(name);
    std::fs::write(dir.join("maps/data.tsx"), DATA_TSX).unwrap();
    std::fs::write(dir.join("maps/data.tmx"), DATA_TMX).unwrap();
    if setup {
        std::fs::write(dir.join("maps/better-tiled.json"), SETUP).unwrap();
    }
    dir
}

fn export_data(dir: &Path) -> (Report, String) {
    let target = dir.join("data.tscn");
    let report = export(
        &dir.join("maps/data.tmx"),
        &mut loader(dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    (report, std::fs::read_to_string(&target).unwrap())
}

/// The lines of the TileSet's sub-resource named `kind` (`TileSet`) or, for an atlas source, by
/// its `resource_name`, as (key, value).
fn resource_lines(tscn: &str, name: &str) -> Vec<(String, String)> {
    let resource = read::parse(tscn).unwrap();
    let (_, section) = resource
        .sub_resources()
        .find(|(sub, section)| {
            if name == "TileSet" {
                sub.class == "TileSet"
            } else {
                section
                    .get("resource_name")
                    .and_then(read::string)
                    .as_deref()
                    == Some(name)
            }
        })
        .unwrap_or_else(|| panic!("no {name}"));
    section
        .props
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The lines under `prefix` (`0:0/0/`), without it, leaving out the texture origin and probability.
fn under(lines: &[(String, String)], prefix: &str) -> Vec<(String, String)> {
    lines
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(prefix)?.to_owned(), v.clone())))
        .filter(|(k, _)| !matches!(k.as_str(), "texture_origin" | "probability"))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The custom data layer names, in the order the TileSet has them.
const LAYERS: [&str; 15] = [
    "art", "class", "count", "dir", "flag", "items", "kind", "label", "loot", "outside", "rarity",
    "speed", "target", "tint", "zIndex",
];

#[test]
fn tile_properties_become_custom_data_layers_of_exact_types() {
    let dir = data_project("tiledata", true);
    let (report, tscn) = export_data(&dir);

    // The layers come before the sources, sorted by name, typed by their values.
    let tileset = resource_lines(&tscn, "TileSet");
    let keys: Vec<&str> = tileset.iter().map(|(k, _)| k.as_str()).collect();
    let first_source = keys.iter().position(|k| k.starts_with("sources/")).unwrap();
    assert!(
        keys[..first_source].contains(&"custom_data_layer_14/type")
            && keys[first_source..]
                .iter()
                .all(|k| k.starts_with("sources/")),
        "{keys:?}"
    );
    let types = [
        "4", "4", "3", "4", "1", "28", "4", "4", "27", "4", "2", "3", "2", "20", "2",
    ];
    let mut expected = Vec::new();
    for (n, (name, kind)) in LAYERS.iter().zip(types).enumerate() {
        expected.push((format!("custom_data_layer_{n}/name"), format!("\"{name}\"")));
        expected.push((format!("custom_data_layer_{n}/type"), kind.to_owned()));
    }
    let custom: Vec<(String, String)> = tileset
        .iter()
        .filter(|(k, _)| k.starts_with("custom_data_layer_"))
        .cloned()
        .collect();
    assert_eq!(custom, expected);
    assert_eq!(report.counts.custom_data_layers, 15);

    // Tile 0: every type, its Godot settings, a file inside the project and one outside it.
    let data = resource_lines(&tscn, "maps/data.tsx");
    let tile = |x: i32, y: i32| under(&data, &format!("{x}:{y}/0/"));
    let loot = "{\"gold\": 5, \"rarity\": 0}";
    assert_eq!(
        tile(0, 0),
        pairs(&[
            ("z_index", "5"),
            ("y_sort_origin", "-4"),
            ("custom_data_0", "\"res://maps/tiles.png\""),
            ("custom_data_1", "\"Crate\""),
            ("custom_data_2", "3.0"),
            ("custom_data_3", "\"East\""),
            ("custom_data_4", "true"),
            ("custom_data_5", "[1, \"two\", 3.5]"),
            ("custom_data_7", "\"hi \\\"there\\\"\""),
            ("custom_data_8", loot),
            ("custom_data_9", "\"../../outside.png\""),
            ("custom_data_10", "2"),
            ("custom_data_11", "1.5"),
            ("custom_data_12", "7"),
            ("custom_data_13", "Color(1, 0, 0, 0.501961)"),
            ("custom_data_14", "9"),
        ])
    );
    // Tile 1: its own `class` property wins over its class; the string `flag` is left out.
    assert_eq!(
        tile(1, 0),
        pairs(&[("custom_data_1", "\"special\""), ("custom_data_2", "2.5")])
    );
    // Tile 2: z_index clamped; a y_sort_origin that isn't a number left out.
    assert_eq!(tile(2, 0), pairs(&[("z_index", "4096")]));

    let notes: Vec<(&str, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.message.as_str()))
        .collect();
    let note = |code: &str| {
        notes
            .iter()
            .filter(|(c, _)| *c == code)
            .map(|(_, m)| *m)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        note("godot/custom-data-type"),
        [
            "custom data layer \"flag\" is a bool layer, so its string values are left out: give \"flag\" one type on every tile (tileset data: tile 1)"
        ]
    );
    assert_eq!(note("godot/custom-data-class").len(), 1);
    assert!(note("godot/custom-data-class")[0].ends_with("(tileset data: tile 1)"));
    assert!(
        note("godot/custom-data-file")[0].contains("\"outside\"")
            && note("godot/custom-data-file")[0].ends_with("(tileset data: tile 0)"),
        "{notes:?}"
    );
    assert!(note("godot/zindex-clamped")[0].ends_with("(tileset data: tile 2)"));
    assert_eq!(
        note("godot/setting-type"),
        ["godotYSortOrigin takes a whole number: other values are left out (tileset data: tile 2)"]
    );
    assert_eq!(
        note("godot/tile-collision-kind").len(),
        1,
        "one note for both shapes"
    );
    assert!(note("godot/tile-collision-kind")[0].ends_with("(tileset data: tile 3)"));
    // The big tile, mirrored, reaches out of its cell: the layer collides cell by cell.
    assert_eq!(note("godot/physics-quadrant-size").len(), 1);
    assert_eq!(report.notes.len(), 7, "{notes:?}");

    // The godot* names are consumed: not custom data, and listed in the report.
    let consumed: Vec<&str> = report.consumed.iter().map(String::as_str).collect();
    assert_eq!(
        consumed,
        [
            "godotCollision",
            "godotOneWay",
            "godotOneWayMargin",
            "godotPhysicsLayer",
            "godotYSortOrigin",
            "godotZIndex"
        ]
    );

    // The same bytes on a second run.
    let (_, again) = export_data(&dir);
    assert_eq!(
        tscn.split_once("[gd_scene").unwrap().1,
        again.split_once("[gd_scene").unwrap().1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn collision_shapes_go_on_the_physics_layers_the_setup_describes() {
    let dir = data_project("tiledata-physics", true);
    let (_, tscn) = export_data(&dir);
    let tileset = resource_lines(&tscn, "TileSet");
    let physics: Vec<(String, String)> = tileset
        .iter()
        .filter(|(k, _)| k.starts_with("physics_layer_"))
        .cloned()
        .collect();
    assert_eq!(
        physics,
        pairs(&[
            ("physics_layer_0/collision_layer", "1"),
            ("physics_layer_1/collision_layer", "4"),
            ("physics_layer_1/collision_mask", "6")
        ])
    );

    // Tile 3: a one-way rectangle on layer 1, a plain one on layer 0; the ellipse kept out of
    // physics, the point and the polyline left out.
    let data = resource_lines(&tscn, "maps/data.tsx");
    assert_eq!(
        under(&data, "3:0/0/"),
        pairs(&[
            (
                "physics_layer_1/polygon_0/points",
                "PackedVector2Array(-8, -8, 8, -8, 8, 0, -8, 0)"
            ),
            ("physics_layer_1/polygon_0/one_way", "true"),
            ("physics_layer_1/polygon_0/one_way_margin", "2.0"),
            (
                "physics_layer_0/polygon_0/points",
                "PackedVector2Array(-8, 0, 8, 0, 8, 8, -8, 8)"
            ),
        ])
    );

    // The big tile's flipped alternative has its data and its one-way shape too.
    let big = resource_lines(&tscn, "maps/data.tmx::big");
    let plain = under(&big, "0:0/0/");
    assert_eq!(
        plain,
        pairs(&[
            (
                "physics_layer_0/polygon_0/points",
                "PackedVector2Array(-8, -8, 24, -8, 24, 8, -8, 8)"
            ),
            ("physics_layer_0/polygon_0/one_way", "true"),
            ("custom_data_6", "\"tall\"")
        ])
    );
    let flipped = under(&big, "0:0/1/");
    assert_eq!(flipped.len(), plain.len(), "{big:?}");
    assert_eq!(
        flipped[1..],
        plain[1..],
        "the same one-way flag and custom data"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn physics_layers_without_a_setup_have_godots_defaults() {
    let dir = data_project("tiledata-nosetup", false);
    let (report, tscn) = export_data(&dir);
    let tileset = resource_lines(&tscn, "TileSet");
    let physics: Vec<(String, String)> = tileset
        .iter()
        .filter(|(k, _)| k.starts_with("physics_layer_"))
        .cloned()
        .collect();
    assert_eq!(
        physics,
        pairs(&[
            ("physics_layer_0/collision_layer", "1"),
            ("physics_layer_1/collision_layer", "1")
        ])
    );
    let unset: Vec<&str> = report
        .notes
        .iter()
        .filter(|n| n.code.as_str() == "godot/physics-layer-unset")
        .map(|n| n.message.as_str())
        .collect();
    assert_eq!(
        unset,
        [
            "physics layer 1 has no entry in physicsLayers of better-tiled.json's godot setup: Godot gives it collision layer 1 and mask 1"
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Describes the tile data of every painted cell: its custom data (Godot's type and text for
/// each), `z_index`, `y_sort_origin` and polygons per physics layer; and the TileSet's custom
/// data and physics layers.
const DATA: &str = r#"extends SceneTree

func _init() -> void:
	var room: Node = (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	var result := {"custom": [], "physics": [], "cells": {}}
	for node: Node in room.find_children("*", "TileMapLayer", true, false):
		var layer := node as TileMapLayer
		var tileset := layer.tile_set
		result["custom"] = []
		for i in tileset.get_custom_data_layers_count():
			result["custom"].append([tileset.get_custom_data_layer_name(i), tileset.get_custom_data_layer_type(i)])
		result["physics"] = []
		for i in tileset.get_physics_layers_count():
			result["physics"].append([tileset.get_physics_layer_collision_layer(i), tileset.get_physics_layer_collision_mask(i)])
		for c: Vector2i in layer.get_used_cells():
			var data := layer.get_cell_tile_data(c)
			var custom := {}
			for i in tileset.get_custom_data_layers_count():
				var v: Variant = data.get_custom_data_by_layer_id(i)
				custom[tileset.get_custom_data_layer_name(i)] = [typeof(v), var_to_str(v)]
			var physics := []
			for l in tileset.get_physics_layers_count():
				var polygons := []
				for p in data.get_collision_polygons_count(l):
					var points := []
					for v: Vector2 in data.get_collision_polygon_points(l, p):
						points.append([v.x, v.y])
					polygons.append({"points": points, "one_way": data.is_collision_polygon_one_way(l, p), "margin": data.get_collision_polygon_one_way_margin(l, p)})
				physics.append(polygons)
			result["cells"]["%s:%d,%d" % [layer.name, c.x, c.y]] = {"z_index": data.z_index, "y_sort_origin": data.y_sort_origin, "custom": custom, "physics": physics}
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	room.free()
	quit()
"#;

/// Imports the project and runs `DATA` on `scene`: its JSON and Godot's stderr.
fn godot_data(godot: &Path, dir: &Path, scene: &str) -> (Json, String) {
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
    std::fs::write(dir.join("data.gd"), DATA).unwrap();
    let output = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .args(["--script", "res://data.gd", "--", scene])
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| panic!("no result from Godot:\n{stdout}\n{stderr}"));
    (serde_json::from_str(json).unwrap(), stderr)
}

fn points(polygon: &Json) -> Vec<DVec2> {
    polygon["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| DVec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

/// Godot reads every custom data value back with its exact type, the tile settings, the one-way
/// shapes and the physics layers' bits.
#[test]
fn godot_reads_custom_data_and_physics_back_exactly() {
    let Some(godot) = godot() else { return };
    let dir = data_project("tiledata-godot", true);
    export_data(&dir);
    let (scene, stderr) = godot_data(&godot, &dir, "res://data.tscn");
    assert_eq!(stderr, "", "Godot loads it cleanly");
    let layers: Vec<(String, i64)> = scene["custom"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l[0].as_str().unwrap().to_owned(), l[1].as_i64().unwrap()))
        .collect();
    assert_eq!(
        layers.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        LAYERS
    );
    assert_eq!(scene["physics"], serde_json::json!([[1, 1], [4, 6]]));

    let cell = |c: &str| &scene["cells"][format!("cells:{c}")];
    let custom = |c: &str, name: &str| {
        let v = &cell(c)["custom"][name];
        (v[0].as_i64().unwrap(), v[1].as_str().unwrap().to_owned())
    };
    let expected = [
        ("art", 4, "\"res://maps/tiles.png\""),
        ("class", 4, "\"Crate\""),
        ("count", 3, "3.0"),
        ("dir", 4, "\"East\""),
        ("flag", 1, "true"),
        ("items", 28, "[1, \"two\", 3.5]"),
        ("label", 4, "\"hi \\\"there\\\"\""),
        ("loot", 27, "{\n\"gold\": 5,\n\"rarity\": 0\n}"),
        ("outside", 4, "\"../../outside.png\""),
        ("rarity", 2, "2"),
        ("speed", 3, "1.5"),
        ("target", 2, "7"),
        ("tint", 20, "Color(1, 0, 0, 0.501961)"),
        ("zIndex", 2, "9"),
    ];
    for (name, kind, text) in expected {
        assert_eq!(custom("0,0", name), (kind, text.to_owned()), "{name}");
    }
    assert_eq!(
        (
            cell("0,0")["z_index"].as_i64(),
            cell("0,0")["y_sort_origin"].as_i64()
        ),
        (Some(5), Some(-4))
    );
    assert_eq!(custom("1,0", "class"), (4, "\"special\"".to_owned()));
    assert_eq!(
        custom("1,0", "flag"),
        (1, "false".to_owned()),
        "left out: the layer's default"
    );
    assert_eq!(cell("2,0")["z_index"].as_i64(), Some(4096));

    // Tile 3's shapes, each on its layer, the one-way one with its margin.
    let physics = &cell("3,0")["physics"];
    assert_eq!(
        (
            physics[0].as_array().unwrap().len(),
            physics[1].as_array().unwrap().len()
        ),
        (1, 1)
    );
    assert_eq!(
        (&physics[1][0]["one_way"], physics[1][0]["margin"].as_f64()),
        (&Json::Bool(true), Some(2.0))
    );
    assert_eq!(
        (&physics[0][0]["one_way"], physics[0][0]["margin"].as_f64()),
        (&Json::Bool(false), Some(1.0)),
        "Godot's default margin"
    );
    assert_eq!(
        points(&physics[1][0]),
        [
            DVec2::new(-8.0, -8.0),
            DVec2::new(8.0, -8.0),
            DVec2::new(8.0, 0.0),
            DVec2::new(-8.0, 0.0)
        ]
    );

    // The flipped big tile has its data and one-way shape, as the plain one does.
    for c in ["1,1", "3,1"] {
        assert_eq!(custom(c, "kind"), (4, "\"tall\"".to_owned()), "{c}");
        assert_eq!(cell(c)["physics"][0][0]["one_way"], Json::Bool(true), "{c}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Tiled's isometric example with collision shapes on the tile its first cell is painted with:
/// a rectangle, a polygon and a rotated rectangle, on the tileset's isometric grid.
fn iso_project(name: &str) -> Option<(std::path::PathBuf, IVec2, u32)> {
    let example =
        bt_test_data::test_data("reference/tiled/examples/isometric_grass_and_water.tmx")?;
    let dir = project(name);
    std::fs::copy(
        example.with_extension("png"),
        dir.join("maps/isometric_grass_and_water.png"),
    )
    .unwrap();
    let map = bt_tmx::Loader::builder()
        .build()
        .load_map(&example)
        .unwrap()
        .value;
    let LayerKind::Tile(tiles) = &map.layers[0].kind else {
        panic!("a tile layer")
    };
    let bt_core::PlacedCell {
        at: cell,
        cell: painted,
    } = tiles.cells().find(|p| !p.cell.is_empty()).unwrap();
    let shapes = format!(
        r#"<tile id="{}"><objectgroup draworder="index"><object id="1" x="0" y="0" width="32" height="32"/><object id="2" x="8" y="4"><polygon points="0,0 16,0 0,16"/></object><object id="3" x="16" y="8" width="8" height="16" rotation="30"/></objectgroup></tile>"#,
        painted.tile().0
    );
    let text = std::fs::read_to_string(&example).unwrap();
    let image = "<image source=\"isometric_grass_and_water.png\" width=\"256\" height=\"384\"/>";
    assert!(text.contains(image));
    std::fs::write(
        dir.join("maps/iso.tmx"),
        text.replacen(image, &format!("{image}\n  {shapes}"), 1),
    )
    .unwrap();
    Some((dir, cell, painted.tile().0))
}

/// Where Tiled draws the shapes of a tile painted at `cell` of the isometric example, from the
/// centre of that cell: bt-core's projection of each shape onto the tileset's 64×32 grid, whose
/// diamond is at the bottom of the 64×64 image, the image drawn from the cell's anchor and its
/// tile offset (0, 16).
fn tiled_iso_shapes(cell: IVec2) -> Vec<Vec<DVec2>> {
    let map = MapGeometry::new(
        Orientation::Isometric,
        IVec2::new(64, 32),
        IVec2::new(25, 25),
    );
    let grid = MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::ONE);
    let image = map.cell_anchor(cell) + DVec2::new(0.0, 16.0) - DVec2::new(0.0, 64.0);
    let lift = DVec2::new(0.0, 64.0 - 32.0);
    let at = |p: DVec2| image + lift + grid.pixel_to_screen(p) - tiled_centre(&map, cell);
    let box_of = |pos: DVec2, size: DVec2| {
        [
            pos,
            pos + DVec2::new(size.x, 0.0),
            pos + size,
            pos + DVec2::new(0.0, size.y),
        ]
    };
    let polygon = [
        DVec2::new(8.0, 4.0),
        DVec2::new(24.0, 4.0),
        DVec2::new(8.0, 20.0),
    ];
    let (pos, turned) = (
        DVec2::new(16.0, 8.0),
        box_of(DVec2::new(16.0, 8.0), DVec2::new(8.0, 16.0)),
    );
    // Tiled turns a shape on the screen, around its projected position.
    let turn =
        |p: DVec2| at(pos) + rotate(grid.pixel_to_screen(p) - grid.pixel_to_screen(pos), 30.0);
    vec![
        box_of(DVec2::ZERO, DVec2::splat(32.0)).map(at).to_vec(),
        polygon.map(at).to_vec(),
        turned.map(turn).to_vec(),
    ]
}

fn close(a: &[DVec2], b: &[DVec2]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 0.01)
}

#[test]
fn shapes_of_an_isometric_grid_tileset_are_projected_like_tiled_draws_them() {
    let Some((dir, cell, tile)) = iso_project("tiledata-iso") else {
        return;
    };
    let target = dir.join("iso.tscn");
    export(
        &dir.join("maps/iso.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let source = resource_lines(&tscn, "maps/iso.tmx::isometric_grass_and_water");
    let at = format!("{}:{}/0/", tile % 4, tile / 4);
    let written: Vec<Vec<DVec2>> = under(&source, &at)
        .iter()
        .filter(|(k, _)| k.ends_with("/points"))
        .map(|(_, v)| {
            let numbers: Vec<f64> = v
                .trim_start_matches("PackedVector2Array(")
                .trim_end_matches(')')
                .split(", ")
                .map(|n| n.parse().unwrap())
                .collect();
            numbers.chunks(2).map(|c| DVec2::new(c[0], c[1])).collect()
        })
        .collect();
    let expected = tiled_iso_shapes(cell);
    assert_eq!(written.len(), 3);
    for (w, e) in written.iter().zip(&expected) {
        assert!(close(w, e), "{w:?} != {e:?}");
    }
    assert_eq!(
        written[0],
        [
            DVec2::new(0.0, 0.0),
            DVec2::new(32.0, 16.0),
            DVec2::new(0.0, 32.0),
            DVec2::new(-32.0, 16.0)
        ],
        "the grid's diamond"
    );

    // Godot reads the same points back.
    if let Some(godot) = godot() {
        let (scene, stderr) = godot_data(&godot, &dir, "res://iso.tscn");
        assert_eq!(stderr, "", "Godot loads it cleanly");
        let polygons = &scene["cells"][format!("Tile Layer 1:{},{}", cell.x, cell.y)]["physics"][0];
        let read: Vec<Vec<DVec2>> = polygons.as_array().unwrap().iter().map(points).collect();
        assert_eq!(read.len(), 3);
        for (r, e) in read.iter().zip(&expected) {
            assert!(close(r, e), "Godot: {r:?} != {e:?}");
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}
