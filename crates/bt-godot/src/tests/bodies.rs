//! `godotNode` (objects.rs): the node type it makes of shapes and tile objects, the collision
//! bits, what keeps today's node, and tile objects' collision shapes as children: named and
//! numbered so that a shape added in Tiled moves no other, their notes said once, and (headless)
//! where Godot puts them, against Tiled's drawing of a tile's collision on a tile object.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bt_core::{LayerKind, MapObject, Shape};
use glam::DVec2;

use super::super::grid::Grid;
use super::super::read;
use super::geometry as object_geom;
use super::oracle::{json_between_markers, run_godot};
use super::support::*;
use super::*;

/// The type in the header of node `name` under `parent`.
fn kind_of<'s>(s: &'s [(String, Vec<String>)], name: &str, parent: &str) -> &'s str {
    let (header, _) = s
        .iter()
        .find(|(h, _)| {
            h.starts_with(&format!("[node name=\"{name}\" "))
                && h.contains(&format!("parent=\"{parent}\""))
        })
        .unwrap_or_else(|| panic!("no node {parent}/{name}"));
    header
        .split("type=\"")
        .nth(1)
        .and_then(|t| t.split('"').next())
        .unwrap_or("")
}

fn has(lines: &[String], key: &str) -> bool {
    lines.iter().any(|l| l.starts_with(&format!("{key} = ")))
}

/// Shapes of every kind with each `godotNode`, the collision bits, and what keeps the usual node.
const SHAPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="8" height="8" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="14">
 <objectgroup id="1" name="things">
  <object id="1" name="static" x="0" y="0" width="32" height="16">
   <properties><property name="godotCollisionLayer" type="int" value="4"/><property name="godotCollisionMask" type="int" value="6"/><property name="godotNode" value="StaticBody2D"/></properties>
  </object>
  <object id="2" name="animatable" x="32" y="0" width="16" height="16"><properties><property name="godotNode" value="AnimatableBody2D"/></properties><ellipse/></object>
  <object id="3" name="character" x="64" y="0" width="16" height="32"><properties><property name="godotCollisionLayer" type="int" value="1"/><property name="godotNode" value="CharacterBody2D"/></properties><capsule/></object>
  <object id="4" name="rigid" x="96" y="0"><properties><property name="godotNode" value="RigidBody2D"/></properties><polygon points="0,0 16,0 8,12"/></object>
  <object id="5" name="area" x="0" y="32" width="8" height="8"><properties><property name="godotCollisionMask" type="int" value="2"/></properties></object>
  <object id="6" name="plain" x="32" y="32" width="8" height="8"><properties><property name="godotCollisionLayer" type="int" value="3"/><property name="godotNode" value="Node2D"/></properties></object>
  <object id="7" name="walled" type="Wall" x="64" y="32" width="8" height="8"/>
  <object id="8" name="unknown" x="0" y="64" width="8" height="8"><properties><property name="godotNode" value="Static"/></properties></object>
  <object id="9" name="number" x="0" y="64" width="8" height="8"><properties><property name="godotNode" type="int" value="5"/></properties></object>
  <object id="10" name="empty" x="0" y="64" width="8" height="8"><properties><property name="godotNode" value=""/></properties></object>
  <object id="11" name="point" x="0" y="96"><properties><property name="godotCollisionLayer" type="int" value="2"/><property name="godotNode" value="StaticBody2D"/></properties><point/></object>
  <object id="12" name="bad" x="0" y="128" width="8" height="8"><properties><property name="godotCollisionLayer" type="int" value="-1"/><property name="godotCollisionMask" type="float" value="2.5"/><property name="godotNode" value="StaticBody2D"/></properties></object>
  <object id="13" name="instance" x="0" y="160" width="8" height="8"><properties><property name="godotNode" value="StaticBody2D"/><property name="resPath" value="res://scenes/thing.tscn"/></properties></object>
 </objectgroup>
</map>
"#;

