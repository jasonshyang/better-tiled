//! Objects (objects.rs): the orthogonal examples placed as before, shapes projected and every
//! object moved by the shift on isometric maps, `noExport` and `godotZIndex`, and `resPath`
//! instances: where the path is looked up, what keeps the typed node, the `@export` scan, and
//! (headless) the variables Godot then sets. Where Godot puts objects on every grid is the
//! oracle's (tests/oracle.rs).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Layer, LayerKind, MapObject, Shape};
use glam::DVec2;
use serde_json::json;

use super::super::objects::{Exports, gd_exports, scene_exports};
use super::super::{collision, read, text};
use super::oracle::{json_between_markers, run_godot};
use super::support::*;
use super::*;

/// Tiled's orthogonal examples with objects or without.
const ORTHOGONAL: [&str; 11] = [
    "desert",
    "sewers",
    "orthogonal-outside",
    "perspective_walls",
    "forest/forest",
    "rpg/island",
    "sewer_automap/sewers",
    "sewer_automap/rule_008",
    "sewer_automap/rule_009",
    "sticker-knight/map/sandbox",
    "sticker-knight/map/sandbox2",
];

fn objects_of(layers: &[Layer], out: &mut Vec<MapObject>) {
    for layer in layers {
        match &layer.kind {
            LayerKind::Object(objects) => out.extend(objects.objects.iter().cloned()),
            LayerKind::Group(children) => objects_of(children, out),
            _ => {}
        }
    }
}

/// Each node's props, by path.
type Props = HashMap<String, Vec<(String, String)>>;

/// The paths of a scene's nodes by `metadata/id` (objects), and every node's props.
fn scene_nodes(tscn: &str) -> (HashMap<i64, String>, Props) {
    let scene = read::parse(tscn).unwrap();
    let (mut ids, mut nodes) = (HashMap::new(), HashMap::new());
    for (node, section) in scene.nodes() {
        if let Some(id) = section.get("metadata/id").and_then(read::int) {
            ids.insert(id, node.path());
        }
        nodes.insert(
            node.path(),
            section
                .props
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
    }
    (ids, nodes)
}

fn prop<'n>(props: &'n [(String, String)], key: &str) -> Option<&'n str> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Orthogonal objects retain their Tiled positions, rotations and image anchors.
#[test]
fn orthogonal_objects_keep_their_positions_rotations_and_image_anchors() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let dir = project("objects-orthogonal");
    let mut maps: Vec<PathBuf> = ORTHOGONAL
        .iter()
        .map(|name| examples.join(format!("{name}.tmx")))
        .collect();
    maps.extend([dir.join("maps/features.tmx"), dir.join("maps/level.tmx")]);
    assert_eq!(check_orthogonal_objects(&maps, &dir), 276);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn highlands_objects_keep_their_positions_rotations_and_image_anchors() {
    let Some(chambers) = bt_test_data::test_data(
        "assets/Epic RPG World - Highlands V1.4.1/Highlands Biome Map/tiled",
    ) else {
        return;
    };
    let mut maps: Vec<PathBuf> = std::fs::read_dir(chambers)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "tmx"))
        .collect();
    maps.sort();
    assert_eq!(maps.len(), 8);
    let dir = project("objects-orthogonal-highlands");
    assert_eq!(check_orthogonal_objects(&maps, &dir), 593);
    std::fs::remove_dir_all(&dir).ok();
}

