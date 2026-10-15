//! The layer and map conventions (docs/design/12-godot-export.md): `noExport`, `tilesetOnly`, `ySortEnabled`,
//! `zIndex` and `godotYSortOrigin`, consumed rather than written as metadata, and the notes on
//! where they differ from Tiled's exporter.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use glam::DVec2;
use serde_json::Value;

use super::super::report::{Element, Report, Severity};
use super::super::{Options, export, read};
use super::oracle::{json_between_markers, run_godot};
use super::support::*;

/// Each convention in each place it can go, and where it can't: on the map, tile layers, a
/// group and its object and image layers, a parallax layer, and layers left out (one sharing a
/// name with a layer that stays, and one painted with a tileset nothing else uses).
const PROBE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="2" height="1" tilewidth="32" tileheight="32" infinite="0" nextlayerid="17" nextobjectid="2">
 <properties>
  <property name="intent" value="kept"/>
  <property name="ySortEnabled" type="bool" value="true"/>
 </properties>
 <tileset firstgid="1" source="tiles.tsx"/>
 <tileset firstgid="5" name="hidden_things" tilewidth="32" tileheight="32" tilecount="1" columns="0">
  <grid orientation="orthogonal" width="1" height="1"/>
  <tile id="0">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
 </tileset>
 <tileset firstgid="6" name="palette_things" tilewidth="32" tileheight="32" tilecount="1" columns="0">
  <grid orientation="orthogonal" width="1" height="1"/>
  <tile id="0">
   <image source="tiles.png" width="64" height="64"/>
  </tile>
 </tileset>
 <layer id="1" name="sorted" width="2" height="1">
  <properties>
   <property name="godotYSortOrigin" type="int" value="8"/>
   <property name="keep" value="yes"/>
   <property name="ySortEnabled" type="bool" value="true"/>
   <property name="zIndex" type="int" value="3"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
 <layer id="2" name="tiled_false" width="2" height="1">
  <properties>
   <property name="ySortEnabled" type="bool" value="false"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
 <layer id="3" name="high" width="2" height="1">
  <properties>
   <property name="zIndex" type="int" value="5000"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
 <layer id="4" name="low" width="2" height="1">
  <properties>
   <property name="zIndex" type="int" value="-9000"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
 <layer id="5" name="dup" width="2" height="1">
  <properties>
   <property name="noExport" type="bool" value="true"/>
  </properties>
  <data encoding="csv">5,0</data>
 </layer>
 <layer id="6" name="palette" width="2" height="1">
  <properties>
   <property name="tilesetOnly" type="bool" value="true"/>
  </properties>
  <data encoding="csv">6,0</data>
 </layer>
 <group id="7" name="group">
  <properties>
   <property name="ySortEnabled" type="bool" value="true"/>
   <property name="zIndex" type="int" value="2"/>
  </properties>
  <objectgroup id="8" name="things" draworder="topdown">
   <properties>
    <property name="ySortEnabled" type="bool" value="false"/>
    <property name="zIndex" type="int" value="-1"/>
   </properties>
   <object id="1" name="spot" x="8" y="8">
    <point/>
   </object>
  </objectgroup>
  <imagelayer id="9" name="sky">
   <properties>
    <property name="ySortEnabled" type="bool" value="true"/>
    <property name="zIndex" type="int" value="-2"/>
   </properties>
   <image source="tiles.png" width="64" height="64"/>
  </imagelayer>
  <layer id="10" name="inner" width="2" height="1">
   <properties>
    <property name="godotYSortOrigin" type="int" value="-4"/>
    <property name="zIndex" value="7"/>
   </properties>
   <data encoding="csv">2,0</data>
  </layer>
 </group>
 <group id="11" name="gone">
  <properties>
   <property name="noExport" type="bool" value="true"/>
  </properties>
  <layer id="12" name="child" width="2" height="1">
   <data encoding="csv">1,0</data>
  </layer>
 </group>
 <layer id="13" name="dup" width="2" height="1">
  <data encoding="csv">1,0</data>
 </layer>
 <objectgroup id="14" name="loose" draworder="index">
  <properties>
   <property name="godotYSortOrigin" type="int" value="4"/>
   <property name="noExport" type="bool" value="false"/>
   <property name="tilesetOnly" type="bool" value="true"/>
  </properties>
 </objectgroup>
 <layer id="15" name="far" width="2" height="1" parallaxx="0.5">
  <properties>
   <property name="ySortEnabled" type="bool" value="true"/>
   <property name="zIndex" type="int" value="1"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
 <layer id="16" name="float" width="2" height="1">
  <properties>
   <property name="zIndex" type="float" value="2.6"/>
  </properties>
  <data encoding="csv">1,0</data>
 </layer>
</map>
"#;

fn probe(name: &str) -> std::path::PathBuf {
    let dir = project(name);
    std::fs::write(dir.join("maps/conventions.tmx"), PROBE).unwrap();
    dir
}

/// A node's lines, without its cells.
fn lines<'t>(s: &'t [(String, Vec<String>)], name: &str, parent: &str) -> Vec<&'t str> {
    node(s, name, parent)
        .iter()
        .map(String::as_str)
        .filter(|l| !l.starts_with("tile_map_data = "))
        .collect()
}

fn codes(report: &Report) -> Vec<&'static str> {
    report.notes.iter().map(|n| n.code.as_str()).collect()
}

