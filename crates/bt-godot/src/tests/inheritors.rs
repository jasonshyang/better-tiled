//! The level-scene check (docs/design/12-godot-export.md): what Godot will do with scenes that inherit or instance
//! an export, predicted from the scenes as written and checked against Godot loading them.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use bt_core::LayerId;
use serde_json::Value;

use super::super::inheritors::{Fate, Finding, LevelScene, TypeChange};
use super::super::read;
use super::super::report::Element;
use super::oracle::{json_between_markers, run_godot};
use super::support::*;
use super::*;
use bt_project::{
    AutoMapRecord as AutoMapEntry, PaintedRecord as PaintedEntry, ProjectFile as ProjectData,
};

/// A Godot 4.7 project (so the export has unique_ids, unless `ids` is false) with `levels/`.
fn project_with_ids(name: &str, ids: bool) -> PathBuf {
    let dir = project(name);
    let features = if ids {
        "config/features=PackedStringArray(\"4.7\", \"Forward Plus\")\n"
    } else {
        ""
    };
    std::fs::write(
        dir.join("project.godot"),
        format!("config_version=5\n\n[application]\n\nconfig/name=\"bt-test\"\n{features}"),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("levels")).unwrap();
    dir
}

/// Exports `map` (a TMX text) as `maps/base.tmx` to `levels/base.tscn`.
fn export_base(dir: &Path, map: &str) -> Report {
    std::fs::write(dir.join("maps/base.tmx"), map).unwrap();
    export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("levels/base.tscn"),
        &Options::default(),
    )
    .unwrap()
}

/// The findings for the level scene called `name`.
fn findings<'r>(report: &'r Report, name: &str) -> &'r LevelScene {
    report
        .level_scenes
        .iter()
        .find(|l| l.path.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no {name} in {:?}", report.level_scenes))
}

/// Findings as (fate, node, now, element), for comparing.
fn seen(level: &LevelScene) -> Vec<(Fate, &str, Option<&str>, Option<Element>)> {
    level
        .findings
        .iter()
        .map(|f| (f.fate, f.node.as_str(), f.now.as_deref(), f.element))
        .collect()
}

/// The map: tile layers, objects named and unnamed (two called Door, two Gate), a layer that will
/// go, and an image layer inside its Parallax2D.
const V1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="13" nextobjectid="14">
 <tileset firstgid="1" source="tiles.tsx"/>
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
1,1,0,0,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <layer id="7" name="deco" width="4" height="4">
  <data encoding="csv">
0,0,0,2,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <objectgroup id="2" name="Objects">
  <object id="1" name="Door" x="32" y="32" width="32" height="32"/>
  <object id="2" x="64" y="0" width="16" height="16"/>
  <object id="3" name="Lamp" x="10" y="10" width="8" height="8"/>
  <object id="4" name="Gone" x="20" y="20"><point/></object>
  <object id="8" name="Door" x="40" y="40"><point/></object>
  <object id="9" name="Chest" x="50" y="50" width="8" height="8"/>
  <object id="12" name="Gate" x="1" y="1"><point/></object>
  <object id="13" name="Gate" x="2" y="2"><point/></object>
 </objectgroup>
 <objectgroup id="3" name="Other">
  <object id="5" name="Sign" x="0" y="64"><point/></object>
 </objectgroup>
 <objectgroup id="10" name="Doomed">
  <object id="11" name="Box" x="0" y="64"><point/></object>
 </objectgroup>
 <imagelayer id="4" name="sky" parallaxx="0.5"/>
</map>
"#;

/// V1 edited:
/// - layers Objects → Things, deco → decor and sky → clouds;
/// - object 1 deleted, so object 8 is plain Door; object 9 Chest → Coffer; object 12 Gate →
///   Portal, so object 13 is plain Gate;
/// - objects 2 and 11 moved to Other, object 4 deleted;
/// - layer Doomed deleted, and a new layer of that name with a new Box.
const V2: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="13" nextobjectid="15">
 <tileset firstgid="1" source="tiles.tsx"/>
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
1,1,0,0,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <layer id="7" name="decor" width="4" height="4">
  <data encoding="csv">
0,0,0,2,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <objectgroup id="2" name="Things">
  <object id="3" name="Lamp" x="10" y="10" width="8" height="8"/>
  <object id="8" name="Door" x="40" y="40"><point/></object>
  <object id="9" name="Coffer" x="50" y="50" width="8" height="8"/>
  <object id="12" name="Portal" x="1" y="1"><point/></object>
  <object id="13" name="Gate" x="2" y="2"><point/></object>
 </objectgroup>
 <objectgroup id="12" name="Doomed">
  <object id="14" name="Box" x="0" y="64"><point/></object>
 </objectgroup>
 <objectgroup id="3" name="Other">
  <object id="2" x="64" y="0" width="16" height="16"/>
  <object id="5" name="Sign" x="0" y="64"><point/></object>
  <object id="11" name="Box" x="0" y="64"><point/></object>
 </objectgroup>
 <imagelayer id="4" name="clouds" parallaxx="0.5"/>
</map>
"#;

/// A level scene inheriting the export of V1, as a person writes it: overrides of layers,
/// objects and shapes, tiles painted on a generated layer, nodes added under layers, objects and
/// other added nodes, and signal connections. Each node carries `at_<n>`, its path, so a test can
/// see where Godot puts it.
const LEVEL: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://levels/base.tscn" id="1_base"]

[node name="Level" instance=ExtResource("1_base")]

[node name="ground" parent="."]
modulate = Color(1, 0, 0, 1)
metadata/at_1 = "ground"

[node name="deco" parent="."]
tile_map_data = PackedByteArray(0, 0, 3, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0)
metadata/at_2 = "deco"

[node name="Door" parent="Objects"]
metadata/at_3 = "Objects/Door"

[node name="rectangle_2" parent="Objects"]
metadata/at_4 = "Objects/rectangle_2"

[node name="Shape" parent="Objects/Lamp"]
disabled = true
metadata/at_5 = "Objects/Lamp/Shape"

[node name="Gone" parent="Objects"]
metadata/at_6 = "Objects/Gone"

[node name="Extra" type="Node2D" parent="Objects/Gone"]
metadata/at_7 = "Objects/Gone/Extra"

[node name="Door_8" parent="Objects"]
metadata/at_8 = "Objects/Door_8"