fn check_orthogonal_objects(maps: &[PathBuf], dir: &Path) -> usize {
    let mut checked = 0;
    for (i, map_path) in maps.iter().enumerate() {
        let target = dir.join(format!("scenes/{i}.tscn"));
        let mut loader = loader(dir);
        export(map_path, &mut loader, &target, &Options::default()).unwrap();
        let loaded = loader.load_map(map_path).unwrap().value;
        let rule_slots = crate::resolve::rule_slots(&loaded);
        let map = crate::resolve::resolved(&loaded, loader.custom_types());
        let (ids, nodes) = scene_nodes(&std::fs::read_to_string(&target).unwrap());
        let mut objects = Vec::new();
        objects_of(&map.layers, &mut objects);
        for o in objects {
            let name = format!("{} object {}", map_path.display(), o.id);
            let Some(path) = ids.get(&(o.id.0 as i64)) else {
                let rule_tile = o
                    .tile
                    .and_then(|c| c.slot())
                    .is_some_and(|s| rule_slots[s.index()]);
                assert!(rule_tile, "{name}: no node");
                continue;
            };
            let props = &nodes[path];
            let rotation = (o.rotation != 0.0).then(|| text::float(o.rotation.to_radians()));
            assert_eq!(prop(props, "rotation"), rotation.as_deref(), "{name}");
            if let Shape::Text(_) = o.shape {
                let edges =
                    [o.pos.x, o.pos.y, o.pos.x + o.size.x, o.pos.y + o.size.y].map(text::float);
                let got = ["offset_left", "offset_top", "offset_right", "offset_bottom"]
                    .map(|k| prop(props, k).unwrap().to_owned());
                assert_eq!(got, edges, "{name}");
            } else {
                assert_eq!(
                    prop(props, "position"),
                    Some(text::vector2(o.pos).as_str()),
                    "{name}"
                );
            }
            if let Some(cell) = o.tile {
                let rect = map
                    .tileset_of(cell)
                    .and_then(|t| t.tile_image(cell.tile()))
                    .map(|image| image.rect)
                    .unwrap();
                let size = DVec2::new(rect.w.max(1) as f64, rect.h.max(1) as f64);
                let scale = DVec2::new(
                    if o.size.x > 0.0 {
                        o.size.x / size.x
                    } else {
                        1.0
                    },
                    if o.size.y > 0.0 {
                        o.size.y / size.y
                    } else {
                        1.0
                    },
                );
                let (min, _) = crate::tests::geometry::local_box(&map, &o);
                assert_eq!(
                    prop(props, "scale"),
                    (scale != DVec2::ONE)
                        .then(|| text::vector2(scale))
                        .as_deref(),
                    "{name}"
                );
                assert_eq!(
                    prop(props, "offset"),
                    Some(text::vector2(min / scale).as_str()),
                    "{name}"
                );
            } else if let Some(shape) = nodes.get(&format!("{path}/Shape")) {
                match &o.shape {
                    Shape::Ellipse if o.size.x != o.size.y => assert_eq!(
                        prop(shape, "polygon"),
                        Some(text::points(&collision::ellipse(o.size)).as_str()),
                        "{name}"
                    ),
                    Shape::Polygon(points) => assert_eq!(
                        prop(shape, "polygon"),
                        Some(text::points(points).as_str()),
                        "{name}"
                    ),
                    _ => assert_eq!(
                        prop(shape, "position"),
                        Some(text::vector2(o.size / 2.0).as_str()),
                        "{name}"
                    ),
                }
            }
            checked += 1;
        }
    }
    checked
}

/// An isometric map (10 rows of 64×32 tiles: Tiled's cell centres are Godot's plus (288, 0)).
const ISOMETRIC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="isometric" renderorder="right-down" width="10" height="10" tilewidth="64" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="5">
 <tileset firstgid="1" name="tiles" tilewidth="32" tileheight="32" tilecount="4" columns="2">
  <image source="tiles.png" width="64" height="64"/>
 </tileset>
 <objectgroup id="1" name="objects">
  <object id="1" name="room" x="32" y="0" width="32" height="32"/>
  <object id="2" name="crate" gid="1" x="64" y="64" width="32" height="32"/>
  <object id="3" name="sign" x="32" y="32" width="64" height="20">
   <text>Hi</text>
  </object>
  <object id="4" name="ring" x="0" y="0" width="32" height="32">
   <ellipse/>
  </object>
 </objectgroup>
</map>
"#;