#[test]
fn layer_conventions_become_node_settings_and_are_not_metadata() {
    let dir = probe("layers");
    let target = dir.join("conventions.tscn");
    let report = export(
        &dir.join("maps/conventions.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);

    let root = &s
        .iter()
        .find(|(h, _)| h.starts_with("[node name=\"conventions\" type=\"Node2D\"]"))
        .unwrap()
        .1;
    assert_eq!(root[0], "texture_filter = 1");
    assert_eq!(
        root[2..],
        ["y_sort_enabled = true", "metadata/intent = \"kept\""],
        "the map's ySortEnabled is the root's"
    );
    assert_eq!(
        lines(&s, "sorted", "."),
        [
            "z_index = 3",
            "y_sort_enabled = true",
            "tile_set = SubResource(\"TileSet_0\")",
            "y_sort_origin = 8",
            "metadata/layer_id = 1",
            "metadata/keep = \"yes\""
        ]
    );
    assert_eq!(
        lines(&s, "tiled_false", "."),
        [
            "tile_set = SubResource(\"TileSet_0\")",
            "metadata/layer_id = 2"
        ],
        "false is false"
    );
    assert_eq!(lines(&s, "high", ".")[0], "z_index = 4096");
    assert_eq!(lines(&s, "low", ".")[0], "z_index = -4096");
    assert_eq!(
        lines(&s, "float", ".")[0],
        "z_index = 3",
        "rounded, as Tiled reads a float"
    );
    assert_eq!(
        lines(&s, "group", "."),
        [
            "z_index = 2",
            "y_sort_enabled = true",
            "metadata/layer_id = 7"
        ]
    );
    assert_eq!(
        lines(&s, "things", "group"),
        ["z_index = -1", "metadata/layer_id = 8"],
        "ySortEnabled false outweighs top-down order"
    );
    assert_eq!(
        lines(&s, "sky", "group")[..2],
        ["z_index = -2", "y_sort_enabled = true"]
    );
    assert_eq!(
        lines(&s, "inner", "group"),
        [
            "z_index = 7",
            "tile_set = SubResource(\"TileSet_0\")",
            "y_sort_origin = -4",
            "metadata/layer_id = 10"
        ],
        "a string zIndex, read as Tiled reads it"
    );
    // Inside its parallax holder, the layer itself carries them; the holder doesn't.
    assert_eq!(node(&s, "far", "."), ["scroll_scale = Vector2(0.5, 1)"]);
    assert_eq!(
        lines(&s, "far", "far")[..2],
        ["z_index = 1", "y_sort_enabled = true"]
    );
    // Where a convention doesn't apply, it stays metadata.
    assert_eq!(
        lines(&s, "loose", "."),
        [
            "metadata/layer_id = 14",
            "metadata/godotYSortOrigin = 4",
            "metadata/tilesetOnly = true"
        ]
    );

    // Left out: noExport (a group with its children) and tilesetOnly; the name a layer left out
    // had is free for the one that stays.
    let headers: Vec<&str> = s
        .iter()
        .map(|(h, _)| h.as_str())
        .filter(|h| h.starts_with("[node"))
        .collect();
    let names: Vec<&str> = headers
        .iter()
        .map(|h| h.split('"').nth(1).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "conventions",
            "sorted",
            "tiled_false",
            "high",
            "low",
            "group",
            "things",
            "spot",
            "sky",
            "inner",
            "dup",
            "loose",
            "far",
            "far",
            "float"
        ]
    );
    assert_eq!(lines(&s, "dup", ".")[1], "metadata/layer_id = 13");
    assert_eq!(report.counts.layers, 12);
    // The tilesetOnly layer's image collection is in the TileSet, the noExport layer's isn't.
    let resource = read::parse(&tscn).unwrap();
    let sources: Vec<String> = resource
        .sub_resources()
        .filter(|(sub, _)| sub.class == "TileSetAtlasSource")
        .map(|(_, sec)| read::string(sec.get("resource_name").unwrap()).unwrap())
        .collect();
    assert_eq!(
        sources,
        ["maps/tiles.tsx", "maps/conventions.tmx::palette_things#0"]
    );

    let consumed: BTreeSet<String> = [
        "godotYSortOrigin",
        "noExport",
        "tilesetOnly",
        "ySortEnabled",
        "zIndex",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(report.consumed, consumed);
    assert!(
        !tscn.contains("metadata/ySortEnabled")
            && !tscn.contains("metadata/zIndex")
            && !tscn.contains("metadata/noExport"),
        "{tscn}"
    );

    assert_eq!(
        codes(&report),
        [
            "godot/left-out",
            "godot/ysort-false",
            "godot/zindex-clamped",
            "godot/differs-from-tiled"
        ],
        "{:?}",
        report.warnings()
    );
    let [origin, y_sort_false, clamped, differs] = &report.notes[..] else {
        panic!("the expected grouped diagnostics")
    };
    assert_eq!(
        (origin.severity, &origin.elements[..]),
        (Severity::Info, &[Element::Layer(15)][..])
    );
    assert!(origin.message.starts_with("the map's parallax origin (0, 0) isn't in the scene: Godot's Parallax2D scrolls layer far about"), "{}", origin.message);
    assert_eq!(y_sort_false.elements, [Element::Layer(2)]);
    assert!(
        y_sort_false
            .message
            .starts_with("layer tiled_false: ySortEnabled is false"),
        "{}",
        y_sort_false.message
    );
    assert_eq!(clamped.elements, [Element::Layer(3), Element::Layer(4)]);
    assert!(
        clamped
            .message
            .ends_with("layer high 5000 becomes 4096, layer low -9000 becomes -4096"),
        "{}",
        clamped.message
    );
    assert_eq!(differs.severity, Severity::Info);
    assert!(
        differs.message.ends_with(
            "ySortEnabled on the map and layers group, sky; zIndex on layers group, things, sky"
        ),
        "{}",
        differs.message
    );
    assert_eq!(differs.elements, [7, 9, 8].map(Element::Layer));
    std::fs::remove_dir_all(&dir).ok();
}

/// `ySortEnabled = true` on an object layer drawn top down changes nothing, as such a layer is
/// y-sorted without it, and the note says so: it said the property applied here, unlike in
/// Tiled's exporter, as if it was what sorted the layer. On a layer drawn in index order it does
/// turn y-sort on.
#[test]
fn y_sort_on_a_top_down_object_layer_is_said_to_change_nothing() {
    let dir = project("ysort-top-down");
    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="32" tileheight="32" infinite="0" nextlayerid="3" nextobjectid="3">
 <objectgroup id="1" name="props">
  <properties>
   <property name="ySortEnabled" type="bool" value="true"/>
  </properties>
  <object id="1" x="8" y="8" width="4" height="4"/>
 </objectgroup>
 <objectgroup id="2" name="markers" draworder="index">
  <properties>
   <property name="ySortEnabled" type="bool" value="true"/>
  </properties>
  <object id="2" x="16" y="16" width="4" height="4"/>
 </objectgroup>
</map>
"#;
    std::fs::write(dir.join("maps/sorted.tmx"), map).unwrap();
    let target = dir.join("sorted.tscn");
    let report = export(
        &dir.join("maps/sorted.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let s = sections(&std::fs::read_to_string(&target).unwrap());
    assert_eq!(lines(&s, "props", ".")[0], "y_sort_enabled = true");
    assert_eq!(lines(&s, "markers", ".")[0], "y_sort_enabled = true");
    let notes: Vec<(&str, Severity, &str, &[Element])> = report
        .notes
        .iter()
        .map(|n| {
            (
                n.code.as_str(),
                n.severity,
                n.message.as_str(),
                n.elements.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/ysort-top-down",
                Severity::Info,
                "layer props: ySortEnabled changes nothing: an object layer drawn top down is y-sorted without it (Tiled's Godot exporter y-sorts no object layer)",
                &[Element::Layer(1)][..]
            ),
            (
                "godot/differs-from-tiled",
                Severity::Info,
                "Tiled's Godot exporter reads ySortEnabled and zIndex on tile layers only; here they also apply: ySortEnabled on layer markers",
                &[Element::Layer(2)][..]
            ),
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// What Tiled's exporter was given, `ySortEnabled = false` on a tile layer (which it y-sorts):
/// a warning, and no y-sort.
#[test]
fn a_tile_layer_set_up_for_tileds_exporter_gets_a_warning_and_no_y_sort() {
    let dir = project("layers-tiled");
    let features = FEATURES.replacen(
        r#"<layer id="1" name="cells" width="6" height="4">"#,
        r#"<layer id="1" name="cells" width="6" height="4">
  <properties>
   <property name="ySortEnabled" type="bool" value="false"/>
  </properties>"#,
        1,
    );
    assert_ne!(features, FEATURES);
    std::fs::write(dir.join("maps/tiled.tmx"), features).unwrap();
    let target = dir.join("tiled.tscn");
    let report = export(
        &dir.join("maps/tiled.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(codes(&report), ["godot/left-out", "godot/ysort-false"]);
    assert_eq!(report.notes[1].severity, Severity::Warning);
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);
    let cells = node(&s, "cells", ".");
    assert!(
        !cells
            .iter()
            .any(|l| l.starts_with("y_sort_enabled") || l.contains("ySortEnabled")),
        "{cells:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Parallax groups (one with an offset), a layer with a factor of its own in one, a layer with
/// none, a layer whose factor undoes its group's, and a layer outside groups.
pub(super) const PARALLAX_GROUPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="1" height="1" tilewidth="32" tileheight="32" infinite="0" nextlayerid="7" nextobjectid="1">
 <tileset firstgid="1" source="tiles.tsx"/>
 <group id="1" name="far" offsetx="10" offsety="20" parallaxx="0.5" parallaxy="0.5">
  <layer id="2" name="farther" width="1" height="1" parallaxx="0.5" parallaxy="0.75"><data encoding="csv">1</data></layer>
  <layer id="3" name="along" width="1" height="1"><data encoding="csv">2</data></layer>
 </group>
 <group id="5" name="fast" parallaxx="0.5">
  <layer id="6" name="back" width="1" height="1" parallaxx="2"><data encoding="csv">2</data></layer>
 </group>
 <layer id="4" name="plain" width="1" height="1" parallaxx="0.3"><data encoding="csv">3</data></layer>
</map>
"#;

/// Tiled scrolls a layer by its factor times its groups' and a group by nothing of its own, while
/// Godot adds up the scrolls of nested Parallax2Ds and sets a Parallax2D's own position: so a
/// group is a Node2D at its offset, and each layer whose factor with its groups' isn't 1 is in a
/// Parallax2D with that product, never one inside another.
#[test]
fn parallax_groups_pass_their_factor_to_their_layers() {
    let dir = project("layers-parallax");
    std::fs::write(dir.join("maps/parallax.tmx"), PARALLAX_GROUPS).unwrap();
    let target = dir.join("parallax.tscn");
    let report = export(
        &dir.join("maps/parallax.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);
    let header = |name: &str, parent: &str| {
        s.iter()
            .find(|(h, _)| {
                h.starts_with(&format!("[node name=\"{name}\" type="))
                    && h.contains(&format!("parent=\"{parent}\""))
            })
            .unwrap_or_else(|| panic!("no {parent}/{name}"))
    };
    let parallax2d = |name: &str, parent: &str| {
        let (h, lines) = header(name, parent);
        assert!(h.contains("type=\"Parallax2D\""), "{h}");
        assert!(
            !lines.iter().any(|l| l.starts_with("position")),
            "Godot sets a Parallax2D's position: {lines:?}"
        );
        value(lines, "scroll_scale").to_owned()
    };
    for (group, position) in [("far", Some("Vector2(10, 20)")), ("fast", None)] {
        let (h, lines) = header(group, ".");
        assert!(h.contains("type=\"Node2D\""), "{h}");
        assert!(
            !lines.iter().any(|l| l.starts_with("scroll_scale")),
            "{lines:?}"
        );
        assert_eq!(
            lines.iter().find_map(|l| l.strip_prefix("position = ")),
            position
        );
    }
    assert_eq!(parallax2d("farther", "far"), "Vector2(0.25, 0.375)");
    assert!(
        header("farther", "far/farther")
            .0
            .contains("type=\"TileMapLayer\"")
    );
    assert_eq!(parallax2d("along", "far"), "Vector2(0.5, 0.5)");
    assert!(
        header("back", "fast").0.contains("type=\"TileMapLayer\""),
        "0.5 × 2 scrolls with the map: no holder"
    );
    assert_eq!(parallax2d("plain", "."), "Vector2(0.3, 1)");
    let holders: Vec<&str> = s
        .iter()
        .filter(|(h, _)| h.contains("type=\"Parallax2D\""))
        .map(|(h, _)| {
            h.split("parent=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(holders, ["far", "far", "."], "no Parallax2D inside another");
    // The parallax origin isn't the centre of Godot's default window.
    assert_eq!(codes(&report), ["godot/left-out"]);
    assert_eq!(report.notes[0].elements, [2, 3, 4].map(Element::Layer));
    std::fs::remove_dir_all(&dir).ok();
}

/// On an isometric grid, tile layers painting tiles that reach above their cell (taller, or
/// raised by their tileset's offset) and saying nothing about y-sort get one note; y-sort stays
/// off.
#[test]
fn tall_tiles_off_the_orthogonal_grid_get_a_note_unless_the_layer_says_how_to_sort() {
    let dir = project("layers-tall");
    let layer = |id: u32, name: &str, props: &str, data: &str| {
        format!(
            " <layer id=\"{id}\" name=\"{name}\" width=\"2\" height=\"1\">\n  <properties>{props}</properties>\n  <data encoding=\"csv\">{data}</data>\n </layer>\n"
        )
    };
    let y_sort =
        |on: bool| format!("<property name=\"ySortEnabled\" type=\"bool\" value=\"{on}\"/>");
    let tmx = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="isometric" renderorder="right-down" width="2" height="1" tilewidth="32" tileheight="16" infinite="0" nextlayerid="7" nextobjectid="1">
 <tileset firstgid="1" source="tiles.tsx"/>
 <tileset firstgid="5" name="flat" tilewidth="32" tileheight="16" tilecount="1" columns="0">
  <tile id="0" x="0" y="0" width="32" height="16"><image source="tiles.png" width="64" height="64"/></tile>
 </tileset>
 <tileset firstgid="6" name="raised" tilewidth="32" tileheight="16" tilecount="1" columns="0">
  <tileoffset x="0" y="-4"/>
  <tile id="0" x="0" y="16" width="32" height="16"><image source="tiles.png" width="64" height="64"/></tile>
 </tileset>
{}{}{}{}{}{}</map>
"#,
        layer(1, "tall", "", "1,0"),
        layer(2, "sorted", &y_sort(true), "1,0"),
        layer(3, "unsorted", &y_sort(false), "1,0"),
        layer(4, "empty", "", "0,0"),
        layer(5, "fits", "", "5,5"),
        layer(6, "lifted", "", "5,6"),
    );
    std::fs::write(dir.join("maps/tall.tmx"), tmx).unwrap();
    let target = dir.join("tall.tscn");
    let report = export(
        &dir.join("maps/tall.tmx"),
        &mut loader(&dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    let codes: Vec<&str> = codes(&report)
        .into_iter()
        .filter(|c| *c != "godot/grid-shift")
        .collect();
    assert_eq!(
        codes,
        ["godot/ysort-false", "godot/tall-tiles-no-ysort"],
        "{:?}",
        report.warnings()
    );
    let tall = report.notes.last().unwrap();
    assert_eq!(
        (tall.severity, &tall.elements[..]),
        (Severity::Info, &[Element::Layer(1), Element::Layer(6)][..])
    );
    assert!(
        tall.message.starts_with(
            "layers tall, lifted: tiles taller than their cell overlap by rendering quadrant"
        ),
        "{}",
        tall.message
    );
    let tscn = std::fs::read_to_string(&target).unwrap();
    assert_eq!(
        tscn.matches("y_sort_enabled = true").count(),
        1,
        "only the layer that asks: {tscn}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Reads back each node's y-sort, z index, y-sort origin and metadata names.
const SETTINGS: &str = r#"extends SceneTree

func _init() -> void:
	var room := (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	var nodes := {}
	for node: Node in [room] + room.find_children("*", "", true, false):
		var d := {"class": node.get_class(), "meta": node.get_meta_list().map(func(k: StringName) -> String: return String(k))}
		if node is CanvasItem:
			d["y_sort_enabled"] = (node as CanvasItem).y_sort_enabled
			d["z_index"] = (node as CanvasItem).z_index
		if node is TileMapLayer:
			d["y_sort_origin"] = (node as TileMapLayer).y_sort_origin
		nodes[String(room.get_path_to(node))] = d
	print("BT-JSON-BEGIN", JSON.stringify(nodes), "BT-JSON-END")
	room.free()
	quit()
"#;

/// Imports the project, then runs `SETTINGS` on `scene`: its JSON and what Godot printed on
/// stderr. Gives up after two minutes rather than hang on a script error.
fn godot_settings(godot: &Path, dir: &Path, scene: &str) -> (Value, String) {
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
    std::fs::write(dir.join("settings.gd"), SETTINGS).unwrap();
    let (out, err) = (dir.join("settings.out"), dir.join("settings.err"));
    let mut child = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .args(["--script", "res://settings.gd", "--", scene])
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

/// Godot reads the conventions back as node settings, has none of them as metadata, and has no
/// node for the layers left out.
#[test]
fn godot_reads_the_layer_conventions_back() {
    let Some(godot) = godot() else { return };
    let dir = probe("layers-godot");
    export(
        &dir.join("maps/conventions.tmx"),
        &mut loader(&dir),
        &dir.join("conventions.tscn"),
        &Options::default(),
    )
    .unwrap();
    let (nodes, stderr) = godot_settings(&godot, &dir, "res://conventions.tscn");
    assert_eq!(stderr, "", "Godot loads it cleanly");
    let paths: BTreeSet<&str> = nodes
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<&str> = [
        ".",
        "sorted",
        "tiled_false",
        "high",
        "low",
        "group",
        "group/things",
        "group/things/spot",
        "group/sky",
        "group/inner",
        "dup",
        "loose",
        "far",
        "far/far",
        "float",
    ]
    .into();
    assert_eq!(
        paths, expected,
        "no node for noExport and tilesetOnly layers, nor the noExport group's children"
    );
    let settings = |path: &str| {
        let n = &nodes[path];
        (
            n["y_sort_enabled"].as_bool().unwrap(),
            n["z_index"].as_i64().unwrap(),
            n["y_sort_origin"].as_i64(),
        )
    };
    assert_eq!(settings("."), (true, 0, None));
    assert_eq!(settings("sorted"), (true, 3, Some(8)));
    assert_eq!(settings("tiled_false"), (false, 0, Some(0)));
    assert_eq!(settings("high"), (false, 4096, Some(0)));
    assert_eq!(settings("low"), (false, -4096, Some(0)));
    assert_eq!(settings("group"), (true, 2, None));
    assert_eq!(settings("group/things"), (false, -1, None));
    assert_eq!(settings("group/sky"), (true, -2, None));
    assert_eq!(settings("group/inner"), (false, 7, Some(-4)));
    assert_eq!(settings("far"), (false, 0, None));
    assert_eq!(settings("far/far"), (true, 1, Some(0)));
    assert_eq!(settings("float"), (false, 3, Some(0)));
    for (path, node) in nodes.as_object().unwrap() {
        let meta: Vec<&str> = node["meta"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        let consumed = |m: &&str| {
            ["ySortEnabled", "zIndex", "noExport"].contains(m)
                || *m == "tilesetOnly" && path != "loose"
                || *m == "godotYSortOrigin" && path != "loose"
        };
        assert!(!meta.iter().any(consumed), "{path}: {meta:?}");
    }
    assert_eq!(
        nodes["loose"]["meta"],
        serde_json::json!(["layer_id", "godotYSortOrigin", "tilesetOnly"])
    );
    assert_eq!(
        nodes["sorted"]["meta"],
        serde_json::json!(["layer_id", "keep"])
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A map with a background colour, a parallax layer and text objects: one in another family and
/// bold, one italic and underlined, one plain (Tiled's sans-serif).
const LEFT_OUT: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="32" tileheight="32" infinite="0" backgroundcolor="#27b99a" nextlayerid="3" nextobjectid="4">
 <layer id="1" name="sky" width="2" height="2" parallaxx="0.5">
  <data encoding="csv">0,0,0,0</data>
 </layer>
 <objectgroup id="2" name="signs">
  <object id="1" x="0" y="0" width="64" height="16"><text fontfamily="Courier New" bold="1">Inn</text></object>
  <object id="2" x="0" y="20" width="64" height="16"><text italic="1" underline="1">Shop</text></object>
  <object id="3" x="0" y="40" width="64" height="16"><text>Well</text></object>
 </objectgroup>
</map>
"##;

/// What the scene can't hold is said, in Info notes: the background colour (with where Godot has
/// one), the parallax origin when a layer scrolls (unless it is the centre of the game's window,
/// where Godot scrolls the same), and text styling a Label doesn't take, with its objects.
#[test]
fn what_the_scene_cant_hold_gets_a_left_out_note() {
    let dir = project("layers-left-out");
    std::fs::write(dir.join("maps/left.tmx"), LEFT_OUT).unwrap();
    let report = export(
        &dir.join("maps/left.tmx"),
        &mut loader(&dir),
        &dir.join("left.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(codes(&report), ["godot/left-out"; 3]);
    assert!(report.notes.iter().all(|n| n.severity == Severity::Info));
    let [colour, origin, text] = &report.notes[..] else {
        panic!("the expected grouped diagnostics")
    };
    assert_eq!(
        colour.message,
        "the map's background colour #27b99a isn't in the scene, which has no background of its own: in Godot it is Project Settings ▸ Rendering ▸ Environment ▸ Defaults ▸ Default Clear Color"
    );
    assert_eq!(origin.elements, [Element::Layer(1)]);
    assert!(origin.message.starts_with("the map's parallax origin (0, 0) isn't in the scene: Godot's Parallax2D scrolls layer sky about the top-left corner of the camera's view"), "{}", origin.message);
    assert!(
        origin
            .message
            .contains("at the centre of the game's window, (576, 324), and the camera not zoomed"),
        "{}",
        origin.message
    );
    assert_eq!(text.elements, [Element::Object(1), Element::Object(2)]);
    assert_eq!(
        text.message,
        "objects 1 and 2: Godot's Label doesn't take their text's font family Courier New, bold, italic or underline, so they show in the theme's font; in Godot a SystemFont can name a family and make it bold or italic"
    );

    // No background colour, plain text, and the parallax origin at the centre of the project's
    // window: nothing is left out.
    let plain = LEFT_OUT
        .replace(
            r##" backgroundcolor="#27b99a""##,
            r#" parallaxoriginx="320" parallaxoriginy="180""#,
        )
        .replace(r#" fontfamily="Courier New" bold="1""#, "")
        .replace(
            r#" italic="1" underline="1""#,
            r#" fontfamily="sans-serif""#,
        );
    std::fs::write(dir.join("maps/left.tmx"), plain).unwrap();
    let godot = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    std::fs::write(dir.join("project.godot"), format!("{godot}\n[display]\n\nwindow/size/viewport_width=640\nwindow/size/viewport_height=360\n")).unwrap();
    let report = export(
        &dir.join("maps/left.tmx"),
        &mut loader(&dir),
        &dir.join("left.tscn"),
        &Options {
            replace: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(report.warnings(), Vec::<String>::new());
    std::fs::remove_dir_all(&dir).ok();
}

/// An infinite map whose chunks reach left of and above the origin, with an image layer
/// repeating both ways at an offset, and one repeating across in a group with an offset.
const REPEATS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="1" height="1" tilewidth="16" tileheight="16" infinite="1" nextlayerid="5" nextobjectid="1">
 <tileset firstgid="1" source="tiles.tsx"/>
 <layer id="1" name="ground" width="1" height="1">
  <data encoding="csv">
   <chunk x="-16" y="-16" width="16" height="16">
1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0
</chunk>
   <chunk x="0" y="0" width="16" height="16">
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2
</chunk>
  </data>
 </layer>
 <imagelayer id="2" name="bg" offsetx="10" offsety="20" repeatx="1" repeaty="1">
  <image source="tiles.png" width="64" height="64"/>
 </imagelayer>
 <group id="3" name="band" offsetx="-6" offsety="4">
  <imagelayer id="4" name="strip" repeatx="1">
   <image source="tiles.png" width="64" height="64"/>
  </imagelayer>
 </group>
</map>
"#;

/// Tiled repeats an image layer from its origin without end; in Godot it covers the map's
/// bounds, in whole images from that origin (the layer's offset and its groups'), so that each
/// image falls where Tiled draws one: on an infinite map reaching left of and above the origin,
/// and on a finite map with an offset.
#[test]
fn repeating_image_layers_cover_the_map_in_whole_images_from_their_origin() {
    let dir = project("layers-repeats");
    std::fs::write(dir.join("maps/repeats.tmx"), REPEATS).unwrap();
    export(
        &dir.join("maps/repeats.tmx"),
        &mut loader(&dir),
        &dir.join("repeats.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("repeats.tscn")).unwrap();
    let s = sections(&tscn);
    // The map spans -256 to 256 both ways; images 64 px from (10, 20), and from (-6, 4) across.
    let region = |name: &str, parent: &str| {
        let lines = node(&s, name, parent);
        (
            value(lines, "position"),
            value(lines, "region_rect"),
            value(lines, "offset"),
        )
    };
    assert_eq!(
        region("bg", "."),
        (
            "Vector2(10, 20)",
            "Rect2(-320, -320, 576, 576)",
            "Vector2(-320, -320)"
        )
    );
    let strip = node(&s, "strip", "band");
    assert!(
        !strip.iter().any(|l| l.starts_with("position")),
        "{strip:?}"
    );
    assert_eq!(
        (value(strip, "region_rect"), value(strip, "offset")),
        ("Rect2(-256, 0, 576, 64)", "Vector2(-256, 0)")
    );

    // A finite map 192 px across, the image 10 px in: from -64 (and down from the top only).
    let finite = FEATURES.replacen(
        r#"<imagelayer id="4" name="bg" parallaxx="0.5" repeatx="1">"#,
        r#"<imagelayer id="4" name="bg" offsetx="10" parallaxx="0.5" repeatx="1">"#,
        1,
    );
    assert_ne!(finite, FEATURES);
    std::fs::write(dir.join("maps/finite.tmx"), finite).unwrap();
    export(
        &dir.join("maps/finite.tmx"),
        &mut loader(&dir),
        &dir.join("finite.tscn"),
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(dir.join("finite.tscn")).unwrap();
    let s = sections(&tscn);
    let bg = node(&s, "bg", "bg");
    assert_eq!(
        (
            value(bg, "position"),
            value(bg, "region_rect"),
            value(bg, "offset")
        ),
        (
            "Vector2(10, 0)",
            "Rect2(-64, 0, 256, 64)",
            "Vector2(-64, 0)"
        )
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Each Sprite2D's rectangle and each TileMapLayer's used cells, in the scene's coordinates.
const RECTS: &str = r#"extends SceneTree

func _initialize() -> void:
	var room := (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	root.add_child(room)
	var out := {}
	for node: Node in room.find_children("*", "", true, false):
		var r := Rect2()
		if node is Sprite2D:
			r = (node as Sprite2D).get_global_transform() * (node as Sprite2D).get_rect()
		elif node is TileMapLayer:
			var layer := node as TileMapLayer
			var used := layer.get_used_rect()
			var size := Vector2(layer.tile_set.tile_size)
			r = Rect2(layer.to_global(Vector2(used.position) * size), Vector2(used.size) * size)
		else:
			continue
		out[String(room.get_path_to(node))] = [r.position.x, r.position.y, r.end.x, r.end.y]
	print("BT-JSON-BEGIN", JSON.stringify(out), "BT-JSON-END")
	quit()
"#;

/// Godot draws the repeating image layers over every cell of the infinite map.
#[test]
fn godot_draws_repeating_image_layers_over_the_whole_map() {
    let Some(godot) = godot() else { return };
    let dir = project("layers-repeats-godot");
    std::fs::write(dir.join("maps/repeats.tmx"), REPEATS).unwrap();
    export(
        &dir.join("maps/repeats.tmx"),
        &mut loader(&dir),
        &dir.join("repeats.tscn"),
        &Options::default(),
    )
    .unwrap();
    let (_, err) = run_godot(&godot, &dir, &["--import"]);
    assert_eq!(err, "");
    std::fs::write(dir.join("rects.gd"), RECTS).unwrap();
    let (out, err) = run_godot(
        &godot,
        &dir,
        &["--script", "res://rects.gd", "--", "res://repeats.tscn"],
    );
    assert_eq!(err, "", "{out}");
    let rects = json_between_markers(&out, &err);
    let rect = |path: &str| -> Vec<f64> {
        rects[path]
            .as_array()
            .unwrap_or_else(|| panic!("no {path}: {rects}"))
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect()
    };
    let cells = rect("ground");
    assert_eq!(
        cells,
        [-256.0, -256.0, 256.0, 256.0],
        "the two painted corners"
    );
    let bg = rect("bg");
    assert!(
        bg[0] <= cells[0] && bg[1] <= cells[1] && bg[2] >= cells[2] && bg[3] >= cells[3],
        "bg {bg:?} covers the cells {cells:?}"
    );
    let strip = rect("band/strip");
    assert!(
        strip[0] <= cells[0] && strip[2] >= cells[2] && strip[3] - strip[1] == 64.0,
        "strip {strip:?} spans the cells {cells:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A 64×64 tile with a collision rectangle all over it, on 32×32 cells: painted, then mirrored
/// next to it, so their shapes overlap.
pub(super) const BIG_TILES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="2" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="2">
 <tileset firstgid="1" name="big" tilewidth="64" tileheight="64" tilecount="1" columns="1">
  <image source="tiles.png" width="64" height="64"/>
  <tile id="0">
   <objectgroup draworder="index">
    <object id="1" x="0" y="0" width="64" height="64"/>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="walls" width="4" height="2">
  <data encoding="csv">
0,0,0,0,
1,2147483649,0,0
</data>
 </layer>
</map>
"#;

/// Writes a staggered map of tall tiles (64×96 on 64×32 cells) whose collision shapes reach into
/// their neighbours', painted every way Tiled can flip them, and their image, in `dir`'s maps
/// folder; returns the map's path.
pub(super) fn mirrored_tall_tiles(dir: &Path) -> std::path::PathBuf {
    let image = image::RgbaImage::from_fn(64, 96, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 2) as u8, 120, 255])
    });
    image.save(dir.join("maps/tall.png")).unwrap();
    let (h, v, d) = (0x8000_0000u32, 0x4000_0000u32, 0x2000_0000u32);
    let rows = [
        [0, h, v, d],
        [h | v, h | d, v | d, h | v | d],
        [0, h, v, h | v],
        [u32::MAX; 4],
    ];
    let csv: Vec<String> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|&f| {
                    if f == u32::MAX {
                        "0".to_owned()
                    } else {
                        (1 | f).to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    let map = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="staggered" renderorder="right-down" width="4" height="4" tilewidth="64" tileheight="32" infinite="0" staggeraxis="x" staggerindex="even" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" name="tall" tilewidth="64" tileheight="96" tilecount="1" columns="1">
  <tileoffset x="0" y="8"/>
  <image source="tall.png" width="64" height="96"/>
  <tile id="0">
   <objectgroup draworder="index">
    <object id="1" x="8" y="40"><polygon points="0,0 40,0 40,40 20,40 20,20 0,20"/></object>
    <object id="2" x="4" y="70" width="30" height="10"/>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
{}
</data>
 </layer>
</map>
"#,
        csv.join(",\n")
    );
    let path = dir.join("maps/mirrored.tmx");
    std::fs::write(&path, map).unwrap();
    path
}

/// Points inside the shapes of the cells of [`mirrored_tall_tiles`] that aren't transposed, in
/// the layer's coordinates (Tiled's pixels): the middle of the L's top bar, mirrored as the cell.
fn inside_mirrored_tall_tiles(path: &Path) -> Vec<DVec2> {
    let map = bt_tmx::Loader::builder()
        .build()
        .load_map(path)
        .unwrap()
        .value;
    let geometry = map.geometry();
    let tiles = map.layers[0].as_tile_layer().unwrap();
    let mut points = Vec::new();
    for bt_core::PlacedCell { at: c, cell } in tiles.cells() {
        if cell.is_empty() || cell.flip().contains(bt_core::Flip::D) {
            continue;
        }
        let rect = geometry.cell_image_rect(c, DVec2::new(64.0, 96.0));
        let x = if cell.flip().contains(bt_core::Flip::H) {
            64.0 - 28.0
        } else {
            28.0
        };
        let y = if cell.flip().contains(bt_core::Flip::V) {
            96.0 - 50.0
        } else {
            50.0
        };
        points.push(rect.min + DVec2::new(0.0, 8.0) + DVec2::new(x, y));
    }
    points
}

/// Godot's collision bodies at points (`x,y;x,y…` in the scene's first TileMapLayer's
/// coordinates) once the scene has been in the tree for a few physics frames, with the layer's
/// physics quadrant size forced to the third argument unless it is 0.
const HITS: &str = r#"extends SceneTree

func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	var room := (load(args[0]) as PackedScene).instantiate()
	var layer := room.find_children("*", "TileMapLayer", true, false)[0] as TileMapLayer
	if int(args[2]) > 0:
		layer.physics_quadrant_size = int(args[2])
	root.add_child(room)
	for i in 4:
		await physics_frame
	var space := layer.get_world_2d().direct_space_state
	var hits := []
	for p in args[1].split(";"):
		var xy := p.split(",")
		var query := PhysicsPointQueryParameters2D.new()
		query.position = layer.to_global(Vector2(float(xy[0]), float(xy[1])))
		hits.append(space.intersect_point(query, 32).size())
	print("BT-JSON-BEGIN", JSON.stringify(hits), "BT-JSON-END")
	quit()
"#;

/// The bodies Godot finds at `points` in `scene`, with the physics quadrant size `forced` (0
/// keeps the scene's), and what it printed on stderr.
fn hits(
    godot: &Path,
    dir: &Path,
    scene: &str,
    points: &[DVec2],
    forced: u32,
) -> (Vec<u64>, String) {
    std::fs::write(dir.join("hits.gd"), HITS).unwrap();
    let points: Vec<String> = points.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
    let (out, err) = run_godot(
        godot,
        dir,
        &[
            "--script",
            "res://hits.gd",
            "--",
            scene,
            &points.join(";"),
            &forced.to_string(),
        ],
    );
    (
        json_between_markers(&out, &err)
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_u64().unwrap())
            .collect(),
        err,
    )
}

/// Godot 4.5 and later merge the collision shapes of a TileMapLayer's 16×16 physics quadrants,
/// and cut a mirrored tile's shapes away where they overlap another cell's (or lose them all):
/// a tile layer with a mirrored tile whose shapes can reach out of its cell collides cell by
/// cell, with an Info note. Shapes inside their cells only touch: such layers keep quadrants. So
/// do projects for a Godot before 4.5.
#[test]
fn mirrored_tiles_whose_shapes_reach_out_of_their_cells_collide_cell_by_cell() {
    let dir = project("layers-quadrants");
    std::fs::write(dir.join("maps/big.tmx"), BIG_TILES).unwrap();
    let quadrant = |map: &str, version: Option<&str>| {
        let features = version.map_or(String::new(), |v| {
            format!("config/features=PackedStringArray(\"{v}\")\n")
        });
        std::fs::write(
            dir.join("project.godot"),
            format!("config_version=5\n\n[application]\n\nconfig/name=\"bt-test\"\n{features}"),
        )
        .unwrap();
        let target = dir.join("quadrants.tscn");
        let report = export(
            &dir.join(format!("maps/{map}.tmx")),
            &mut loader(&dir),
            &target,
            &Options {
                replace: true,
                ..Options::default()
            },
        )
        .unwrap();
        let tscn = std::fs::read_to_string(&target).unwrap();
        let walls = sections(&tscn)
            .into_iter()
            .find(|(h, _)| h.contains("type=\"TileMapLayer\""))
            .unwrap()
            .1;
        (
            walls
                .iter()
                .find_map(|l| l.strip_prefix("physics_quadrant_size = "))
                .map(str::to_owned),
            report,
        )
    };
    for version in [None, Some("4.5"), Some("4.7")] {
        let (size, report) = quadrant("big", version);
        assert_eq!(size.as_deref(), Some("1"), "{version:?}");
        assert_eq!(codes(&report), ["godot/physics-quadrant-size"]);
        assert_eq!(
            (report.notes[0].severity, &report.notes[0].elements[..]),
            (Severity::Info, &[Element::Layer(1)][..])
        );
        assert!(report.notes[0].message.starts_with("layer walls: mirrored tiles with collision shapes that can overlap another cell's, so each cell collides on its own (physics_quadrant_size = 1)"), "{}", report.notes[0].message);
    }
    let (size, report) = quadrant("big", Some("4.4"));
    assert_eq!(
        (size, report.notes.len()),
        (None, 0),
        "Godot 4.4 has a body per cell"
    );

    // 32×32 tiles shaped all over, mirrored every way next to each other: their shapes only touch.
    let small = BIG_TILES
        .replace(
            r#"tilewidth="64" tileheight="64" tilecount="1" columns="1""#,
            r#"tilewidth="32" tileheight="32" tilecount="4" columns="2""#,
        )
        .replace(
            r#"width="64" height="64"/>
   </objectgroup>"#,
            r#"width="32" height="32"/>
   </objectgroup>"#,
        )
        .replace("1,2147483649,0,0", "1,2147483649,1073741825,536870913");
    std::fs::write(dir.join("maps/small.tmx"), small).unwrap();
    let (size, report) = quadrant("small", None);
    assert_eq!((size, report.notes.len()), (None, 0));
    // Tall tiles on a staggered grid.
    mirrored_tall_tiles(&dir);
    assert_eq!(quadrant("mirrored", None).0.as_deref(), Some("1"));
    std::fs::remove_dir_all(&dir).ok();
}

/// In Godot, mirrored tiles collide where Tiled draws their shapes: where two tiles' shapes
/// overlap, both cells' bodies are there; and on the staggered map, every tile collides inside
/// its shapes, though Godot can't decompose the quadrant's merged shapes (it says so, and drops
/// them all) when it is left at its default size.
#[test]
fn godot_has_mirrored_tiles_collide_where_tiled_draws_their_shapes() {
    let Some(godot) = godot() else { return };
    let dir = project("layers-quadrants-godot");
    std::fs::write(dir.join("maps/big.tmx"), BIG_TILES).unwrap();
    export(
        &dir.join("maps/big.tmx"),
        &mut loader(&dir),
        &dir.join("big.tscn"),
        &Options::default(),
    )
    .unwrap();
    let path = mirrored_tall_tiles(&dir);
    export(
        &path,
        &mut loader(&dir),
        &dir.join("mirrored.tscn"),
        &Options::default(),
    )
    .unwrap();
    let (_, err) = run_godot(&godot, &dir, &["--import"]);
    assert_eq!(err, "");

    // Over the plain tile only, over both (two points), over the mirrored one only.
    let overlap = [
        DVec2::new(16.0, 16.0),
        DVec2::new(48.0, 16.0),
        DVec2::new(48.0, 48.0),
        DVec2::new(80.0, 48.0),
    ];
    assert_eq!(
        hits(&godot, &dir, "res://big.tscn", &overlap, 0),
        (vec![1, 2, 2, 1], String::new())
    );
    let (merged, _) = hits(&godot, &dir, "res://big.tscn", &overlap, 16);
    assert_eq!(
        merged,
        [1, 0, 0, 1],
        "Godot's merged quadrant lost the overlap (if Godot no longer does, the export needn't set physics_quadrant_size)"
    );

    let inside = inside_mirrored_tall_tiles(&path);
    assert_eq!(inside.len(), 8);
    let (found, err) = hits(&godot, &dir, "res://mirrored.tscn", &inside, 0);
    assert_eq!(err, "");
    assert!(found.iter().all(|&n| n >= 1), "{found:?} at {inside:?}");
    let (merged, err) = hits(&godot, &dir, "res://mirrored.tscn", &inside, 16);
    assert!(
        err.contains("Convex decomposing failed!") && merged.contains(&0),
        "{merged:?}: {err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