[node name="Chest" parent="Objects"]
metadata/at_9 = "Objects/Chest"

[node name="Shape" parent="Objects/Chest"]
disabled = true
metadata/at_10 = "Objects/Chest/Shape"

[node name="Bulb" type="Node2D" parent="Objects/Chest"]
metadata/at_11 = "Objects/Chest/Bulb"

[node name="Gate" parent="Objects"]
metadata/at_12 = "Objects/Gate"

[node name="Player" type="Node2D" parent="Objects"]
metadata/at_13 = "Objects/Player"

[node name="Gun" type="Node2D" parent="Objects/Player"]
metadata/at_14 = "Objects/Player/Gun"

[node name="Box" parent="Doomed"]
metadata/at_15 = "Doomed/Box"

[node name="Pet" type="Node2D" parent="Doomed"]
metadata/at_16 = "Doomed/Pet"

[node name="Bird" type="Node2D" parent="sky/sky"]
metadata/at_17 = "sky/sky/Bird"

[connection signal="visibility_changed" from="Objects/Lamp" to="Objects/Chest" method="hide"]
[connection signal="visibility_changed" from="Objects/Player" to="." method="hide"]
[connection signal="visibility_changed" from="Other/Sign" to="ground" method="hide"]
[connection signal="visibility_changed" from="Other/Sign" to="Objects/Player/Gun" method="hide"]
"#;

/// [`LEVEL`] as Godot 4.7.2 saves it after loading it (headless, load and pack): overrides carry
/// the base node's unique_id, and nodes whose parent isn't in the file their parent's id.
const LEVEL_SAVED: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://levels/base.tscn" id="1_80vhd"]

[node name="Level" unique_id=1666501156 instance=ExtResource("1_80vhd")]

[node name="ground" parent="." index="0" unique_id=1073741825]
modulate = Color(1, 0, 0, 1)
metadata/at_1 = "ground"

[node name="deco" parent="." index="1" unique_id=1073741831]
tile_map_data = PackedByteArray(0, 0, 3, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0)
metadata/at_2 = "deco"

[node name="Door" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="0" unique_id=536870913]
metadata/at_3 = "Objects/Door"

[node name="rectangle_2" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="1" unique_id=536870914]
metadata/at_4 = "Objects/rectangle_2"

[node name="Shape" parent="Objects/Lamp" parent_id_path=PackedInt32Array(536870915) index="0" unique_id=1880463632]
disabled = true
metadata/at_5 = "Objects/Lamp/Shape"

[node name="Gone" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="3" unique_id=536870916]
metadata/at_6 = "Objects/Gone"

[node name="Extra" type="Node2D" parent="Objects/Gone" index="0" unique_id=475063837]
metadata/at_7 = "Objects/Gone/Extra"

[node name="Door_8" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="4" unique_id=536870920]
metadata/at_8 = "Objects/Door_8"

[node name="Chest" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="5" unique_id=536870921]
metadata/at_9 = "Objects/Chest"

[node name="Shape" parent="Objects/Chest" index="0" unique_id=1754373571]
disabled = true
metadata/at_10 = "Objects/Chest/Shape"

[node name="Bulb" type="Node2D" parent="Objects/Chest" index="1" unique_id=410154200]
metadata/at_11 = "Objects/Chest/Bulb"

[node name="Gate" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="6" unique_id=536870924]
metadata/at_12 = "Objects/Gate"

[node name="Player" type="Node2D" parent="Objects" parent_id_path=PackedInt32Array(1073741826) index="8" unique_id=401561480]
metadata/at_13 = "Objects/Player"

[node name="Gun" type="Node2D" parent="Objects/Player" index="0" unique_id=793560646]
metadata/at_14 = "Objects/Player/Gun"

[node name="Box" parent="Doomed" parent_id_path=PackedInt32Array(1073741834) index="0" unique_id=536870923]
metadata/at_15 = "Doomed/Box"

[node name="Pet" type="Node2D" parent="Doomed" parent_id_path=PackedInt32Array(1073741834) index="1" unique_id=1001687553]
metadata/at_16 = "Doomed/Pet"

[node name="Bird" type="Node2D" parent="sky/sky" parent_id_path=PackedInt32Array(1073741828) index="0" unique_id=1834182676]
metadata/at_17 = "sky/sky/Bird"

[connection signal="visibility_changed" from="Objects/Lamp" to="Objects/Chest" method="hide"]
[connection signal="visibility_changed" from="Objects/Player" to="." method="hide"]
[connection signal="visibility_changed" from="Other/Sign" to="ground" method="hide"]
[connection signal="visibility_changed" from="Other/Sign" to="Objects/Player/Gun" method="hide"]
"#;

/// A scene that instances the export inside a node of its own, with its children editable.
const MAIN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://levels/base.tscn" id="1_base"]

[node name="Main" type="Node2D"]

[node name="World" type="Node2D" parent="."]
metadata/at_1 = "World"

[node name="Desert" parent="World" instance=ExtResource("1_base")]
metadata/at_2 = "World/Desert"

[node name="Door_8" parent="World/Desert/Objects"]
metadata/at_3 = "World/Desert/Objects/Door_8"

[node name="Hero" type="Node2D" parent="World/Desert/Objects"]
metadata/at_4 = "World/Desert/Objects/Hero"

[node name="Camera" type="Camera2D" parent="."]
metadata/at_5 = "Camera"

[connection signal="visibility_changed" from="World/Desert/Objects/Lamp" to="Camera" method="hide"]
[connection signal="visibility_changed" from="World/Desert/Other/Sign" to="Camera" method="hide"]
[connection signal="visibility_changed" from="Camera" to="World" method="hide"]

[editable path="World/Desert"]
"#;

use Fate::*;

type Expected = Vec<(Fate, &'static str, Option<&'static str>, Option<Element>)>;