#[test]
fn isometric_shapes_are_projected_and_every_object_moves_by_the_shift() {
    let dir = project("objects-isometric");
    std::fs::write(dir.join("maps/iso.tmx"), ISOMETRIC).unwrap();
    let report = export(
        &dir.join("maps/iso.tmx"),
        &mut loader(&dir),
        &dir.join("iso.tscn"),
        &Options::default(),
    )
    .unwrap();
    let codes: Vec<&str> = report.notes.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(codes, ["godot/grid-shift"]);
    assert!(
        report.notes[0]
            .message
            .contains("objects and image layers are moved by (-288, 0) px"),
        "{}",
        report.notes[0].message
    );
    let s = sections(&std::fs::read_to_string(dir.join("iso.tscn")).unwrap());
    // A rectangle: a diamond around its projected corner, (352, 16) on Tiled's screen.
    assert_eq!(
        value(node(&s, "room", "objects"), "position"),
        "Vector2(64, 16)"
    );
    assert_eq!(
        value(node(&s, "Shape", "objects/room"), "polygon"),
        "PackedVector2Array(0, 0, 32, 16, 0, 32, -32, 16)"
    );
    // A circle is no circle there: 24 points around the projected ellipse.
    let ring = value(node(&s, "Shape", "objects/ring"), "polygon");
    assert!(
        ring.starts_with("PackedVector2Array(16, 24, ") && ring.matches(", ").count() == 47,
        "{ring}"
    );
    assert_eq!(
        value(node(&s, "ring", "objects"), "position"),
        "Vector2(32, 0)"
    );
    // Tile objects and text stand upright at their projected position, tiles centred above it.
    let crate_ = node(&s, "crate", "objects");
    assert_eq!(
        (value(crate_, "position"), value(crate_, "offset")),
        ("Vector2(32, 64)", "Vector2(-16, -32)")
    );
    let sign = node(&s, "sign", "objects");
    let edges =
        ["offset_left", "offset_top", "offset_right", "offset_bottom"].map(|k| value(sign, k));
    assert_eq!(edges, ["32.0", "32.0", "96.0", "52.0"]);

    // On a staggered grid nothing is projected or shifted: shapes keep their types.
    std::fs::write(
        dir.join("maps/staggered.tmx"),
        ISOMETRIC.replace(
            "orientation=\"isometric\"",
            "orientation=\"staggered\" staggeraxis=\"y\" staggerindex=\"odd\"",
        ),
    )
    .unwrap();
    let report = export(
        &dir.join("maps/staggered.tmx"),
        &mut loader(&dir),
        &dir.join("staggered.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert!(report.notes.is_empty(), "{:?}", report.warnings());
    let s = sections(&std::fs::read_to_string(dir.join("staggered.tscn")).unwrap());
    assert_eq!(
        value(node(&s, "room", "objects"), "position"),
        "Vector2(32, 0)"
    );
    let shape = node(&s, "Shape", "objects/room");
    assert_eq!(value(shape, "position"), "Vector2(16, 16)");
    assert!(value(shape, "shape").starts_with("SubResource(\"RectangleShape2D_"));
    assert_eq!(
        value(node(&s, "crate", "objects"), "offset"),
        "Vector2(0, -32)"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_export_leaves_objects_out_and_godot_z_index_orders_them() {
    let dir = project("objects-conventions");
    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="5">
 <objectgroup id="1" name="objects">
  <object id="1" name="hidden" x="0" y="0" width="8" height="8">
   <properties><property name="noExport" type="bool" value="true"/></properties>
  </object>
  <object id="2" name="kept" x="0" y="0" width="8" height="8">
   <properties><property name="godotZIndex" type="int" value="3"/><property name="noExport" type="bool" value="false"/><property name="other" value="x"/></properties>
  </object>
  <object id="3" name="high" x="0" y="0">
   <properties><property name="godotZIndex" type="int" value="5000"/></properties>
   <point/>
  </object>
  <object id="4" name="wrong" x="0" y="0">
   <properties><property name="godotZIndex" value="up"/></properties>
   <point/>
  </object>
 </objectgroup>
</map>
"#;
    std::fs::write(dir.join("maps/conventions.tmx"), map).unwrap();
    let report = export(
        &dir.join("maps/conventions.tmx"),
        &mut loader(&dir),
        &dir.join("c.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("c.tscn")).unwrap();
    let s = sections(&tscn);
    assert!(!tscn.contains("\"hidden\""), "{tscn}");
    assert_eq!(report.counts.objects, 3);
    let kept = node(&s, "kept", "objects");
    assert_eq!(
        (value(kept, "z_index"), value(kept, "metadata/other")),
        ("3", "\"x\"")
    );
    assert!(
        !tscn.contains("metadata/noExport") && !tscn.contains("metadata/godotZIndex"),
        "consumed, so not metadata: {tscn}"
    );
    assert_eq!(value(node(&s, "high", "objects"), "z_index"), "4096");
    assert!(
        !node(&s, "wrong", "objects")
            .iter()
            .any(|l| l.starts_with("z_index"))
    );
    let codes: Vec<&str> = report.notes.iter().map(|n| n.code.as_str()).collect();
    assert_eq!(
        codes,
        ["godot/zindex-clamped", "godot/property-type"],
        "{:?}",
        report.warnings()
    );
    assert_eq!(
        report.consumed,
        ["godotZIndex".to_owned(), "noExport".to_owned()].into()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `noExport` on an object is read as on a layer (`QVariant::toBool`, as Tiled's exporter
/// reads it): a string "true" or a number other than 0 leaves the object out too.
#[test]
fn no_export_on_objects_is_read_as_on_layers() {
    let dir = project("objects-no-export");
    let values = [
        ("bool", "true", false),
        ("string", "true", false),
        ("int", "1", false),
        ("float", "0.5", false),
        ("string", "yes", false),
        ("bool", "false", true),
        ("string", "false", true),
        ("string", "FALSE", true),
        ("int", "0", true),
        ("string", "", true),
    ];
    let objects: String = values
        .iter()
        .enumerate()
        .map(|(i, (kind, value, _))| format!("  <object id=\"{}\" name=\"o{}\" x=\"0\" y=\"0\"><properties><property name=\"noExport\" type=\"{kind}\" value=\"{value}\"/></properties><point/></object>\n", i + 1, i + 1))
        .collect();
    let map = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" orientation=\"orthogonal\" renderorder=\"right-down\" width=\"4\" height=\"4\" tilewidth=\"32\" tileheight=\"32\" infinite=\"0\" nextlayerid=\"2\" nextobjectid=\"20\">\n <objectgroup id=\"1\" name=\"objects\">\n{objects} </objectgroup>\n</map>\n"
    );
    std::fs::write(dir.join("maps/no-export.tmx"), map).unwrap();
    let report = export(
        &dir.join("maps/no-export.tmx"),
        &mut loader(&dir),
        &dir.join("n.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("n.tscn")).unwrap();
    let kept: Vec<bool> = (1..=values.len())
        .map(|i| tscn.contains(&format!("[node name=\"o{i}\" ")))
        .collect();
    assert_eq!(kept, values.map(|(_, _, kept)| kept), "{tscn}");
    assert!(!tscn.contains("metadata/noExport"), "{tscn}");
    assert_eq!(report.counts.objects, 5);
    std::fs::remove_dir_all(&dir).ok();
}

/// A minimal scene for instances.
fn write_scene(dir: &Path, res: &str, script: Option<&str>) {
    let path = dir.join(res.trim_start_matches("res://"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = match script {
        Some(script) => format!(
            "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"{script}\" id=\"1\"]\n\n[node name=\"Root\" type=\"Node2D\"]\nscript = ExtResource(\"1\")\n"
        ),
        None => "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n".to_owned(),
    };
    std::fs::write(path, text).unwrap();
}

/// The `res://` path of the scene the object node `name` (under `objects`) is an instance of.
fn instance_of(tscn: &str, name: &str) -> Option<String> {
    let scene = read::parse(tscn).unwrap();
    let (node, _) = scene
        .nodes()
        .find(|(n, _)| n.name == name && n.parent.as_deref() == Some("objects"))?;
    let id = node.instance?;
    scene.ext_resources().find(|e| e.id == id).map(|e| e.path)
}

#[test]
fn res_path_comes_from_the_object_then_its_template_its_tile_and_its_class() {
    let dir = project("objects-respath");
    for scene in ["own", "class", "tile", "template"] {
        write_scene(&dir, &format!("res://scenes/{scene}.tscn"), None);
    }
    std::fs::write(
        dir.join("maps/probe.tiled-project"),
        r##"{"propertyTypes": [{"id": 1, "name": "Spawner", "type": "class", "useAs": ["object"], "color": "#ffa0a0a4", "drawFill": true,
            "members": [{"name": "resPath", "type": "string", "value": "res://scenes/class.tscn"}]}]}"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("maps/things.tsx"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="things" tilewidth="32" tileheight="32" tilecount="4" columns="2">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="1">
  <properties><property name="resPath" value="res://scenes/tile.tscn"/></properties>
 </tile>
</tileset>
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("maps/thing.tx"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<template>
 <tileset firstgid="1" source="things.tsx"/>
 <object name="thing" gid="2" width="32" height="32">
  <properties><property name="resPath" value="res://scenes/template.tscn"/></properties>
 </object>
</template>
"#,
    )
    .unwrap();
    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="7">
 <tileset firstgid="1" source="things.tsx"/>
 <objectgroup id="1" name="objects">
  <object id="1" name="own" type="Spawner" x="0" y="0" width="32" height="32">
   <properties><property name="resPath" value="res://scenes/own.tscn"/></properties>
  </object>
  <object id="2" name="classed" type="Spawner" x="0" y="0" width="32" height="32"/>
  <object id="3" name="tiled" type="Spawner" gid="2" x="0" y="64" width="32" height="32"/>
  <object id="4" name="templated" template="thing.tx" x="32" y="64"/>
  <object id="5" name="overridden" template="thing.tx" x="64" y="64">
   <properties><property name="resPath" value="res://scenes/own.tscn"/></properties>
  </object>
  <object id="6" name="plain" x="96" y="0" width="16" height="16"/>
 </objectgroup>
</map>
"#;
    std::fs::write(dir.join("maps/respath.tmx"), map).unwrap();
    let types = bt_tmx::load_project(&dir.join("maps/probe.tiled-project"))
        .unwrap()
        .value
        .custom_types;
    let mut loader = bt_tmx::Loader::builder()
        .custom_types(Arc::new(types))
        .build();
    let report = export(
        &dir.join("maps/respath.tmx"),
        &mut loader,
        &dir.join("r.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("r.tscn")).unwrap();
    let found: Vec<Option<String>> = [
        "own",
        "classed",
        "tiled",
        "templated",
        "overridden",
        "plain",
    ]
    .iter()
    .map(|n| instance_of(&tscn, n))
    .collect();
    let scene = |n: &str| Some(format!("res://scenes/{n}.tscn"));
    assert_eq!(
        found,
        [
            scene("own"),
            scene("class"),
            scene("tile"),
            scene("template"),
            scene("own"),
            None
        ]
    );
    // The instance keeps the object's name, group and id; the plain object stays an Area2D.
    let s = sections(&tscn);
    assert!(
        s.iter().any(|(h, _)| h.starts_with(
            "[node name=\"own\" parent=\"objects\" groups=[\"Spawner\"] instance=ExtResource("
        )),
        "{tscn}"
    );
    assert_eq!(
        value(node(&s, "own", "objects"), "position"),
        "Vector2(16, 16)",
        "the box's centre"
    );
    assert!(
        s.iter()
            .any(|(h, _)| h == "[node name=\"plain\" type=\"Area2D\" parent=\"objects\"]"),
        "{tscn}"
    );
    assert!(!tscn.contains("metadata/resPath"), "{tscn}");
    assert_eq!((report.counts.objects, report.counts.instances), (6, 5));
    assert!(report.consumed.contains("resPath"));
    assert!(report.notes.is_empty(), "{:?}", report.warnings());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_res_path_that_names_no_scene_keeps_the_typed_node_with_a_warning() {
    let dir = project("objects-respath-missing");
    write_scene(&dir, "res://scenes/real.tscn", None);
    std::fs::write(dir.join("scenes/baked.scn"), b"RSCC").unwrap();
    let object = |id: u32, value: &str| {
        format!(
            r#"<object id="{id}" name="o{id}" x="0" y="0" width="8" height="8"><properties><property name="resPath" {value}/><property name="hp" type="int" value="3"/></properties></object>"#
        )
    };
    let map = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="7">
 <objectgroup id="1" name="objects">
  {}
  {}
  {}
  {}
  {}
  {}
 </objectgroup>
</map>
"#,
        object(1, r#"value="scenes/real.tscn""#),
        object(2, r#"value="res://scenes/nowhere.tscn""#),
        object(3, r#"value="res://scenes/nowhere.tscn""#),
        object(4, r#"type="int" value="3""#),
        object(5, r#"value="res://scenes/baked.scn""#),
        object(6, r#"type="file" value="../scenes/real.tscn""#),
    );
    std::fs::write(dir.join("maps/missing.tmx"), map).unwrap();
    let report = export(
        &dir.join("maps/missing.tmx"),
        &mut loader(&dir),
        &dir.join("m.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("m.tscn")).unwrap();
    let found: Vec<Option<String>> = (1..=6)
        .map(|i| instance_of(&tscn, &format!("o{i}")))
        .collect();
    let real = Some("res://scenes/real.tscn".to_owned());
    assert_eq!(
        found,
        [
            None,
            None,
            None,
            None,
            Some("res://scenes/baked.scn".to_owned()),
            real
        ]
    );
    let s = sections(&tscn);
    assert!(
        s.iter()
            .any(|(h, _)| h == "[node name=\"o2\" type=\"Area2D\" parent=\"objects\"]"),
        "{tscn}"
    );
    let notes: Vec<(&str, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/respath-missing",
                "resPath \"3\" isn't a res://….tscn or .scn path: object 4 stays a plain node"
            ),
            (
                "godot/respath-missing",
                "resPath \"res://scenes/nowhere.tscn\" isn't a file in the Godot project: objects 2 and 3 stay plain nodes"
            ),
            (
                "godot/respath-missing",
                "resPath \"scenes/real.tscn\" isn't a res://….tscn or .scn path: object 1 stays a plain node"
            ),
            (
                "godot/export-undeclared",
                "res://scenes/baked.scn: it is a binary scene, which better-tiled can't read, so on object 5 hp is metadata only"
            ),
            (
                "godot/export-undeclared",
                "res://scenes/real.tscn: its root script has no @export var hp, so on object 6 it is metadata only; Godot drops properties a scene doesn't declare"
            ),
        ]
    );
    assert_eq!(
        report.notes[1].elements,
        [Element::Object(2), Element::Object(3)]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exported_variables_are_read_from_gdscript_text() {
    let source = r#"@tool
extends Node2D
## A chest.

@export var gold: int = 0
@export_range(0, 10) var level := 1
@export
var on_its_own_line: String
@export_group("Loot")
@export_enum("Common", "Rare (ish)") var rarity: int
var plain := 3
@onready var label = $Label
@export_category("Misc")
var after_a_category = 1
@export var with_setter: int:
	set(v):
		with_setter = v
# @export var commented
var s = "@export var in_a_string"
var t = """
@export var in_a_long_string
"""
class Inner:
	@export var inner: int
@export_flags("A", "B",
	"C") var flags := 0
@export_storage var stored := 1
@export var target: Node2D
@export var path: NodePath
@export var inferred_path := ^"../x"
@export var items: Array[Node] = []
"#;
    let exports = gd_exports(source);
    let types: BTreeMap<&str, Option<&str>> = exports
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_deref()))
        .collect();
    let expected = BTreeMap::from([
        ("gold", Some("int")),
        ("level", None),
        ("on_its_own_line", Some("String")),
        ("rarity", Some("int")),
        ("with_setter", Some("int")),
        ("flags", None),
        ("stored", None),
        ("target", Some("Node2D")),
        ("path", Some("NodePath")),
        ("inferred_path", Some("NodePath")),
        ("items", Some("Array[Node]")),
    ]);
    assert_eq!(types, expected);
}

#[test]
fn a_scenes_exports_come_from_its_root_script_built_in_script_or_base_scene() {
    let dir = std::env::temp_dir().join(format!("bt-godot-objects-exports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("chest.gd"),
        "extends Node2D\n@export var gold: int\n",
    )
    .unwrap();
    write_scene(&dir, "res://chest.tscn", Some("res://chest.gd"));
    // Inherits from chest.tscn, so has its script.
    std::fs::write(dir.join("big.tscn"), "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://chest.tscn\" id=\"1_a\"]\n\n[node name=\"Big\" instance=ExtResource(\"1_a\")]\n").unwrap();
    std::fs::write(
        dir.join("builtin.tscn"),
        "[gd_scene format=3]\n\n[sub_resource type=\"GDScript\" id=\"GDScript_1\"]\nscript/source = \"extends Node2D\n@export var hp := 3\n\"\n\n[node name=\"B\" type=\"Node2D\"]\nscript = SubResource(\"GDScript_1\")\n",
    )
    .unwrap();
    write_scene(&dir, "res://sharp.tscn", Some("res://Sharp.cs"));
    write_scene(&dir, "res://bare.tscn", None);
    write_scene(&dir, "res://lost.tscn", Some("res://gone.gd"));
    let known = |names: &[(&str, &str)]| {
        Exports::Known(
            names
                .iter()
                .map(|(n, t)| (n.to_string(), Some(t.to_string())))
                .collect(),
        )
    };
    assert_eq!(
        scene_exports(&dir, "res://chest.tscn"),
        known(&[("gold", "int")])
    );
    assert_eq!(
        scene_exports(&dir, "res://big.tscn"),
        known(&[("gold", "int")])
    );
    assert_eq!(
        scene_exports(&dir, "res://builtin.tscn"),
        Exports::Known(BTreeMap::from([("hp".to_owned(), None)]))
    );
    assert_eq!(scene_exports(&dir, "res://bare.tscn"), known(&[]));
    assert!(
        matches!(scene_exports(&dir, "res://sharp.tscn"), Exports::Unknown(why) if why.contains("C#"))
    );
    assert!(
        matches!(scene_exports(&dir, "res://lost.tscn"), Exports::Unknown(why) if why.contains("can't be read"))
    );
    assert!(
        matches!(scene_exports(&dir, "res://baked.scn"), Exports::Unknown(why) if why.contains("binary"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

const INSTANCES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="5">
 <objectgroup id="1" name="objects">
  <object id="1" name="chest" x="32" y="32" width="32" height="16">
   <properties>
    <property name="gold" type="int" value="5"/>
    <property name="note" value="hello"/>
    <property name="resPath" value="res://scenes/chest.tscn"/>
    <property name="target" type="object" value="2"/>
   </properties>
  </object>
  <object id="2" name="spawn" x="100" y="100">
   <point/>
  </object>
  <object id="3" name="lost" x="0" y="0" width="16" height="16">
   <properties><property name="resPath" value="res://scenes/nowhere.tscn"/></properties>
  </object>
  <object id="4" name="turned" x="64" y="0" width="32" height="16" rotation="90">
   <properties><property name="gold" type="int" value="7"/><property name="resPath" value="res://scenes/chest.tscn"/></properties>
  </object>
 </objectgroup>
</map>
"#;

/// What Godot made of the instances.
const INSTANCE_CHECK: &str = r#"extends SceneTree

func _initialize() -> void:
	var scene := (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	root.add_child(scene)
	var chest := scene.get_node("objects/chest") as Node2D
	var turned := scene.get_node("objects/turned") as Node2D
	var target: Variant = chest.get("target")
	var result := {
		"chest": {"script": chest.get_script() != null, "gold": chest.get("gold"), "note": chest.get("note"), "meta": [chest.get_meta("gold"), chest.get_meta("note")], "target": String(chest.get_path_to(target as Node)) if target is Node else str(target), "position": [chest.position.x, chest.position.y]},
		"turned": {"gold": turned.get("gold"), "rotation": turned.rotation, "position": [turned.position.x, turned.position.y]},
		"lost": scene.get_node("objects/lost").get_class(),
	}
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	root.remove_child(scene)
	scene.free()
	quit()
"#;

#[test]
fn godot_sets_the_variables_an_instances_script_exports() {
    let dir = project("objects-instances");
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("scenes/chest.gd"),
        "extends Node2D\n\n@export var gold: int = 0\n@export var target: Node2D\n",
    )
    .unwrap();
    write_scene(
        &dir,
        "res://scenes/chest.tscn",
        Some("res://scenes/chest.gd"),
    );
    std::fs::write(dir.join("maps/instances.tmx"), INSTANCES).unwrap();
    let report = export(
        &dir.join("maps/instances.tmx"),
        &mut loader(&dir),
        &dir.join("levels/instances.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("levels/instances.tscn")).unwrap();
    let s = sections(&tscn);
    assert!(s.iter().any(|(h, _)| h.starts_with("[node name=\"chest\" parent=\"objects\" node_paths=PackedStringArray(\"target\") instance=ExtResource(")), "{tscn}");
    let chest = node(&s, "chest", "objects");
    assert_eq!(
        (
            value(chest, "gold"),
            value(chest, "target"),
            value(chest, "metadata/note")
        ),
        ("5", r#"NodePath("../spawn")"#, "\"hello\"")
    );
    assert_eq!(
        value(chest, "metadata/gold"),
        "5",
        "set directly and as metadata"
    );
    assert!(!chest.iter().any(|l| l.starts_with("note = ")));
    let notes: Vec<(&str, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/respath-missing",
                "resPath \"res://scenes/nowhere.tscn\" isn't a file in the Godot project: object 3 stays a plain node"
            ),
            (
                "godot/export-undeclared",
                "res://scenes/chest.tscn: its root script has no @export var note, so on object 1 it is metadata only; Godot drops properties a scene doesn't declare"
            ),
            (
                "godot/respath-rotated",
                "object 4 is turned: its scene instance goes at the centre of its turned box, turned with it; Tiled's exporter puts it at the centre of the unturned box, unturned"
            ),
        ]
    );
    let Some(godot) = godot() else { return };
    let (_, stderr) = run_godot(&godot, &dir, &["--import"]);
    assert!(!stderr.contains("ERROR"), "{stderr}");
    std::fs::write(dir.join("check.gd"), INSTANCE_CHECK).unwrap();
    let (stdout, stderr) = run_godot(
        &godot,
        &dir,
        &[
            "--script",
            "res://check.gd",
            "--",
            "res://levels/instances.tscn",
        ],
    );
    let result = json_between_markers(&stdout, &stderr);
    assert_eq!(
        result["chest"],
        json!({"script": true, "gold": 5, "note": null, "meta": [5, "hello"], "target": "../spawn", "position": [48.0, 40.0]}),
        "{stderr}"
    );
    let turned = &result["turned"];
    assert_eq!(turned["gold"], 7);
    assert!(
        (turned["rotation"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
        "{turned}"
    );
    assert_eq!(
        turned["position"],
        json!([56.0, 16.0]),
        "the turned box's centre"
    );
    assert_eq!(result["lost"], "Area2D");
    assert!(stderr.trim().is_empty(), "{stderr}");
    std::fs::remove_dir_all(&dir).ok();
}
