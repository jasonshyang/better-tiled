//! What the Godot tests share: a scratch Godot project with the test maps, the FEATURES map, a
//! reader for scene sections, and running Godot headless.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

/// A Godot project in a scratch folder, with the committed test maps in `maps/`.
pub(super) fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bt-godot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nconfig/name=\"bt-test\"\n",
    )
    .unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures");
    for entry in std::fs::read_dir(&fixtures).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dir.join("maps").join(entry.file_name())).unwrap();
    }
    std::fs::write(dir.join("maps/features.tmx"), FEATURES).unwrap();
    dir.canonicalize().unwrap()
}

pub(super) fn loader(project: &Path) -> bt_tmx::Loader {
    let types = bt_tmx::load_project(&project.join("maps/game.tiled-project"))
        .unwrap()
        .value
        .custom_types;
    bt_tmx::Loader::builder()
        .custom_types(Arc::new(types))
        .build()
}

/// A map with what the export maps one way or another: flips, an animated tile, a tile bigger
/// than the grid with a collision shape, a group with an offset, tint and opacity, a parallax
/// image layer, an added layer, and objects of every kind with references, files and classes.
pub(super) const FEATURES: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="6" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="8" nextobjectid="9">
 <properties>
  <property name="intent" value="a test room"/>
 </properties>
 <tileset firstgid="1" source="tiles.tsx"/>
 <tileset firstgid="5" name="anim" tilewidth="32" tileheight="32" tilecount="4" columns="2">
  <image source="tiles.png" width="64" height="64"/>
  <tile id="0">
   <animation>
    <frame tileid="0" duration="100"/>
    <frame tileid="1" duration="250"/>
   </animation>
  </tile>
 </tileset>
 <tileset firstgid="9" name="big" tilewidth="64" tileheight="64" tilecount="1" columns="1">
  <image source="tiles.png" width="64" height="64"/>
  <tile id="0">
   <objectgroup draworder="index">
    <object id="1" x="0" y="32" width="64" height="32"/>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="cells" width="6" height="4">
  <data encoding="csv">
1,2147483650,1073741827,536870916,0,0,
5,0,0,9,0,0,
0,0,0,0,0,0,
0,0,0,0,0,3758096385
</data>
 </layer>
 <group id="2" name="group" offsetx="8" opacity="0.5" tintcolor="#ff0000">
  <layer id="3" name="inner" width="6" height="4" visible="0">
   <data encoding="csv">
0,0,0,0,0,1,
0,0,0,0,0,0,
0,0,0,0,0,0,
0,0,0,0,0,0
</data>
  </layer>
 </group>
 <imagelayer id="4" name="bg" parallaxx="0.5" repeatx="1">
  <image source="tiles.png" width="64" height="64"/>
 </imagelayer>
 <layer id="5" name="glow" width="6" height="4" mode="add">
  <data encoding="csv">
0,0,0,0,0,0,
0,0,0,0,0,0,
0,0,0,0,0,0,
2,0,0,0,0,0
</data>
 </layer>
 <objectgroup id="6" name="objects" draworder="index">
  <object id="1" name="crate" type="Box" gid="1" x="0" y="96" width="32" height="32">
   <properties>
    <property name="target" type="object" value="2"/>
   </properties>
  </object>
  <object id="2" name="spawn" type="spawn" x="48" y="48">
   <properties>
    <property name="art" type="file" value="tiles.png"/>
    <property name="enemy" value="slime"/>
    <property name="speed" type="float" value="1.5"/>
    <property name="tint" type="color" value="#ff00ff00"/>
    <property name="wave" type="int" value="2"/>
   </properties>
   <point/>
  </object>
  <object id="3" name="zone" x="64" y="0" width="64" height="32" rotation="30"/>
  <object id="4" name="path" x="0" y="0">
   <polyline points="0,0 32,32 64,0"/>
  </object>
  <object id="5" name="pill" x="96" y="64" width="64" height="32">
   <capsule/>
  </object>
  <object id="6" name="sign" x="0" y="0" width="96" height="20">
   <text pixelsize="12" halign="center">Hello</text>
  </object>
  <object id="7" name="flipped" gid="2147483649" x="64" y="128" width="64" height="64"/>
  <object id="8" name="ring" x="128" y="0" width="20" height="10">
   <ellipse/>
  </object>
 </objectgroup>
</map>
"##;

/// The `[node …]` sections of a scene: (header, lines).
pub(super) fn sections(tscn: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in tscn.lines() {
        if line.starts_with('[') {
            out.push((line.to_owned(), Vec::new()));
        } else if let Some(last) = out.last_mut()
            && !line.is_empty()
        {
            last.1.push(line.to_owned());
        }
    }
    out
}

pub(super) fn node<'t>(
    sections: &'t [(String, Vec<String>)],
    name: &str,
    parent: &str,
) -> &'t [String] {
    let header = |h: &str| {
        h.starts_with(&format!("[node name=\"{name}\" "))
            && h.contains(&format!("parent=\"{parent}\""))
    };
    &sections
        .iter()
        .find(|(h, _)| header(h))
        .unwrap_or_else(|| panic!("no node {parent}/{name}"))
        .1
}

pub(super) fn value<'t>(lines: &'t [String], key: &str) -> &'t str {
    let prefix = format!("{key} = ");
    lines
        .iter()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {key} in {lines:?}"))
}

/// The cells of a `tile_map_data`: (x, y, source, atlas x, atlas y, alternative).
pub(super) fn cells(data: &str) -> Vec<[i32; 6]> {
    let bytes: Vec<u8> = data
        .trim_start_matches("PackedByteArray(")
        .trim_end_matches(')')
        .split(", ")
        .map(|b| b.parse().unwrap())
        .collect();
    assert_eq!(&bytes[..2], &[0, 0], "format version 0");
    bytes[2..]
        .chunks(12)
        .map(|c| {
            let v = |i: usize| u16::from_le_bytes([c[i], c[i + 1]]);
            [
                v(0) as i16 as i32,
                v(2) as i16 as i32,
                v(4) as i32,
                v(6) as i32,
                v(8) as i32,
                v(10) as i32,
            ]
        })
        .collect()
}

/// Godot's executable (as `bt godot verify` finds it), or the test is skipped.
pub(super) fn godot() -> Option<PathBuf> {
    let found = super::super::verify::executable();
    if found.is_none() {
        bt_test_data::missing("Godot 4", "set GODOT to its executable");
    }
    found
}

/// Runs a GDScript in the project (headless); returns the JSON it prints between markers.
pub(super) fn run_gdscript(godot: &Path, project: &Path, script: &str, args: &[&str]) -> Value {
    std::fs::write(project.join("check.gd"), script).unwrap();
    let output = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(project)
        .args(["--script", "res://check.gd", "--"])
        .args(args)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| {
            panic!(
                "no result from Godot:\n{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    serde_json::from_str(json).unwrap()
}