/// The findings for [`LEVEL_SAVED`] once V1's export becomes V2's: what Godot 4.7.2 does (the
/// headless test below checks each).
fn saved_expected() -> Expected {
    let (layer, object) = (Element::Layer, Element::Object);
    vec![
        (Follows, "deco", Some("decor"), Some(layer(7))),
        (FrozenTiles, "deco", Some("decor"), Some(layer(7))),
        // Found by name first: the Door that is left.
        (
            Retargeted,
            "Objects/Door",
            Some("Things/Door"),
            Some(object(1)),
        ),
        (
            Follows,
            "Objects/rectangle_2",
            Some("Other/rectangle_2"),
            Some(object(2)),
        ),
        (
            Follows,
            "Objects/Lamp/Shape",
            Some("Things/Lamp/Shape"),
            Some(object(3)),
        ),
        (Dropped, "Objects/Gone", None, Some(object(4))),
        (
            MovedToRoot,
            "Objects/Gone/Extra",
            Some("Objects_Gone#Extra"),
            Some(object(4)),
        ),
        (
            Follows,
            "Objects/Door_8",
            Some("Things/Door"),
            Some(object(8)),
        ),
        (
            Follows,
            "Objects/Chest",
            Some("Things/Coffer"),
            Some(object(9)),
        ),
        // Saved by path, as its parent is in the file too.
        (
            Renamed,
            "Objects/Chest/Shape",
            Some("Things/Coffer/Objects_Chest#Shape"),
            Some(object(9)),
        ),
        (
            MovedToRoot,
            "Objects/Chest/Bulb",
            Some("Objects_Chest#Bulb"),
            Some(object(9)),
        ),
        (
            Retargeted,
            "Objects/Gate",
            Some("Things/Gate"),
            Some(object(12)),
        ),
        (
            Follows,
            "Objects/Player",
            Some("Things/Player"),
            Some(layer(2)),
        ),
        (
            MovedToRoot,
            "Objects/Player/Gun",
            Some("Objects_Player#Gun"),
            None,
        ),
        // Its layer's id is gone, so its path finds the new layer, and the new Box there.
        (
            Retargeted,
            "Doomed/Box",
            Some("Doomed/Box"),
            Some(object(11)),
        ),
        (
            Follows,
            "sky/sky/Bird",
            Some("clouds/clouds/Bird"),
            Some(layer(4)),
        ),
        (
            Disconnected,
            "visibility_changed: Objects/Lamp → Objects/Chest",
            Some("Things/Lamp → Things/Coffer"),
            Some(object(3)),
        ),
        (
            Disconnected,
            "visibility_changed: Objects/Player → .",
            Some("Things/Player → ."),
            None,
        ),
        (
            Disconnected,
            "visibility_changed: Other/Sign → Objects/Player/Gun",
            Some("Other/Sign → Objects_Player#Gun"),
            None,
        ),
    ]
}

/// The findings for [`LEVEL`], which has no unique_ids: found by path alone.
fn text_expected() -> Expected {
    let (layer, object) = (Element::Layer, Element::Object);
    vec![
        (Lost, "deco", Some("decor"), Some(layer(7))),
        (Dropped, "Objects/Door", None, Some(object(1))),
        (
            Lost,
            "Objects/rectangle_2",
            Some("Other/rectangle_2"),
            Some(object(2)),
        ),
        (
            Lost,
            "Objects/Lamp/Shape",
            Some("Things/Lamp/Shape"),
            Some(object(3)),
        ),
        (Dropped, "Objects/Gone", None, Some(object(4))),
        (
            MovedToRoot,
            "Objects/Gone/Extra",
            Some("Objects_Gone#Extra"),
            Some(object(4)),
        ),
        (Lost, "Objects/Door_8", Some("Things/Door"), Some(object(8))),
        (
            Lost,
            "Objects/Chest",
            Some("Things/Coffer"),
            Some(object(9)),
        ),
        (
            Lost,
            "Objects/Chest/Shape",
            Some("Things/Coffer/Shape"),
            Some(object(9)),
        ),
        (
            MovedToRoot,
            "Objects/Chest/Bulb",
            Some("Objects_Chest#Bulb"),
            Some(object(9)),
        ),
        (
            Lost,
            "Objects/Gate",
            Some("Things/Portal"),
            Some(object(12)),
        ),
        (
            MovedToRoot,
            "Objects/Player",
            Some("Objects#Player"),
            Some(layer(2)),
        ),
        (
            MovedToRoot,
            "Objects/Player/Gun",
            Some("Objects_Player#Gun"),
            None,
        ),
        (
            Retargeted,
            "Doomed/Box",
            Some("Doomed/Box"),
            Some(object(11)),
        ),
        (
            MovedToRoot,
            "sky/sky/Bird",
            Some("sky_sky#Bird"),
            Some(layer(4)),
        ),
        (
            Disconnected,
            "visibility_changed: Objects/Lamp → Objects/Chest",
            Some("Things/Lamp → Things/Coffer"),
            Some(object(3)),
        ),
        (
            Disconnected,
            "visibility_changed: Objects/Player → .",
            Some("Objects#Player → ."),
            None,
        ),
        (
            Disconnected,
            "visibility_changed: Other/Sign → Objects/Player/Gun",
            Some("Other/Sign → Objects_Player#Gun"),
            None,
        ),
    ]
}

/// Renames, moves, deletions, a painted layer and connections, in a level scene Godot saved
/// (with unique_ids), one written by hand (without), and one that instances the export.
#[test]
fn each_change_to_the_map_gives_the_finding_godot_acts_on() {
    let dir = project_with_ids("level-findings", true);
    let first = export_base(&dir, V1);
    assert!(first.level_scenes.is_empty(), "no level scene yet");
    std::fs::write(dir.join("levels/level.tscn"), LEVEL_SAVED).unwrap();
    std::fs::write(dir.join("levels/level_text.tscn"), LEVEL).unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN).unwrap();
    // Unchanged: nothing moves, but the painted layer is still frozen.
    let same = export_base(&dir, V1);
    assert_eq!(
        seen(findings(&same, "level.tscn")),
        [(FrozenTiles, "deco", Some("deco"), Some(Element::Layer(7)))]
    );
    assert_eq!(
        seen(findings(&same, "level_text.tscn")),
        [(FrozenTiles, "deco", Some("deco"), Some(Element::Layer(7)))]
    );
    assert_eq!(seen(findings(&same, "main.tscn")), []);

    let report = export_base(&dir, V2);
    assert_eq!(report.level_scenes.len(), 3, "{:?}", report.level_scenes);
    assert_eq!(seen(findings(&report, "level.tscn")), saved_expected());
    assert_eq!(seen(findings(&report, "level_text.tscn")), text_expected());
    assert_eq!(
        seen(findings(&report, "main.tscn")),
        [
            (
                Lost,
                "World/Desert/Objects/Door_8",
                Some("World/Desert/Things/Door"),
                Some(Element::Object(8))
            ),
            (
                MovedToRoot,
                "World/Desert/Objects/Hero",
                Some("World_Desert_Objects#Hero"),
                Some(Element::Layer(2))
            ),
            (
                Disconnected,
                "visibility_changed: World/Desert/Objects/Lamp → Camera",
                Some("World/Desert/Things/Lamp → Camera"),
                Some(Element::Object(3))
            ),
        ]
    );
    // The level scenes are only read.
    assert_eq!(
        std::fs::read_to_string(dir.join("levels/level.tscn")).unwrap(),
        LEVEL_SAVED
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("levels/level_text.tscn")).unwrap(),
        LEVEL
    );

    // godot::status, after the export, says what Godot will do too. The export it replaced is
    // gone, so where a level names a node by path alone (a connection here) it can't say which
    // node that was.
    let status = status(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("levels/base.tscn"),
    )
    .unwrap();
    assert_eq!(status.summary(), Summary::UpToDate);
    let level = status
        .level_scenes
        .iter()
        .find(|l| l.path.ends_with("levels/level.tscn"))
        .unwrap();
    let mut expected = saved_expected();
    expected[16] = (
        Disconnected,
        "visibility_changed: Objects/Lamp → Objects/Chest",
        None,
        Some(Element::Object(9)),
    );
    assert_eq!(seen(level), expected);
    std::fs::remove_dir_all(&dir).ok();
}