#[test]
fn godot_node_picks_the_node_of_shapes_and_their_collision_bits() {
    let dir = project("bodies-shapes");
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("scenes/thing.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Thing\" type=\"Node2D\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("maps/bodies.tiled-project"),
        r##"{"propertyTypes": [{"id": 1, "name": "Wall", "type": "class", "useAs": ["object"], "color": "#ffa0a0a4", "drawFill": true,
            "members": [{"name": "godotNode", "type": "string", "value": "StaticBody2D"}]}]}"##,
    )
    .unwrap();
    std::fs::write(dir.join("maps/shapes.tmx"), SHAPES).unwrap();
    let types = bt_tmx::load_project(&dir.join("maps/bodies.tiled-project"))
        .unwrap()
        .value
        .custom_types;
    let mut loader = bt_tmx::Loader::builder()
        .custom_types(Arc::new(types))
        .build();
    let report = export(
        &dir.join("maps/shapes.tmx"),
        &mut loader,
        &dir.join("shapes.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("shapes.tscn")).unwrap();
    let s = sections(&tscn);

    // Each type, its shape kept under it as an area's is.
    let kinds = [
        "static",
        "animatable",
        "character",
        "rigid",
        "area",
        "plain",
        "walled",
    ]
    .map(|n| kind_of(&s, n, "things"));
    assert_eq!(
        kinds,
        [
            "StaticBody2D",
            "AnimatableBody2D",
            "CharacterBody2D",
            "RigidBody2D",
            "Area2D",
            "Node2D",
            "StaticBody2D"
        ]
    );
    for (name, shape) in [
        ("static", "CollisionShape2D"),
        ("animatable", "CollisionShape2D"),
        ("character", "CollisionShape2D"),
        ("rigid", "CollisionPolygon2D"),
        ("plain", "CollisionShape2D"),
    ] {
        assert_eq!(
            kind_of(&s, "Shape", &format!("things/{name}")),
            shape,
            "{name}"
        );
    }
    assert!(
        value(node(&s, "Shape", "things/static"), "shape")
            .starts_with("SubResource(\"RectangleShape2D_")
    );
    assert!(
        value(node(&s, "Shape", "things/animatable"), "shape")
            .starts_with("SubResource(\"CircleShape2D_")
    );
    assert!(
        value(node(&s, "Shape", "things/character"), "shape")
            .starts_with("SubResource(\"CapsuleShape2D_")
    );

    // The bits, after the placement; Godot's default 1 left out; bodies and areas only.
    let body = node(&s, "static", "things");
    assert_eq!(
        body[..3],
        [
            "position = Vector2(0, 0)",
            "collision_layer = 4",
            "collision_mask = 6"
        ]
    );
    assert!(!has(node(&s, "character", "things"), "collision_layer"));
    let area = node(&s, "area", "things");
    assert_eq!(
        (value(area, "collision_mask"), has(area, "collision_layer")),
        ("2", false)
    );
    let plain = node(&s, "plain", "things");
    assert_eq!(
        value(plain, "metadata/godotCollisionLayer"),
        "3",
        "a Node2D has no bits: the setting stays metadata"
    );
    assert!(!has(plain, "collision_layer") && !has(plain, "metadata/godotNode"));
    let bad = node(&s, "bad", "things");
    assert!(
        !has(bad, "collision_layer")
            && !has(bad, "collision_mask")
            && !has(bad, "metadata/godotCollisionLayer"),
        "{bad:?}"
    );

    // Values naming no type keep the area; an empty one says nothing.
    for name in ["unknown", "number", "empty"] {
        assert_eq!(kind_of(&s, name, "things"), "Area2D", "{name}");
        assert!(
            !has(node(&s, name, "things"), "metadata/godotNode"),
            "{name}"
        );
    }
    // A point has no area, and an instance its scene's type: the names stay metadata.
    let point = node(&s, "point", "things");
    assert_eq!(kind_of(&s, "point", "things"), "Marker2D");
    assert_eq!(
        (
            value(point, "metadata/godotNode"),
            value(point, "metadata/godotCollisionLayer")
        ),
        ("\"StaticBody2D\"", "2")
    );
    assert!(
        s.iter().any(|(h, _)| h
            .starts_with("[node name=\"instance\" parent=\"things\" instance=ExtResource(")),
        "{tscn}"
    );
    assert_eq!(
        value(node(&s, "instance", "things"), "metadata/godotNode"),
        "\"StaticBody2D\""
    );

    let types = "StaticBody2D, AnimatableBody2D, CharacterBody2D, RigidBody2D, Area2D or Node2D";
    let notes: Vec<(&str, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/property-type",
                "object 12: godotCollisionLayer takes Godot's bits, a whole number from 0 to 4294967295; it is left out"
            ),
            (
                "godot/property-type",
                "object 12: godotCollisionMask takes Godot's bits, a whole number from 0 to 4294967295; it is left out"
            ),
            (
                "godot/node-type",
                format!("godotNode \"Static\" isn't {types}: object 8 keeps its usual node")
                    .as_str()
            ),
            (
                "godot/node-type",
                format!("godotNode 5 isn't {types}: object 9 keeps its usual node").as_str()
            ),
        ],
        "no export-undeclared note for the instance's godotNode"
    );
    for name in ["godotNode", "godotCollisionLayer", "godotCollisionMask"] {
        assert!(report.consumed.contains(name), "{name}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A tile with shapes of every sort: a rectangle, a named polygon, a point (no area), a shape out
/// of physics and a one-way one. Tile 1 has none.
const CRATES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="crates" tilewidth="32" tileheight="32" tilecount="4" columns="2">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="0">
  <objectgroup draworder="index">
   <object id="1" x="4" y="16" width="24" height="16"/>
   <object id="2" name="lid" x="0" y="0"><polygon points="0,0 32,0 16,8"/></object>
   <object id="3" x="8" y="8"><point/></object>
   <object id="4" x="0" y="0" width="8" height="8"><properties><property name="godotCollision" type="bool" value="false"/></properties></object>
   <object id="5" x="0" y="0" width="32" height="4"><properties><property name="godotOneWay" type="bool" value="true"/><property name="godotOneWayMargin" type="float" value="2"/></properties></object>
   <object id="6" x="28" y="4" width="4" height="8" rotation="30"/>
  </objectgroup>
 </tile>
</tileset>
"#;

/// Tile objects: a body flipped and drawn twice as wide, one whose tile has no shapes, a plain
/// node and a plain sprite.
const TILE_OBJECTS: &str = r#" <objectgroup id="2" name="things">
  <object id="1" name="crate" gid="2147483649" x="64" y="96" width="64" height="32"><properties><property name="godotNode" value="StaticBody2D"/></properties></object>
  <object id="2" name="box" gid="2" x="0" y="32" width="32" height="32"><properties><property name="godotNode" value="StaticBody2D"/></properties></object>
  <object id="3" name="deco" gid="1" x="0" y="64" width="32" height="32"><properties><property name="godotNode" value="Node2D"/></properties></object>
  <object id="4" name="sprite" gid="1" x="0" y="96" width="32" height="32"/>
 </objectgroup>
"#;

const PAINTED: &str = " <layer id=\"1\" name=\"cells\" width=\"8\" height=\"8\">\n  <data encoding=\"csv\">\n1,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0,\n0,0,0,0,0,0,0,0\n</data>\n </layer>\n";

fn crates_map(layers: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" orientation=\"orthogonal\" renderorder=\"right-down\" width=\"8\" height=\"8\" tilewidth=\"32\" tileheight=\"32\" infinite=\"0\" nextlayerid=\"3\" nextobjectid=\"7\">\n <tileset firstgid=\"1\" source=\"crates.tsx\"/>\n{layers}</map>\n"
    )
}

/// A project for Godot 4.7, so nodes get `unique_id`s.
fn godot_47(dir: &Path) {
    std::fs::write(dir.join("project.godot"), "config_version=5\n\n[application]\n\nconfig/name=\"bt-test\"\nconfig/features=PackedStringArray(\"4.7\", \"Forward Plus\")\n").unwrap();
}

/// Each child of node `parent`: (name, type, unique_id).
fn children(tscn: &str, parent: &str) -> Vec<(String, String, Option<i32>)> {
    let scene = read::parse(tscn).unwrap();
    scene
        .nodes()
        .filter(|(n, _)| n.parent.as_deref() == Some(parent))
        .map(|(n, _)| {
            (
                n.name.clone(),
                n.class.clone().unwrap_or_default(),
                n.unique_id,
            )
        })
        .collect()
}

#[test]
fn tile_objects_given_a_godot_node_hold_their_sprite_and_their_tiles_shapes() {
    let dir = project("bodies-tiles");
    godot_47(&dir);
    std::fs::write(dir.join("maps/crates.tsx"), CRATES).unwrap();
    std::fs::write(dir.join("maps/crates.tmx"), crates_map(TILE_OBJECTS)).unwrap();
    let target = dir.join("crates.tscn");
    let report = export(
        &dir.join("maps/crates.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);

    // The body is placed and turned as the sprite was; the sprite under it draws the tile.
    assert!(
        s.iter().any(|(h, _)| h
            == "[node name=\"crate\" type=\"StaticBody2D\" parent=\"things\" unique_id=536870913]"),
        "{tscn}"
    );
    assert_eq!(
        node(&s, "crate", "things")[..1],
        ["position = Vector2(64, 96)"]
    );
    assert!(
        !has(node(&s, "crate", "things"), "texture")
            && !has(node(&s, "crate", "things"), "metadata/godotNode")
    );
    let sprite = node(&s, "Sprite", "things/crate");
    assert!(value(sprite, "texture").starts_with("ExtResource("));
    assert_eq!(
        [
            value(sprite, "scale"),
            value(sprite, "offset"),
            value(sprite, "flip_h")
        ],
        ["Vector2(2, 1)", "Vector2(0, -32)", "true"]
    );
    assert_eq!(value(sprite, "region_rect"), "Rect2(0, 0, 32, 32)");

    // The shapes, flipped in the object's box and scaled with it; the point, and the shape out of
    // physics, aren't there. Named like objects, the lowest id keeping `Shape`.
    let kids = children(&tscn, "things/crate");
    let names: Vec<(&str, &str)> = kids
        .iter()
        .map(|(n, k, _)| (n.as_str(), k.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Sprite", "Sprite2D"),
            ("Shape", "CollisionPolygon2D"),
            ("lid", "CollisionPolygon2D"),
            ("Shape_5", "CollisionPolygon2D"),
            ("Shape_6", "CollisionPolygon2D")
        ]
    );
    assert_eq!(
        value(node(&s, "Shape", "things/crate"), "polygon"),
        "PackedVector2Array(56, -16, 8, -16, 8, 0, 56, 0)"
    );
    assert_eq!(
        value(node(&s, "lid", "things/crate"), "polygon"),
        "PackedVector2Array(64, -32, 0, -32, 32, -24)"
    );
    let one_way = node(&s, "Shape_5", "things/crate");
    assert_eq!(
        one_way,
        [
            "polygon = PackedVector2Array(64, -32, 0, -32, 0, -28, 64, -28)",
            "one_way_collision = true",
            "one_way_collision_margin = 2.0"
        ]
    );
    // Helpers: hashed ids, all different.
    let mut ids: Vec<i32> = kids.iter().map(|(_, _, id)| id.unwrap()).collect();
    assert!(ids.iter().all(|&id| id >= 3 << 29), "{ids:?}");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), kids.len());

    // A plain node holds them too (for their geometry); a tile object without godotNode stays a
    // sprite.
    assert_eq!(kind_of(&s, "deco", "things"), "Node2D");
    assert_eq!(children(&tscn, "things/deco").len(), 5);
    assert_eq!(kind_of(&s, "sprite", "things"), "Sprite2D");
    assert!(value(node(&s, "sprite", "things"), "texture").starts_with("ExtResource("));
    assert!(children(&tscn, "things/sprite").is_empty());

    let notes: Vec<(&str, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/body-no-shape",
                "object 2 collides with nothing: its tile has no collision shapes; draw some in Tiled's tile collision editor, or give it the godotNode Node2D"
            ),
            (
                "godot/tile-collision-kind",
                "tile collision shapes that are points, polylines, text or tiles have no area, and Godot's physics layers take only polygons: they are left out: draw them as rectangles, ellipses or polygons (tileset crates: tile 0)"
            ),
        ]
    );
    for name in [
        "godotCollision",
        "godotOneWay",
        "godotOneWayMargin",
        "godotNode",
    ] {
        assert!(report.consumed.contains(name), "{name}");
    }

    // A shape added to the tile in Tiled (with a higher id) renames and renumbers no other.
    let added = CRATES.replace(
        "  </objectgroup>",
        "   <object id=\"7\" x=\"0\" y=\"28\" width=\"8\" height=\"4\"/>\n  </objectgroup>",
    );
    std::fs::write(dir.join("maps/crates.tsx"), added).unwrap();
    export(
        &dir.join("maps/crates.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let again = children(&std::fs::read_to_string(&target).unwrap(), "things/crate");
    assert_eq!(again[..kids.len()], kids[..]);
    assert_eq!(
        (again.len(), again[kids.len()].0.as_str()),
        (kids.len() + 1, "Shape_7")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_tiles_shapes_are_reported_once_whether_its_tileset_is_built_before_after_or_never() {
    let dir = project("bodies-notes");
    std::fs::write(dir.join("maps/crates.tsx"), CRATES).unwrap();
    // Tile 0 painted before the objects, after them, or not at all.
    for (i, layers) in [
        format!("{PAINTED}{TILE_OBJECTS}"),
        format!("{TILE_OBJECTS}{PAINTED}"),
        TILE_OBJECTS.to_owned(),
    ]
    .iter()
    .enumerate()
    {
        std::fs::write(dir.join("maps/notes.tmx"), crates_map(layers)).unwrap();
        let report = export(
            &dir.join("maps/notes.tmx"),
            &mut loader(&dir),
            &dir.join(format!("notes{i}.tscn")),
            &Options::default(),
        )
        .unwrap();
        let mut codes: Vec<&str> = report.notes.iter().map(|n| n.code.as_str()).collect();
        codes.sort_unstable();
        assert_eq!(
            codes,
            ["godot/body-no-shape", "godot/tile-collision-kind"],
            "case {i}: {:?}",
            report.warnings()
        );
        let kind = report
            .notes
            .iter()
            .find(|n| n.code.as_str() == "godot/tile-collision-kind")
            .unwrap();
        assert!(
            kind.message.ends_with("(tileset crates: tile 0)"),
            "case {i}: {}",
            kind.message
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// Tile bodies of every type, turned, flipped and scaled, in a layer with an offset.
const ORACLE_OBJECTS: &str = r#" <objectgroup id="2" name="things" offsetx="5" offsety="-3">
  <object id="1" name="static" gid="1" x="64" y="96" width="64" height="32"><properties><property name="godotCollisionLayer" type="int" value="5"/><property name="godotCollisionMask" type="int" value="12"/><property name="godotNode" value="StaticBody2D"/></properties></object>
  <object id="2" name="turned" gid="2147483649" x="160" y="160" width="32" height="48" rotation="30"><properties><property name="godotNode" value="CharacterBody2D"/></properties></object>
  <object id="3" name="upside" gid="1073741825" x="96" y="200" width="32" height="32" rotation="-45"><properties><property name="godotNode" value="Area2D"/></properties></object>
  <object id="4" name="both" gid="3221225473" x="200" y="64" width="16" height="16"><properties><property name="godotNode" value="RigidBody2D"/></properties></object>
  <object id="5" name="sized" gid="1" x="0" y="32"><properties><property name="godotNode" value="AnimatableBody2D"/></properties></object>
  <object id="6" name="plain" gid="1" x="32" y="240" width="32" height="32" rotation="90"><properties><property name="godotNode" value="Node2D"/></properties></object>
 </objectgroup>
"#;

/// What Godot made of the tile bodies of each scene it is given.
const CHECK: &str = r#"extends SceneTree

func _initialize() -> void:
	var result := {}
	for path: String in OS.get_cmdline_user_args():
		var scene := (load(path) as PackedScene).instantiate()
		root.add_child(scene)
		var objects := {}
		for node: Node in scene.get_node("things").get_children():
			var d := {"class": node.get_class()}
			if node is CollisionObject2D:
				d["bits"] = [(node as CollisionObject2D).collision_layer, (node as CollisionObject2D).collision_mask]
			var shapes := {}
			for child: Node in node.get_children():
				if child is CollisionPolygon2D:
					var polygon := child as CollisionPolygon2D
					var points := []
					for p: Vector2 in polygon.polygon:
						var g := polygon.global_transform * p
						points.append([g.x, g.y])
					shapes[String(child.name)] = {"points": points, "one_way": polygon.one_way_collision, "margin": polygon.one_way_collision_margin}
				elif child is Sprite2D:
					var sprite := child as Sprite2D
					var rect := sprite.get_rect()
					var a := sprite.to_global(rect.position)
					var b := sprite.to_global(rect.end)
					d["sprite"] = [a.x, a.y, b.x, b.y]
			d["shapes"] = shapes
			objects[String(node.name)] = d
		result[path] = objects
		root.remove_child(scene)
		scene.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// Where Tiled draws the physics shapes of the tile of object `o` (Tiled's
/// `paintTileCollisionShapes`, worked out here on its own): each shape's outline on the tile's
/// image, turned around its position, then the image over the object's box, flipped in it, the
/// box turned with the object (object_geom), less the grid's shift. By the node name each wants.
fn tiled_shapes(
    map: &bt_core::Map,
    o: &MapObject,
    offset: DVec2,
    shift: DVec2,
) -> Vec<(String, Vec<DVec2>)> {
    let cell = o.tile.unwrap();
    let ts = cell.slot().and_then(|slot| map.tileset(slot)).unwrap();
    let image = ts.tile_pixel_size(cell.tile()).as_dvec2();
    let frame = object_geom::frame(map, o, offset);
    let (min, max) = object_geom::local_box(map, o);
    let on_box = |p: DVec2| {
        let flipped = DVec2::new(
            if cell.flip().contains(bt_core::Flip::H) {
                image.x - p.x
            } else {
                p.x
            },
            if cell.flip().contains(bt_core::Flip::V) {
                image.y - p.y
            } else {
                p.y
            },
        );
        frame.to_world(min + flipped * (max - min) / image) - shift
    };
    let mut out = Vec::new();
    for c in ts
        .tile(cell.tile())
        .unwrap()
        .collision
        .iter()
        .flat_map(|g| g.layer.objects.iter())
    {
        let local = match &c.shape {
            Shape::Rectangle => vec![
                DVec2::ZERO,
                DVec2::new(c.size.x, 0.0),
                c.size,
                DVec2::new(0.0, c.size.y),
            ],
            Shape::Polygon(points) => points.clone(),
            _ => continue,
        };
        if c.properties.contains_key("godotCollision") {
            continue;
        }
        let name = if !c.name.is_empty() {
            c.name.clone()
        } else if out.iter().any(|(n, _): &(String, _)| n == "Shape") {
            format!("Shape_{}", c.id)
        } else {
            "Shape".to_owned()
        };
        out.push((
            name,
            local
                .into_iter()
                .map(|p| on_box(c.pos + object_geom::rotate(p, c.rotation)))
                .collect(),
        ));
    }
    out
}

fn point(v: &Value) -> DVec2 {
    DVec2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

#[test]
fn godot_puts_a_tile_bodys_shapes_where_tiled_draws_them() {
    let dir = project("bodies-oracle");
    std::fs::write(dir.join("maps/crates.tsx"), CRATES).unwrap();
    std::fs::write(dir.join("maps/ortho.tmx"), crates_map(ORACLE_OBJECTS)).unwrap();
    let iso = crates_map(ORACLE_OBJECTS)
        .replace("orientation=\"orthogonal\"", "orientation=\"isometric\"")
        .replace(
            "tilewidth=\"32\" tileheight=\"32\" infinite",
            "tilewidth=\"64\" tileheight=\"32\" infinite",
        );
    std::fs::write(dir.join("maps/iso.tmx"), iso).unwrap();
    let mut cases = Vec::new();
    for name in ["ortho", "iso"] {
        let map_path = dir.join(format!("maps/{name}.tmx"));
        let mut loader = loader(&dir);
        let report = export(
            &map_path,
            &mut loader,
            &dir.join(format!("levels/{name}.tscn")),
            &Options::default(),
        )
        .unwrap();
        assert!(
            report
                .notes
                .iter()
                .all(|n| ["godot/tile-collision-kind", "godot/grid-shift"]
                    .contains(&n.code.as_str())),
            "{:?}",
            report.warnings()
        );
        let loaded = loader.load_map(&map_path).unwrap().value;
        cases.push((
            format!("res://levels/{name}.tscn"),
            crate::resolve::resolved(&loaded, loader.custom_types()),
        ));
    }
    let Some(godot) = godot() else { return };
    let (_, stderr) = run_godot(&godot, &dir, &["--import"]);
    assert!(!stderr.contains("ERROR"), "{stderr}");
    std::fs::write(dir.join("check.gd"), CHECK).unwrap();
    let mut args = vec!["--script", "res://check.gd", "--"];
    args.extend(cases.iter().map(|(res, _)| res.as_str()));
    let (stdout, stderr) = run_godot(&godot, &dir, &args);
    let result = json_between_markers(&stdout, &stderr);
    assert!(stderr.trim().is_empty(), "{stderr}");

    let classes = HashMap::from([
        ("static", "StaticBody2D"),
        ("turned", "CharacterBody2D"),
        ("upside", "Area2D"),
        ("both", "RigidBody2D"),
        ("sized", "AnimatableBody2D"),
        ("plain", "Node2D"),
    ]);
    let mut checked = 0;
    for (res, map) in &cases {
        let shift = Grid::of(map).unwrap().shift();
        let layer = map.layers.iter().find(|l| l.name == "things").unwrap();
        let LayerKind::Object(objects) = &layer.kind else {
            panic!()
        };
        let offset = layer.offset;
        for o in objects.objects.iter() {
            let got = &result[res][&o.name];
            let at = format!("{res} {}", o.name);
            assert_eq!(got["class"], classes[o.name.as_str()], "{at}");
            // The sprite still covers the object's box.
            let frame = object_geom::frame(map, o, offset);
            let (min, max) = object_geom::local_box(map, o);
            let corners = [frame.to_world(min) - shift, frame.to_world(max) - shift];
            let sprite = &got["sprite"];
            let drawn = [
                DVec2::new(sprite[0].as_f64().unwrap(), sprite[1].as_f64().unwrap()),
                DVec2::new(sprite[2].as_f64().unwrap(), sprite[3].as_f64().unwrap()),
            ];
            assert!(
                drawn
                    .iter()
                    .zip(&corners)
                    .all(|(a, b)| a.distance(*b) < 0.01),
                "{at}: sprite at {drawn:?}, Tiled's box {corners:?}"
            );
            let want = tiled_shapes(map, o, offset, shift);
            let shapes = got["shapes"].as_object().unwrap();
            assert_eq!(shapes.len(), want.len(), "{at}: {shapes:?}");
            for (name, points) in &want {
                let polygon: Vec<DVec2> = got["shapes"][name]["points"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{at}: no {name}"))
                    .iter()
                    .map(point)
                    .collect();
                assert_eq!(polygon.len(), points.len(), "{at} {name}");
                assert!(
                    polygon
                        .iter()
                        .zip(points)
                        .all(|(a, b)| a.distance(*b) < 0.01),
                    "{at} {name}: Godot's {polygon:?}, Tiled's {points:?}"
                );
                checked += 1;
            }
            let one_way = &got["shapes"]["Shape_5"];
            assert_eq!(
                (one_way["one_way"].as_bool(), one_way["margin"].as_f64()),
                (Some(true), Some(2.0)),
                "{at}"
            );
            assert_eq!(got["shapes"]["Shape"]["one_way"], false, "{at}");
        }
        assert_eq!(
            result[res]["static"]["bits"],
            serde_json::json!([5, 12]),
            "{res}"
        );
        assert_eq!(
            result[res]["turned"]["bits"],
            serde_json::json!([1, 1]),
            "{res}"
        );
    }
    assert_eq!(checked, 2 * 6 * 4);
    std::fs::remove_dir_all(&dir).ok();
}