/// One note per kind of finding and level scene, naming the nodes and what to do, with the
/// elements the editor's Issues jump to.
#[test]
fn findings_become_one_note_per_kind() {
    let dir = project_with_ids("level-notes", true);
    export_base(&dir, V1);
    std::fs::write(dir.join("levels/level.tscn"), LEVEL_SAVED).unwrap();
    std::fs::write(dir.join("levels/level_text.tscn"), LEVEL).unwrap();
    let report = export_base(&dir, V2);
    let notes: Vec<&Note> = report
        .notes
        .iter()
        .filter(|n| n.code.as_str().starts_with("godot/level-scene-"))
        .collect();
    let kinds: Vec<(&str, Severity)> = notes
        .iter()
        .map(|n| (n.code.as_str(), n.severity))
        .collect();
    use Severity::{Info, Warning};
    assert_eq!(
        kinds,
        [
            // level.tscn
            ("godot/level-scene-follows", Info),
            ("godot/level-scene-renamed", Warning),
            ("godot/level-scene-retargeted", Warning),
            ("godot/level-scene-dropped", Warning),
            ("godot/level-scene-moved-to-root", Warning),
            ("godot/level-scene-tiles", Warning),
            ("godot/level-scene-connection", Warning),
            // level_text.tscn
            ("godot/level-scene-lost", Warning),
            ("godot/level-scene-retargeted", Warning),
            ("godot/level-scene-dropped", Warning),
            ("godot/level-scene-moved-to-root", Warning),
            ("godot/level-scene-connection", Warning),
        ]
    );
    let message = |i: usize| notes[i].message.as_str();
    assert_eq!(
        message(0),
        "level.tscn: Godot finds 7 nodes again by unique_id where this export renames or moves them: deco → decor (layer 7), Objects/rectangle_2 → Other/rectangle_2 (object 2), Objects/Lamp/Shape → Things/Lamp/Shape (object 3), Objects/Door_8 → Things/Door (object 8), Objects/Chest → Things/Coffer (object 9), and 2 more. Save level.tscn once in Godot so it keeps the new paths"
    );
    assert_eq!(
        notes[0].elements,
        [
            Element::Layer(7),
            Element::Object(2),
            Element::Object(3),
            Element::Object(8),
            Element::Object(9),
            Element::Layer(2),
            Element::Layer(4)
        ]
    );
    assert_eq!(
        message(1),
        "level.tscn: Godot finds a node by unique_id but not by its parent's path, so it keeps the override and renames it: Objects/Chest/Shape → Things/Coffer/Objects_Chest#Shape (object 9). Scripts that find it by path need the new name; an override of a node and of its parent in one level scene is saved by path, so avoid that"
    );
    assert_eq!(
        message(2),
        "level.tscn overrides 3 nodes that this export no longer has where level.tscn looks, and Godot applies the overrides to the node that took their name instead: Objects/Door → Things/Door (object 1), Objects/Gate → Things/Gate (object 12), Doomed/Box (object 11). Remove or move those overrides in level.tscn"
    );
    assert_eq!(
        message(3),
        "level.tscn overrides a node that base.tscn no longer has, and Godot drops those overrides: Objects/Gone (object 4)"
    );
    assert_eq!(
        message(4),
        "level.tscn adds 3 nodes under nodes Godot can't find by path in base.tscn any more, so Godot moves them to the root: Objects/Gone/Extra → Objects_Gone#Extra (object 4), Objects/Chest/Bulb → Objects_Chest#Bulb (object 9), Objects/Player/Gun → Objects_Player#Gun. Put them back in Godot"
    );
    assert_eq!(
        message(5),
        "level.tscn paints its own tiles on a layer of base.tscn: Godot shows those instead, so what the map paints there stops showing: deco → decor (layer 7). Paint on a TileMapLayer of your own that uses the same TileSet, and remove the override"
    );
    assert_eq!(
        message(6),
        "Godot drops 3 signal connections of level.tscn, because a node at either end is renamed, moved or gone and connections are found by path: visibility_changed: Objects/Lamp → Objects/Chest (object 3), now Things/Lamp → Things/Coffer, visibility_changed: Objects/Player → ., now Things/Player → ., visibility_changed: Other/Sign → Objects/Player/Gun, now Other/Sign → Objects_Player#Gun. Connect them again in Godot"
    );
    assert_eq!(
        message(7),
        "level_text.tscn has overrides of 7 nodes that Godot finds by path, which this export changes, so they will be lost: deco → decor (layer 7), Objects/rectangle_2 → Other/rectangle_2 (object 2), Objects/Lamp/Shape → Things/Lamp/Shape (object 3), Objects/Door_8 → Things/Door (object 8), Objects/Chest → Things/Coffer (object 9), and 2 more. Before Godot saves level_text.tscn, rename them back in the map and export again, save level_text.tscn once in Godot (it then finds nodes by unique_id), and rename them again"
    );

    // A project for Godot before 4.6 gets no unique_ids: paths are all there is.
    let old = project_with_ids("level-notes-no-ids", false);
    export_base(&old, V1);
    std::fs::write(old.join("levels/level_text.tscn"), LEVEL).unwrap();
    let report = export_base(&old, V2);
    assert_eq!(seen(findings(&report, "level_text.tscn")), text_expected());
    let lost = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/level-scene-lost")
        .unwrap();
    assert!(lost.message.ends_with("Godot projects before 4.6 find nodes by path only: rename them back, or move the overrides to the new paths"), "{}", lost.message);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&old).ok();
}

/// An export from before node names came from ids (counter names, no unique_ids): each override
/// the renaming would lose is named by object id, found by the old scene's metadata (Decision 6).
#[test]
fn overrides_of_counter_named_nodes_are_named_by_object_id() {
    let dir = project_with_ids("level-counter-names", false);
    export_base(&dir, V1);
    // The export as an earlier better-tiled wrote it: unnamed objects numbered by order.
    let base = dir.join("levels/base.tscn");
    let old = std::fs::read_to_string(&base)
        .unwrap()
        .replace("\"rectangle_2\"", "\"rectangle\"")
        .replace(
            "parent=\"Objects/rectangle_2\"",
            "parent=\"Objects/rectangle\"",
        );
    std::fs::write(&base, &old).unwrap();
    let level = "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://levels/base.tscn\" id=\"1\"]\n\n[node name=\"Level\" instance=ExtResource(\"1\")]\n\n[node name=\"rectangle\" parent=\"Objects\"]\nposition = Vector2(1, 2)\n";
    std::fs::write(dir.join("levels/level.tscn"), level).unwrap();
    let report = export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &base,
        &Options {
            replace: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(
        seen(findings(&report, "level.tscn")),
        [(
            Lost,
            "Objects/rectangle",
            Some("Objects/rectangle_2"),
            Some(Element::Object(2))
        )]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// An export from when a parallax group was a Parallax2D itself, its layers in Parallax2Ds of
/// their own only for a factor of their own: now a layer without one gets a holder at its path,
/// and one whose factor undoes its group's loses its holder. Overrides of a layer that the holder
/// took the place of are retargeted; those of a layer under a holder that went are lost.
#[test]
fn overrides_follow_parallax_groups_that_change_shape() {
    let dir = project_with_ids("level-parallax-groups", false);
    let base = dir.join("levels/base.tscn");
    export_base(&dir, super::layers::PARALLAX_GROUPS);
    let now = std::fs::read_to_string(&base).unwrap();
    // The export as the earlier better-tiled wrote it.
    let old = now
        .replace("[node name=\"far\" type=\"Node2D\" parent=\".\"]", "[node name=\"far\" type=\"Parallax2D\" parent=\".\"]\nscroll_scale = Vector2(0.5, 0.5)")
        .replace("[node name=\"along\" type=\"Parallax2D\" parent=\"far\"]\nscroll_scale = Vector2(0.5, 0.5)\n\n", "")
        .replace("parent=\"far/along\"", "parent=\"far\"")
        .replace("[node name=\"back\" type=\"TileMapLayer\" parent=\"fast\"]", "[node name=\"back\" type=\"Parallax2D\" parent=\"fast\"]\nscroll_scale = Vector2(1.5, 1)\n\n[node name=\"back\" type=\"TileMapLayer\" parent=\"fast/back\"]");
    let sections = |text: &str| text.lines().filter(|l| l.starts_with("[node ")).count();
    assert_eq!(
        (sections(&old), old.matches("type=\"Parallax2D\"").count()),
        (sections(&now), 4),
        "{old}"
    );
    std::fs::write(&base, &old).unwrap();
    let level = "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://levels/base.tscn\" id=\"1\"]\n\n[node name=\"Level\" instance=ExtResource(\"1\")]\n\n[node name=\"along\" parent=\"far\"]\nmodulate = Color(1, 0, 0, 1)\n\n[node name=\"back\" parent=\"fast\"]\nscroll_offset = Vector2(4, 0)\n\n[node name=\"back\" parent=\"fast/back\"]\nmodulate = Color(0, 1, 0, 1)\n";
    std::fs::write(dir.join("levels/level.tscn"), level).unwrap();
    let report = export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &base,
        &Options {
            replace: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(
        seen(findings(&report, "level.tscn")),
        [
            (
                Retargeted,
                "far/along",
                Some("far/along"),
                Some(Element::Layer(3))
            ),
            (
                Retargeted,
                "fast/back",
                Some("fast/back"),
                Some(Element::Layer(6))
            ),
            (
                Lost,
                "fast/back/back",
                Some("fast/back"),
                Some(Element::Layer(6))
            ),
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Overrides of objects AutoMap placed (better-tiled.json's record of them) are noted, while
/// that record still matches the map.
#[test]
fn overrides_of_objects_automap_placed_are_noted() {
    let dir = project_with_ids("level-automap", true);
    let map_path = dir.join("maps/base.tmx");
    export_base(&dir, V1);
    std::fs::write(dir.join("levels/level.tscn"), LEVEL_SAVED).unwrap();
    let record = |checksum: String| {
        let file = dir.join("maps/better-tiled.json");
        let painted = PaintedEntry {
            layer: 2,
            name: "Objects".to_owned(),
            checksum,
            objects: true,
            generated: vec![9, 3],
            ..PaintedEntry::default()
        };
        let automap = AutoMapEntry {
            rules: "rules.txt".to_owned(),
            painted: vec![painted],
            ..AutoMapEntry::default()
        };
        std::fs::create_dir_all(dir.join("locks")).unwrap();
        let key = bt_project::paths::key(&file, &map_path).unwrap();
        ProjectData::edit(&file, &dir.join("locks"), |data| {
            data.set_automap(&key, automap)
        })
        .unwrap();
    };
    let map = bt_tmx::Loader::builder()
        .build()
        .load_map(&map_path)
        .unwrap()
        .value;
    let objects = &map
        .layer(LayerId(2))
        .unwrap()
        .as_object_layer()
        .unwrap()
        .objects;
    record(bt_project::checksum::objects(&map, objects));
    let report = export_base(&dir, V1);
    let automap: Vec<_> = seen(findings(&report, "level.tscn"))
        .into_iter()
        .filter(|f| f.0 == AutoMapPlaced)
        .collect();
    let chest = |node| (AutoMapPlaced, node, Some(node), Some(Element::Object(9)));
    assert_eq!(
        automap,
        [
            (
                AutoMapPlaced,
                "Objects/Lamp/Shape",
                Some("Objects/Lamp/Shape"),
                Some(Element::Object(3))
            ),
            chest("Objects/Chest"),
            chest("Objects/Chest/Shape")
        ]
    );
    let note = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/level-scene-automap")
        .unwrap();
    assert_eq!(note.severity, Severity::Info);
    assert_eq!(
        note.message,
        "level.tscn overrides 3 nodes that AutoMap placed: Godot keeps those overrides only while the rules place them the same way: Objects/Lamp/Shape (object 3), Objects/Chest (object 9), Objects/Chest/Shape (object 9)"
    );
    // The map changed since (in Tiled, say): the record no longer says which objects the rules
    // placed.
    record("0000000000000000".to_owned());
    let report = export_base(&dir, V1);
    assert!(
        findings(&report, "level.tscn")
            .findings
            .iter()
            .all(|f| f.fate != AutoMapPlaced)
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Which scenes are level scenes: those that inherit or instance the export, by path or by uid
/// (a stale path still finds it), not generated scenes or scenes that only mention it.
#[test]
fn level_scenes_are_found_by_path_or_uid() {
    let dir = project_with_ids("level-which", true);
    export_base(&dir, V1);
    let tscn = std::fs::read_to_string(dir.join("levels/base.tscn")).unwrap();
    let uid = read::parse_header(&tscn).unwrap().uid().unwrap();
    // An override as Godot saves it: found by its parent's id and its own.
    let scene = |ext: &str, root: &str| {
        let door = "[node name=\"Door_8\" parent=\"Objects\" parent_id_path=PackedInt32Array(1073741826) unique_id=536870920]\nposition = Vector2(1, 2)\n";
        format!(
            "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" {ext} id=\"1\"]\n\n{root}\n\n{door}"
        )
    };
    let inherit = "[node name=\"Level\" instance=ExtResource(\"1\")]";
    std::fs::write(
        dir.join("levels/by_path.tscn"),
        scene("path=\"res://levels/base.tscn\"", inherit),
    )
    .unwrap();
    std::fs::write(
        dir.join("by_uid.tscn"),
        scene(
            &format!("uid=\"{uid}\" path=\"res://old/base.tscn\""),
            inherit,
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("spawner.tscn"),
        scene(
            "path=\"res://levels/base.tscn\"",
            "[node name=\"Spawner\" type=\"Node2D\"]\nmetadata/level = ExtResource(\"1\")",
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("other.tscn"),
        scene("path=\"res://levels/other.tscn\"", inherit),
    )
    .unwrap();
    let generated = format!(
        "; better-tiled-export 1 fnv1a64=0000000000000000\n{}",
        scene("path=\"res://levels/base.tscn\"", inherit)
    );
    std::fs::write(dir.join("generated.tscn"), generated).unwrap();
    // A level scene Godot couldn't read either: its start is fine, but a node has no name.
    std::fs::write(
        dir.join("broken.tscn"),
        format!(
            "{}\n[node parent=\".\"]\n",
            scene("path=\"res://levels/base.tscn\"", inherit)
        ),
    )
    .unwrap();

    let report = export_base(&dir, V2);
    let mut found: Vec<PathBuf> = report
        .level_scenes
        .iter()
        .map(|l| l.path.strip_prefix(&dir).unwrap().to_path_buf())
        .collect();
    found.sort();
    assert_eq!(
        found,
        ["broken.tscn", "by_uid.tscn", "levels/by_path.tscn"].map(PathBuf::from)
    );
    for name in ["by_path.tscn", "by_uid.tscn"] {
        assert_eq!(
            seen(findings(&report, name)),
            [(
                Follows,
                "Objects/Door_8",
                Some("Things/Door"),
                Some(Element::Object(8))
            )],
            "{name}"
        );
    }
    let broken = findings(&report, "broken.tscn");
    assert!(
        broken.findings.is_empty()
            && broken.unreadable.as_deref() == Some("line 10: [node] has no name"),
        "{broken:?}"
    );
    let note = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/level-scene-unreadable")
        .unwrap();
    assert!(
        note.message
            .contains("broken.tscn to see what Godot will do with it once base.tscn changes"),
        "{}",
        note.message
    );

    // A level scene already as the export now is: listed, with nothing to say.
    let current = scene("path=\"res://levels/base.tscn\"", inherit)
        .replace("parent=\"Objects\"", "parent=\"Things\"")
        .replace("Door_8", "Door");
    std::fs::write(dir.join("levels/by_path.tscn"), current).unwrap();
    let report = export_base(&dir, V2);
    assert_eq!(seen(findings(&report, "by_path.tscn")), []);
    assert!(
        report
            .notes
            .iter()
            .all(|n| !n.message.starts_with("by_path.tscn"))
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Loads a scene in Godot and says where each `at_<n>` marker is, which signal connections
/// there are, and each TileMapLayer's cells.
const WHERE: &str = r#"extends SceneTree

func _init() -> void:
	var room: Node = (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	var markers := {}
	var connections := []
	var cells := {}
	for node: Node in [room] + room.find_children("*", "", true, false):
		var path := String(room.get_path_to(node))
		for key: StringName in node.get_meta_list():
			if String(key).begins_with("at_"):
				var at: String = node.get_meta(key)
				if not markers.has(at):
					markers[at] = []
				markers[at].append(path)
		if node is TileMapLayer:
			var used := []
			for c: Vector2i in (node as TileMapLayer).get_used_cells():
				used.append([c.x, c.y])
			used.sort()
			cells[path] = used
		for s: Dictionary in node.get_signal_list():
			for c: Dictionary in node.get_signal_connection_list(s["name"]):
				if c["flags"] & CONNECT_PERSIST:
					var callable: Callable = c["callable"]
					var target := callable.get_object() as Node
					connections.append([path, String(s["name"]), String(room.get_path_to(target)), String(callable.get_method())])
	print("BT-JSON-BEGIN", JSON.stringify({"markers": markers, "connections": connections, "cells": cells}), "BT-JSON-END")
	room.free()
	quit()
"#;

/// Loads a scene as Godot's editor saves one: instantiated for editing, packed and saved.
const PACK: &str = r#"extends SceneTree

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	var scene: Node = (load(args[0]) as PackedScene).instantiate(PackedScene.GEN_EDIT_STATE_MAIN)
	var packed := PackedScene.new()
	var result := {"packed": packed.pack(scene)}
	result["saved"] = ResourceSaver.save(packed, args[1])
	scene.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// Where Godot should put each marked node of `level` (by its marker), and which connections it
/// should keep, given the findings.
fn predicted(
    level: &str,
    findings: &[Finding],
) -> (HashMap<String, Option<String>>, BTreeSet<Vec<String>>) {
    let resource = read::parse(level).unwrap();
    let mut at = HashMap::new();
    for (node, section) in resource.nodes() {
        let Some(marker) = section
            .props
            .iter()
            .find(|(k, _)| k.starts_with("metadata/at_"))
            .and_then(|(_, v)| read::string(v))
        else {
            continue;
        };
        let fate = findings.iter().find(|f| {
            f.node == node.path() && !matches!(f.fate, FrozenTiles | AutoMapPlaced | TypeChanged)
        });
        let place = match fate {
            Some(f) if matches!(f.fate, Follows | Renamed | Retargeted | MovedToRoot) => {
                f.now.clone()
            }
            Some(f) if matches!(f.fate, Lost | Dropped) => None,
            _ => Some(node.path()),
        };
        at.insert(marker, place);
    }
    let dropped: Vec<&str> = findings
        .iter()
        .filter(|f| f.fate == Disconnected)
        .map(|f| f.node.as_str())
        .collect();
    let kept = resource
        .connections()
        .filter(|c| !dropped.contains(&format!("{}: {} → {}", c.signal, c.from, c.to).as_str()))
        .map(|c| vec![c.from, c.signal, c.to, c.method])
        .collect();
    (at, kept)
}

/// Each finding against Godot: after the export changes, Godot loads each level scene, and the
/// overrides and added nodes are where the findings say (followed, renamed, landed on another
/// node, moved to the root) or gone (lost, dropped), the connections said to be dropped are,
/// and a painted layer shows the level's tiles.
#[test]
fn godot_does_what_the_level_scene_check_predicts() {
    let Some(godot) = godot() else { return };
    let dir = project_with_ids("level-godot", true);
    export_base(&dir, V1);
    std::fs::write(dir.join("levels/level_text.tscn"), LEVEL).unwrap();
    std::fs::write(dir.join("main_text.tscn"), MAIN).unwrap();
    run_godot(&godot, &dir, &["--import"]);
    std::fs::write(dir.join("pack.gd"), PACK).unwrap();
    for (from, to) in [
        ("res://levels/level_text.tscn", "res://levels/level.tscn"),
        ("res://main_text.tscn", "res://main.tscn"),
    ] {
        let (stdout, stderr) =
            run_godot(&godot, &dir, &["--script", "res://pack.gd", "--", from, to]);
        assert_eq!(
            json_between_markers(&stdout, &stderr),
            serde_json::json!({"packed": 0, "saved": 0}),
            "{stderr}"
        );
    }
    let saved = std::fs::read_to_string(dir.join("levels/level.tscn")).unwrap();
    assert!(saved.contains("parent_id_path="), "{saved}");

    let report = export_base(&dir, V2);
    assert_eq!(
        seen(findings(&report, "level.tscn")),
        saved_expected(),
        "Godot saves level scenes as LEVEL_SAVED has it"
    );
    run_godot(&godot, &dir, &["--import"]);
    std::fs::write(dir.join("where.gd"), WHERE).unwrap();
    for (name, res) in [
        ("level.tscn", "res://levels/level.tscn"),
        ("level_text.tscn", "res://levels/level_text.tscn"),
        ("main.tscn", "res://main.tscn"),
        ("main_text.tscn", "res://main_text.tscn"),
    ] {
        let level = findings(&report, name);
        let text = std::fs::read_to_string(&level.path).unwrap();
        let (stdout, stderr) = run_godot(&godot, &dir, &["--script", "res://where.gd", "--", res]);
        let result = json_between_markers(&stdout, &stderr);
        let (at, kept) = predicted(&text, &level.findings);
        assert!(!at.is_empty());
        for (marker, place) in &at {
            let found: Vec<&str> = result["markers"]
                .get(marker)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            assert_eq!(
                found,
                place.iter().map(String::as_str).collect::<Vec<_>>(),
                "{name}: where Godot puts {marker}\n{:?}\n{stderr}",
                level.findings
            );
        }
        let connections: BTreeSet<Vec<String>> = result["connections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                c.as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap().to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(connections, kept, "{name}: the connections Godot keeps");
        if name.starts_with("level") {
            let frozen = level.findings.iter().any(|f| f.fate == FrozenTiles);
            let tiles = if frozen {
                serde_json::json!([[1, 1]])
            } else {
                serde_json::json!([[3, 0]])
            };
            assert_eq!(result["cells"]["decor"], tiles, "{name}: the painted layer");
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// The check runs on every export, so on every save when the editor exports on save: the
/// Highlands Sample scene with 200 level scenes, timed (reported, not asserted).
#[test]
fn the_level_scene_check_on_the_highlands_sample_with_200_level_scenes() {
    let Some(path) = bt_test_data::test_data(
        "assets/Epic RPG World - Highlands V1.4.1/TiledMap Editor/Sample scene.tmx",
    ) else {
        return;
    };
    let dir = project_with_ids("level-timing", true);
    let target = dir.join("levels/sample.tscn");
    let timed = |runs: usize| {
        let mut best = f64::MAX;
        let mut report = None;
        for _ in 0..runs {
            let start = Instant::now();
            report = Some(
                export(
                    &path,
                    &mut bt_tmx::Loader::builder().build(),
                    &target,
                    &Options::default(),
                )
                .unwrap(),
            );
            best = best.min(start.elapsed().as_secs_f64() * 1000.0);
        }
        (best, report.unwrap())
    };
    timed(1);
    let (alone, _) = timed(3);
    // Each level overrides every top-level layer, adds a node under the first and connects a
    // signal.
    let scene = std::fs::read_to_string(&target).unwrap();
    let layers: Vec<(String, i32)> = read::parse(&scene)
        .unwrap()
        .nodes()
        .filter(|(n, _)| n.parent.as_deref() == Some("."))
        .map(|(n, _)| (n.name, n.unique_id.unwrap_or(0)))
        .collect();
    for i in 0..200 {
        let mut text = format!(
            "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://levels/sample.tscn\" id=\"1\"]\n\n[node name=\"Level{i}\" instance=ExtResource(\"1\")]\n"
        );
        for (layer, uid) in &layers {
            text.push_str(&format!("\n[node name=\"{layer}\" parent=\".\" unique_id={uid}]\nmodulate = Color(1, 1, 1, 0.5)\n"));
        }
        text.push_str(&format!("\n[node name=\"Player\" type=\"Node2D\" parent=\"{}\"]\n\n[connection signal=\"visibility_changed\" from=\"{0}/Player\" to=\".\" method=\"hide\"]\n", layers[0].0));
        std::fs::write(dir.join(format!("levels/level_{i:03}.tscn")), text).unwrap();
    }
    let (with_levels, report) = timed(3);
    assert_eq!(report.level_scenes.len(), 200);
    assert!(
        report.level_scenes.iter().all(|l| l.findings.is_empty()),
        "{:?}",
        report.level_scenes[0]
    );
    println!(
        "Highlands Sample scene, an export that writes nothing: {alone:.1} ms; with 200 level scenes of {} overrides each: {with_levels:.1} ms (on-save budget: 100 ms, release build)",
        layers.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Two tile objects and a shape; with `bodies`, `godotNode` makes each a StaticBody2D.
fn typed_map(bodies: bool) -> String {
    let node = |kind: &str| {
        if bodies {
            format!("<properties><property name=\"godotNode\" value=\"{kind}\"/></properties>")
        } else {
            String::new()
        }
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="3" nextobjectid="4">
 <tileset firstgid="1" source="tiles.tsx"/>
 <objectgroup id="2" name="Objects">
  <object id="1" name="Lamp" gid="1" x="0" y="32" width="32" height="32">{}</object>
  <object id="2" name="Torch" gid="2" x="32" y="32" width="32" height="32">{}</object>
  <object id="3" name="Door" x="64" y="0" width="32" height="32">{}</object>
 </objectgroup>
</map>
"#,
        node("StaticBody2D"),
        node("StaticBody2D"),
        node("StaticBody2D")
    )
}

/// A level overriding a sprite's flip, another sprite's position (which a body has too) and an
/// area's monitoring.
const TYPED_LEVEL: &str = "[gd_scene format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://levels/base.tscn\" id=\"1\"]\n\n[node name=\"Level\" instance=ExtResource(\"1\")]\n\n[node name=\"Lamp\" parent=\"Objects\"]\nflip_h = true\n\n[node name=\"Torch\" parent=\"Objects\"]\nposition = Vector2(40, 50)\n\n[node name=\"Door\" parent=\"Objects\"]\nmonitoring = false\n";

/// What Godot made of the level's overrides.
const TYPED_CHECK: &str = r#"extends SceneTree

func _init() -> void:
	var room: Node = (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	var lamp := room.get_node("Objects/Lamp")
	var sprite: Node = lamp if lamp is Sprite2D else lamp.get_node("Sprite")
	var torch := room.get_node("Objects/Torch") as Node2D
	var door := room.get_node("Objects/Door")
	var result := {"lamp": lamp.get_class(), "flip_h": sprite.get("flip_h"), "torch": [torch.position.x, torch.position.y], "door": door.get_class(), "monitoring": door.get("monitoring")}
	room.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// `godotNode` changing the type of a node a level overrides: the override sets what the new
/// type has and Godot drops the rest without a word, so the check warns, saying where a tile
/// object's sprite settings went. An override of only what every node has (a position) is
/// left alone.
#[test]
fn overrides_of_nodes_whose_type_changed_are_noted() {
    let dir = project_with_ids("level-types", true);
    export_base(&dir, &typed_map(false));
    std::fs::write(dir.join("levels/level.tscn"), TYPED_LEVEL).unwrap();
    let godot = godot();
    if let Some(godot) = &godot {
        run_godot(godot, &dir, &["--import"]);
        let before = run_gdscript(godot, &dir, TYPED_CHECK, &["res://levels/level.tscn"]);
        assert_eq!(
            before,
            serde_json::json!({"lamp": "Sprite2D", "flip_h": true, "torch": [40.0, 50.0], "door": "Area2D", "monitoring": false})
        );
    }
    let report = export_base(&dir, &typed_map(true));
    let level = findings(&report, "level.tscn");
    assert_eq!(
        seen(level),
        [
            (
                TypeChanged,
                "Objects/Lamp",
                Some("Objects/Lamp"),
                Some(Element::Object(1))
            ),
            (
                TypeChanged,
                "Objects/Door",
                Some("Objects/Door"),
                Some(Element::Object(3))
            )
        ]
    );
    let types: Vec<_> = level
        .findings
        .iter()
        .map(|f| f.types.clone().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            TypeChange {
                was: "Sprite2D".to_owned(),
                now: "StaticBody2D".to_owned(),
                sprite: Some("Objects/Lamp/Sprite".to_owned())
            },
            TypeChange {
                was: "Area2D".to_owned(),
                now: "StaticBody2D".to_owned(),
                sprite: None
            },
        ]
    );
    let note = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/level-scene-type")
        .unwrap();
    assert_eq!(note.severity, Severity::Warning);
    assert_eq!(
        note.message,
        "level.tscn overrides 2 nodes whose type this export changes, and Godot ignores the overridden settings the new type doesn't have: Objects/Lamp (object 1): Sprite2D → StaticBody2D (sprite settings on Objects/Lamp/Sprite), Objects/Door (object 3): Area2D → StaticBody2D. Move those overrides in level.tscn to the nodes that have the settings now"
    );
    assert_eq!(note.elements, [Element::Object(1), Element::Object(3)]);
    // Once the export is there, the scene it replaced is gone: nothing more to say.
    let again = export_base(&dir, &typed_map(true));
    assert!(findings(&again, "level.tscn").findings.is_empty());

    let Some(godot) = godot else { return };
    run_godot(&godot, &dir, &["--import"]);
    let after = run_gdscript(&godot, &dir, TYPED_CHECK, &["res://levels/level.tscn"]);
    assert_eq!(
        after,
        serde_json::json!({"lamp": "StaticBody2D", "flip_h": false, "torch": [40.0, 50.0], "door": "StaticBody2D", "monitoring": null})
    );
    std::fs::remove_dir_all(&dir).ok();
}
