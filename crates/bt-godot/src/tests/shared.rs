//! The shared TileSet (docs/design/12-godot-export.md): one `.tres` per grid that every map of the grid registers
//! its tilesets in, with source ids that are only ever added, whichever map is exported.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::super::contract::Found;
use super::super::report::{Element, Severity, State};
use super::super::{Error, Options, export, export_all, read};
use super::support::*;

/// An atlas with an animated tile, one Godot can't animate (a warning), a tile with an int
/// `gold`, and one with collision.
const A_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="a" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="0">
  <animation>
   <frame tileid="0" duration="100"/>
   <frame tileid="1" duration="100"/>
  </animation>
 </tile>
 <tile id="3">
  <animation>
   <frame tileid="3" duration="100"/>
   <frame tileid="2" duration="100"/>
  </animation>
 </tile>
 <tile id="5">
  <properties>
   <property name="gold" type="int" value="3"/>
  </properties>
 </tile>
 <tile id="15">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0" width="16" height="16"/>
  </objectgroup>
 </tile>
</tileset>
"#;

/// An atlas whose `gold` is a float: with a's, a float layer.
const B_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="b" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="2">
  <properties>
   <property name="gold" type="float" value="1.5"/>
  </properties>
 </tile>
</tileset>
"#;

/// An image collection of two tiles bigger than the grid, one with a `label` of its own.
const C_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="c" tilewidth="64" tileheight="64" tilecount="2" columns="0">
 <grid orientation="orthogonal" width="1" height="1"/>
 <tile id="0">
  <image source="tiles.png" width="64" height="64"/>
 </tile>
 <tile id="1">
  <properties>
   <property name="label" value="x"/>
  </properties>
  <image source="tiles.png" width="64" height="64"/>
 </tile>
</tileset>
"#;

/// Tall tiles, which flipped cells need alternatives of.
const T_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="t" tilewidth="16" tileheight="32" tilecount="8" columns="4">
 <image source="tiles.png" width="64" height="64"/>
</tileset>
"#;

/// The fourth map's own tileset, animated.
const D_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="d" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="4">
  <animation>
   <frame tileid="4" duration="100"/>
   <frame tileid="5" duration="100"/>
   <frame tileid="6" duration="100"/>
  </animation>
 </tile>
</tileset>
"#;

/// A 4×4 map of 16×16 cells with `tilesets` (TMX lines) and one row of `cells`.
fn map(tilesets: &str, cells: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
{tilesets}
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
{cells},
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
</map>
"#
    )
}

const H: u32 = 0x8000_0000;
const V: u32 = 0x4000_0000;

/// A Godot project with a setup (shared TileSets by default) and four maps: m1 (a, b and t, one
/// tall tile flipped up and down), m2 (b, the collection c and t, flipped sideways), m3 (a, c and
/// a tileset of its own) and m4 (d and a).
fn maps(name: &str) -> PathBuf {
    let dir = project(name);
    std::fs::write(
        dir.join("better-tiled.json"),
        r#"{"betterTiled": 1, "godot": {"project": "."}}"#,
    )
    .unwrap();
    for (file, text) in [
        ("a.tsx", A_TSX),
        ("b.tsx", B_TSX),
        ("c.tsx", C_TSX),
        ("t.tsx", T_TSX),
        ("d.tsx", D_TSX),
    ] {
        std::fs::write(dir.join("maps").join(file), text).unwrap();
    }
    let ts = |gid: u32, file: &str| format!(r#" <tileset firstgid="{gid}" source="{file}"/>"#);
    let local = r#" <tileset firstgid="19" name="local" tilewidth="16" tileheight="16" tilecount="16" columns="4">
  <image source="tiles.png" width="64" height="64"/>
 </tileset>"#;
    let m1 = map(
        &[ts(1, "a.tsx"), ts(17, "b.tsx"), ts(33, "t.tsx")].join("\n"),
        &format!("1,6,19,{}", 33 | V),
    );
    let m2 = map(
        &[ts(1, "b.tsx"), ts(17, "c.tsx"), ts(19, "t.tsx")].join("\n"),
        &format!("3,17,18,{}", 19 | H),
    );
    let m3 = map(
        &[ts(1, "a.tsx"), ts(17, "c.tsx"), local.to_owned()].join("\n"),
        "2,18,19,20",
    );
    let m4 = map(&[ts(1, "d.tsx"), ts(17, "a.tsx")].join("\n"), "5,17,32,0");
    for (file, text) in [
        ("m1.tmx", m1),
        ("m2.tmx", m2),
        ("m3.tmx", m3),
        ("m4.tmx", m4),
    ] {
        std::fs::write(dir.join("maps").join(file), text).unwrap();
    }
    dir
}

fn scene(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("levels/m{n}.tscn"))
}

fn tres(dir: &Path) -> PathBuf {
    dir.join("tilesets/ortho-16x16.tres")
}

fn run(dir: &Path, n: usize, options: &Options) -> Result<super::super::Report, Error> {
    export(
        &dir.join(format!("maps/m{n}.tmx")),
        &mut loader(dir),
        &scene(dir, n),
        options,
    )
}

fn ok(dir: &Path, n: usize) -> super::super::Report {
    run(dir, n, &Options::default()).unwrap_or_else(|e| panic!("m{n}: {e}"))
}

/// The shared TileSet's sources: resource_name → id.
fn ids(dir: &Path) -> BTreeMap<String, i64> {
    let text = std::fs::read_to_string(tres(dir)).unwrap();
    let resource = read::parse(&text).unwrap();
    let names: BTreeMap<String, String> = resource
        .sub_resources()
        .filter(|(s, _)| s.class == "TileSetAtlasSource")
        .map(|(s, section)| {
            (
                s.id,
                read::string(section.get("resource_name").unwrap()).unwrap(),
            )
        })
        .collect();
    let main = resource.resource().unwrap();
    main.props
        .iter()
        .filter_map(|(k, v)| {
            Some((
                names[&read::sub_resource(v)?].clone(),
                k.strip_prefix("sources/")?.parse().ok()?,
            ))
        })
        .collect()
}

/// The section of the source named `name` in the shared TileSet, as written.
fn section(dir: &Path, name: &str) -> String {
    let text = std::fs::read_to_string(tres(dir)).unwrap();
    let resource = read::parse(&text).unwrap();
    let (_, found) = resource
        .sub_resources()
        .find(|(_, s)| s.get("resource_name").and_then(read::string).as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {name}"));
    text[found.span.clone()].to_owned()
}

/// A scene's cells, by layer `ground`.
fn ground(dir: &Path, n: usize) -> Vec<[i32; 6]> {
    let tscn = std::fs::read_to_string(scene(dir, n)).unwrap();
    cells(value(
        node(&sections(&tscn), "ground", "."),
        "tile_map_data",
    ))
}

/// The names of the sources a scene's cells point at, in cell order.
fn named(dir: &Path, n: usize) -> Vec<String> {
    let by_id: BTreeMap<i64, String> = ids(dir).into_iter().map(|(name, id)| (id, name)).collect();
    ground(dir, n)
        .iter()
        .map(|c| by_id[&(c[2] as i64)].clone())
        .collect()
}

#[test]
fn maps_of_one_grid_share_one_tileset_whose_ids_stay() {
    let dir = maps("shared-ids");
    ok(&dir, 1);
    let expected = |pairs: &[(&str, i64)]| {
        pairs
            .iter()
            .map(|(n, id)| ((*n).to_owned(), *id))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        ids(&dir),
        expected(&[
            ("maps/a.tsx", 0),
            ("maps/a.tsx#anim1", 1),
            ("maps/b.tsx", 2),
            ("maps/t.tsx", 3)
        ])
    );
    ok(&dir, 2);
    ok(&dir, 3);
    let three = expected(&[
        ("maps/a.tsx", 0),
        ("maps/a.tsx#anim1", 1),
        ("maps/b.tsx", 2),
        ("maps/t.tsx", 3),
        ("maps/c.tsx#0", 4),
        ("maps/c.tsx#1", 5),
        ("maps/m3.tmx::local", 6),
    ]);
    assert_eq!(
        ids(&dir),
        three,
        "each map adds its new sources after the highest id"
    );

    // The scenes use the shared file by its uid, and keep no TileSet of their own.
    let text = std::fs::read_to_string(tres(&dir)).unwrap();
    assert!(text.starts_with("; Generated by better-tiled from the tilesets of ortho-16x16 maps. Set tiles up in better-tiled:\n"), "{text}");
    assert!(
        text.trim_end()
            .ends_with("metadata/bt_source = \"the tilesets of ortho-16x16 maps\""),
        "{text}"
    );
    let uid = read::parse_header(&text).unwrap().uid().unwrap();
    for n in 1..=3 {
        let tscn = std::fs::read_to_string(scene(&dir, n)).unwrap();
        assert!(!tscn.contains("type=\"TileSet\" id="), "m{n}: {tscn}");
        let line = format!(
            "[ext_resource type=\"TileSet\" uid=\"{uid}\" path=\"res://tilesets/ortho-16x16.tres\" id=\""
        );
        let id = tscn
            .split(&line)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_else(|| panic!("m{n}: {tscn}"));
        assert_eq!(
            value(node(&sections(&tscn), "ground", "."), "tile_set"),
            format!("ExtResource(\"{id}\")")
        );
    }
    assert_eq!(
        named(&dir, 1),
        ["maps/a.tsx#anim1", "maps/a.tsx", "maps/b.tsx", "maps/t.tsx"]
    );
    assert_eq!(
        named(&dir, 2),
        ["maps/b.tsx", "maps/c.tsx#0", "maps/c.tsx#1", "maps/t.tsx"]
    );
    assert_eq!(
        named(&dir, 3),
        [
            "maps/a.tsx",
            "maps/c.tsx#1",
            "maps/m3.tmx::local",
            "maps/m3.tmx::local"
        ]
    );
    // Flipped tall tiles: the alternative is the flip's own number (v = 2, h = 1), whichever map.
    assert_eq!(
        (ground(&dir, 1)[3][5], ground(&dir, 2)[3][5]),
        (2 | 8192, 4096),
        "a flip sideways doesn't move a tile as wide as its cell"
    );
    assert!(
        section(&dir, "maps/t.tsx")
            .contains("\n0:0/2 = 2\n0:0/2/texture_origin = Vector2i(0, -8)\n")
    );

    // A fourth map's new tileset goes after them all; nobody's ids move.
    ok(&dir, 4);
    let mut four = three.clone();
    four.extend(expected(&[("maps/d.tsx", 7), ("maps/d.tsx#anim1", 8)]));
    assert_eq!(ids(&dir), four);
    // Nothing changed for m1: nothing is written, the shared file included.
    let report = ok(&dir, 1);
    assert!(report.up_to_date(), "{:?}", report.files);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn exporting_one_map_keeps_what_the_others_use() {
    let dir = maps("shared-keeps");
    ok(&dir, 1);
    let (a, anim) = (
        section(&dir, "maps/a.tsx"),
        section(&dir, "maps/a.tsx#anim1"),
    );
    assert_eq!(
        a.matches("/0 = 0\n").count(),
        16,
        "every tile of a, transparent ones too: {a}"
    );
    assert!(anim.contains("0:0/animation_frame_1/duration"), "{anim}");
    ok(&dir, 2);
    assert_eq!(
        (
            section(&dir, "maps/a.tsx"),
            section(&dir, "maps/a.tsx#anim1")
        ),
        (a, anim),
        "m2 doesn't use a, and a stays as it was"
    );

    // The custom data layers are those of every tileset registered: a's int gold with b's float
    // gold make a float layer, and c's label is there whichever map is exported.
    let text = std::fs::read_to_string(tres(&dir)).unwrap();
    let main = &text[text.find("\n[resource]\n").unwrap()..];
    for line in [
        "custom_data_layer_0/name = \"gold\"",
        "custom_data_layer_0/type = 3",
        "custom_data_layer_1/name = \"label\"",
        "custom_data_layer_1/type = 4",
        "physics_layer_0/collision_layer = 1",
    ] {
        assert!(main.contains(line), "{line}: {main}");
    }
    assert!(
        section(&dir, "maps/a.tsx").contains("1:1/0/custom_data_0 = 3.0\n"),
        "a's ints are floats in the shared layer"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_tileset_is_the_same_whichever_map_is_exported() {
    let dir = maps("shared-orders");
    for n in [1, 2, 3] {
        ok(&dir, n);
    }
    let bytes = std::fs::read(tres(&dir)).unwrap();
    for order in [
        [1, 2, 3],
        [1, 3, 2],
        [2, 1, 3],
        [2, 3, 1],
        [3, 1, 2],
        [3, 2, 1],
    ] {
        for n in order {
            let report = ok(&dir, n);
            assert!(
                report
                    .files
                    .iter()
                    .any(|f| f.path == tres(&dir) && f.state == State::Unchanged),
                "m{n} in {order:?}: {:?}",
                report.files
            );
            assert_eq!(
                std::fs::read(tres(&dir)).unwrap(),
                bytes,
                "m{n} in {order:?}"
            );
        }
    }
    // The notes about another map's tilesets are that map's to give.
    let layout = |report: &super::super::Report| {
        report
            .notes
            .iter()
            .filter(|n| n.code.as_str() == "godot/animation-layout")
            .map(|n| n.elements.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        layout(&ok(&dir, 1)),
        [vec![Element::Tile { tileset: 0, id: 3 }]],
        "a is m1's"
    );
    assert_eq!(
        layout(&ok(&dir, 2)),
        Vec::<Vec<Element>>::new(),
        "m2 has a from m1"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copies_of_its_images_are_placed_by_their_tileset_and_used_by_it() {
    let dir = maps("shared-copies");
    let outside = std::env::temp_dir().join(format!("bt-godot-shared-art-{}", std::process::id()));
    std::fs::create_dir_all(outside.join("pack")).unwrap();
    std::fs::copy(dir.join("maps/tiles.png"), outside.join("pack/far.png")).unwrap();
    std::fs::write(
        outside.join("pack/far.tsx"),
        B_TSX
            .replace("name=\"b\"", "name=\"far\"")
            .replace("tiles.png", "far.png"),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("maps/sub")).unwrap();
    let far = format!(
        r#" <tileset firstgid="1" source="{}"/>"#,
        outside.join("pack/far.tsx").display()
    );
    std::fs::write(dir.join("maps/near.tmx"), map(&far, "1,0,0,0")).unwrap();
    std::fs::write(dir.join("maps/sub/deep.tmx"), map(&far, "2,0,0,0")).unwrap();
    export(
        &dir.join("maps/near.tmx"),
        &mut loader(&dir),
        &dir.join("levels/near.tscn"),
        &Options::default(),
    )
    .unwrap();
    let bytes = std::fs::read(tres(&dir)).unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        text.contains("path=\"res://images/far.png\""),
        "copied by its path from its tileset, into the setup's folder: {text}"
    );
    export(
        &dir.join("maps/sub/deep.tmx"),
        &mut loader(&dir),
        &dir.join("levels/sub/deep.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tres(&dir)).unwrap(),
        bytes,
        "a map in another folder uses the same copy"
    );
    let sources: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("images/.sources.json")).unwrap())
            .unwrap();
    assert_eq!(
        sources["far.png"]["usedBy"],
        serde_json::json!(["res://tilesets/ortho-16x16.tres"]),
        "{sources}"
    );
    std::fs::remove_dir_all(&outside).ok();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_grid_mismatch_fails_and_a_missing_tileset_in_use_is_refused() {
    let dir = maps("shared-refusals");
    for n in [1, 2, 3] {
        ok(&dir, n);
    }
    // An isometric map pointed at the orthogonal maps' file.
    let iso = map(r#" <tileset firstgid="1" source="a.tsx"/>"#, "1,0,0,0")
        .replace("orientation=\"orthogonal\"", "orientation=\"isometric\"")
        .replace("tileheight=\"16\" infinite", "tileheight=\"8\" infinite");
    let with = |text: &str, value: &str| {
        text.replace("<tileset firstgid=\"1\"", &format!("<properties>\n  <property name=\"tilesetResPath\" value=\"{value}\"/>\n </properties>\n <tileset firstgid=\"1\""))
    };
    std::fs::write(
        dir.join("maps/iso.tmx"),
        with(&iso, "res://tilesets/ortho-16x16.tres"),
    )
    .unwrap();
    let failed = |file: &str| match export(
        &dir.join("maps").join(file),
        &mut loader(&dir),
        &dir.join("levels/x.tscn"),
        &Options::default(),
    ) {
        Err(Error::Failed(message)) => message,
        other => panic!("{file}: {other:?}"),
    };
    let message = failed("iso.tmx");
    assert!(message.ends_with("res://tilesets/ortho-16x16.tres is the TileSet of ortho-16x16 maps, and this map's grid is iso-16x8: a tile sits on one grid only, so give the map a tilesetResPath of its own"), "{message}");
    for bad in ["tilesets/x.tres", "res://x.png", "res://../x.tres"] {
        std::fs::write(
            dir.join("maps/bad.tmx"),
            with(
                &map(r#" <tileset firstgid="1" source="a.tsx"/>"#, "1,0,0,0"),
                bad,
            ),
        )
        .unwrap();
        assert!(
            failed("bad.tmx")
                .contains("tilesetResPath is a res://….tres path inside the Godot project"),
            "{bad}"
        );
    }
    assert!(!dir.join("levels/x.tscn").exists());

    // The shared file gone while m2 and m3 use it: a new one would renumber under their cells.
    std::fs::remove_file(tres(&dir)).unwrap();
    let before = std::fs::read(scene(&dir, 1)).unwrap();
    for options in [
        Options::default(),
        Options {
            replace: true,
            ..Options::default()
        },
    ] {
        let Err(Error::Refused(refusals)) = run(&dir, 1, &options) else {
            panic!("not refused")
        };
        assert_eq!(refusals.len(), 1);
        assert_eq!(
            (refusals[0].path.clone(), refusals[0].found.clone()),
            (
                tres(&dir),
                Found::Missing(vec![scene(&dir, 2), scene(&dir, 3)])
            )
        );
        assert_eq!(
            refusals[0].way_out(),
            "restore it, or export every map that uses it again with bt godot export --all"
        );
    }
    assert!(
        !tres(&dir).exists() && std::fs::read(scene(&dir, 1)).unwrap() == before,
        "nothing written"
    );

    // m3's scene saved again by Godot, which drops the header: still the export's, by its
    // marker, so exporting every map again asks only to replace it, not to restore the TileSet.
    let m3 = std::fs::read_to_string(scene(&dir, 3)).unwrap();
    let resaved: String = m3
        .lines()
        .filter(|l| !l.starts_with(';'))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(scene(&dir, 3), &resaved).unwrap();
    let results = export_all(&dir, &mut |_| Ok(loader(&dir)), &Options::default()).unwrap();
    assert!(!results.written);
    let refused = |e: &Error| match e {
        Error::Refused(refusals) => refusals.iter().map(|r| r.found.clone()).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    let found: Vec<Found> = results
        .scenes
        .iter()
        .filter_map(|e| e.outcome.as_ref().err())
        .flat_map(refused)
        .collect();
    assert_eq!(found, [Found::ResavedByGodot], "{results:?}");
    std::fs::write(scene(&dir, 3), &m3).unwrap();

    // Exporting every map again makes it afresh, numbered in the order of the scenes.
    let results = export_all(&dir, &mut |_| Ok(loader(&dir)), &Options::default()).unwrap();
    assert!(results.written);
    assert_eq!(
        results
            .scenes
            .iter()
            .map(|e| (e.scene.clone(), e.outcome.is_ok()))
            .collect::<Vec<_>>(),
        [
            (scene(&dir, 1), true),
            (scene(&dir, 2), true),
            (scene(&dir, 3), true)
        ]
    );
    assert_eq!(
        named(&dir, 2),
        ["maps/b.tsx", "maps/c.tsx#0", "maps/c.tsx#1", "maps/t.tsx"]
    );
    assert_eq!(
        named(&dir, 3),
        [
            "maps/a.tsx",
            "maps/c.tsx#1",
            "maps/m3.tmx::local",
            "maps/m3.tmx::local"
        ]
    );
    assert!(run(&dir, 1, &Options::default()).unwrap().up_to_date());
    std::fs::remove_dir_all(&dir).ok();
}

/// c with walkable and light-blocking shapes: a navigation polygon both tiles have, and an
/// occluder of tile 1's own.
const C_AREAS_TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="c" tilewidth="64" tileheight="64" tilecount="2" columns="0">
 <grid orientation="orthogonal" width="1" height="1"/>
 <tile id="0">
  <image source="tiles.png" width="64" height="64"/>
  <objectgroup draworder="index">
   <object id="1" x="0" y="32" width="64" height="32">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotNavigationLayer" type="int" value="0"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
 <tile id="1">
  <properties>
   <property name="label" value="x"/>
  </properties>
  <image source="tiles.png" width="64" height="64"/>
  <objectgroup draworder="index">
   <object id="1" x="0" y="32" width="64" height="32">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotNavigationLayer" type="int" value="0"/>
    </properties>
   </object>
   <object id="2" x="8" y="8" width="16" height="16">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotOcclusionLayer" type="int" value="0"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
</tileset>
"#;

/// A scene of the user's own painted with the shared TileSet names its ids too: with the file
/// gone, an export that would make it afresh is refused for it as for a generated scene.
#[test]
fn a_missing_tileset_a_scene_of_the_users_paints_with_is_refused() {
    let dir = maps("shared-users-scene");
    ok(&dir, 1);
    let uid = read::parse_header(&std::fs::read_to_string(tres(&dir)).unwrap())
        .unwrap()
        .uid()
        .unwrap();
    std::fs::create_dir_all(dir.join("mine")).unwrap();
    let room = format!(
        "[gd_scene format=3]\n\n[ext_resource type=\"TileSet\" uid=\"{uid}\" path=\"res://tilesets/ortho-16x16.tres\" id=\"1\"]\n\n[node name=\"room\" type=\"TileMapLayer\"]\ntile_map_data = PackedByteArray(0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0)\ntile_set = ExtResource(\"1\")\n"
    );
    std::fs::write(dir.join("mine/room.tscn"), room).unwrap();
    std::fs::remove_file(tres(&dir)).unwrap();
    let Err(Error::Refused(refusals)) = run(&dir, 1, &Options::default()) else {
        panic!("not refused")
    };
    assert_eq!(
        (refusals[0].path.clone(), refusals[0].found.clone()),
        (tres(&dir), Found::Missing(vec![dir.join("mine/room.tscn")]))
    );
    assert_eq!(
        refusals[0].way_out(),
        "restore it: room.tscn paints with it, and a TileSet made afresh would number its tiles anew"
    );
    assert!(!tres(&dir).exists());

    // Exporting every map again can't renumber the user's cells either: it refuses, writing nothing.
    let before = std::fs::read(scene(&dir, 1)).unwrap();
    let results = export_all(&dir, &mut |_| Ok(loader(&dir)), &Options::default()).unwrap();
    assert!(!results.written);
    let Some(Err(Error::Refused(refusals))) = results
        .scenes
        .iter()
        .find(|e| e.scene == scene(&dir, 1))
        .map(|e| &e.outcome)
    else {
        panic!("{results:?}")
    };
    assert_eq!(
        refusals[0].found,
        Found::Missing(vec![dir.join("mine/room.tscn")])
    );
    assert!(
        !tres(&dir).exists() && std::fs::read(scene(&dir, 1)).unwrap() == before,
        "nothing written"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sources_it_can_no_longer_make_are_kept_as_they_were() {
    let dir = maps("shared-missing");
    // Their sub-resources too: the kept sources' navigation polygons and occluders stay as they were.
    std::fs::write(dir.join("maps/c.tsx"), C_AREAS_TSX).unwrap();
    for n in [1, 2, 3] {
        ok(&dir, n);
    }
    let bytes = std::fs::read(tres(&dir)).unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        text.contains("[sub_resource type=\"NavigationPolygon\" id=\"NavigationPolygon_")
            && text.contains("[sub_resource type=\"OccluderPolygon2D\" id=\"OccluderPolygon2D_"),
        "{text}"
    );

    // c's file gone: m1 (which doesn't use c) keeps c's sources, and c's label layer, as they were.
    std::fs::rename(dir.join("maps/c.tsx"), dir.join("maps/c.away")).unwrap();
    let report = ok(&dir, 1);
    let note = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/shared-tileset-missing")
        .unwrap_or_else(|| panic!("{:?}", report.notes));
    assert_eq!(note.severity, Severity::Warning);
    assert!(note.message.starts_with("res://tilesets/ortho-16x16.tres keeps the 2 sources of maps/c.tsx as made before, since that tileset can't be read ("), "{}", note.message);
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(tres(&dir)).unwrap()),
        String::from_utf8_lossy(&bytes),
        "the same file"
    );
    std::fs::rename(dir.join("maps/c.away"), dir.join("maps/c.tsx")).unwrap();
    assert!(
        ok(&dir, 1)
            .notes
            .iter()
            .all(|n| n.code.as_str() != "godot/shared-tileset-missing")
    );

    // c without its tile 1: the source stays for the scenes exported before, as it was.
    std::fs::write(
        dir.join("maps/c.tsx"),
        C_AREAS_TSX
            .split(" <tile id=\"1\">")
            .next()
            .unwrap()
            .to_owned()
            + "</tileset>\n",
    )
    .unwrap();
    let report = ok(&dir, 1);
    let note = report
        .notes
        .iter()
        .find(|n| n.code.as_str() == "godot/shared-source-kept")
        .unwrap_or_else(|| panic!("{:?}", report.notes));
    assert_eq!(
        (note.severity, note.message.as_str()),
        (
            Severity::Info,
            "res://tilesets/ortho-16x16.tres keeps maps/c.tsx#1 as made before, for the scenes exported then, though no tileset makes them now: to drop them, delete the file and export every map again"
        )
    );
    assert_eq!(
        String::from_utf8_lossy(&std::fs::read(tres(&dir)).unwrap()),
        String::from_utf8_lossy(&bytes)
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// The `res://` paths a resource's `ext_resource`s name that aren't files of the project.
fn not_there(dir: &Path, file: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(file).unwrap();
    read::parse(&text)
        .unwrap()
        .ext_resources()
        .map(|e| e.path)
        .filter(|p| !dir.join(p.trim_start_matches("res://")).is_file())
        .collect()
}

/// The copies of a shared TileSet's kept sources stay while it names them, so it still loads.
#[test]
fn kept_sources_keep_the_image_copies_they_name() {
    let dir = maps("shared-kept-copies");
    // c's images from outside the project, so they are copied: one for each tile.
    let outside =
        std::env::temp_dir().join(format!("bt-godot-shared-kept-art-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    let art = |n: usize| outside.join(format!("c{n}.png"));
    for n in [0, 1] {
        std::fs::copy(dir.join("maps/tiles.png"), art(n)).unwrap();
    }
    let c = C_TSX
        .replacen("tiles.png", &art(0).display().to_string(), 1)
        .replacen("tiles.png", &art(1).display().to_string(), 1);
    std::fs::write(dir.join("maps/c.tsx"), &c).unwrap();
    for n in [1, 2, 3] {
        ok(&dir, n);
    }
    let text = std::fs::read_to_string(tres(&dir)).unwrap();
    assert!(
        text.contains("c0.png\" id=")
            && text.contains("c1.png\" id=")
            && not_there(&dir, &tres(&dir)).is_empty(),
        "{text}"
    );
    let pruned = |report: &super::super::Report| {
        report
            .files
            .iter()
            .filter(|f| f.state == State::Pruned)
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
    };

    // c's file gone: its sources are kept, and so are their copies.
    std::fs::rename(dir.join("maps/c.tsx"), dir.join("maps/c.away")).unwrap();
    let report = ok(&dir, 1);
    assert_eq!(
        (pruned(&report), not_there(&dir, &tres(&dir))),
        (Vec::new(), Vec::new())
    );
    assert_eq!(
        std::fs::read_to_string(tres(&dir)).unwrap(),
        text,
        "the same file"
    );
    std::fs::rename(dir.join("maps/c.away"), dir.join("maps/c.tsx")).unwrap();
    // c without its tile 1, whose image only it uses.
    std::fs::write(
        dir.join("maps/c.tsx"),
        c.split(" <tile id=\"1\">").next().unwrap().to_owned() + "</tileset>\n",
    )
    .unwrap();
    let report = ok(&dir, 1);
    assert_eq!(
        (pruned(&report), not_there(&dir, &tres(&dir))),
        (Vec::new(), Vec::new())
    );
    let sources: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("images/.sources.json")).unwrap())
            .unwrap();
    let c1 = sources
        .as_object()
        .unwrap()
        .iter()
        .find(|(key, _)| key.ends_with("c1.png"))
        .map(|(_, c)| c["usedBy"].clone());
    assert_eq!(
        c1,
        Some(serde_json::json!(["res://tilesets/ortho-16x16.tres"])),
        "{sources}"
    );

    // Godot loads every scene, from the TileSet with its kept sources.
    if let Some(godot) = godot() {
        let scenes: Vec<String> = (1..=3).map(|n| format!("res://levels/m{n}.tscn")).collect();
        let (result, stderr) = godot_run(&godot, &dir, LOAD, &scenes);
        assert_eq!(stderr, "");
        for scene in &scenes {
            assert_eq!(
                result[scene]["missing"].as_i64(),
                Some(0),
                "{scene}: {result}"
            );
        }
    }
    std::fs::remove_dir_all(&outside).ok();
    std::fs::remove_dir_all(&dir).ok();
}

/// An atlas whose tiles `ids` animate over their cell and the next: tiles 1 and 2 share a cell,
/// so they are in two animation sources.
fn e_tsx(ids: &[u32]) -> String {
    let animated = |id: u32| {
        format!(
            " <tile id=\"{id}\">\n  <animation>\n   <frame tileid=\"{id}\" duration=\"100\"/>\n   <frame tileid=\"{}\" duration=\"100\"/>\n  </animation>\n </tile>\n",
            id + 1
        )
    };
    let tiles: String = ids.iter().map(|&id| animated(id)).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" tiledversion=\"1.12.2\" name=\"e\" tilewidth=\"16\" tileheight=\"16\" tilecount=\"16\" columns=\"4\">\n <image source=\"tiles.png\" width=\"64\" height=\"64\"/>\n{tiles}</tileset>\n"
    )
}

/// Exporting one map after an animation was added keeps every animated tile in the source and
/// cell it had, so the scenes of the others, not exported since, still find their tiles.
#[test]
fn animated_tiles_stay_where_they_were_when_an_animation_is_added() {
    let dir = maps("shared-animations");
    std::fs::write(dir.join("maps/e.tsx"), e_tsx(&[1, 2])).unwrap();
    let e = r#" <tileset firstgid="1" source="e.tsx"/>"#;
    std::fs::write(dir.join("maps/m5.tmx"), map(e, "2,3,0,0")).unwrap();
    std::fs::write(dir.join("maps/m6.tmx"), map(e, "6,1,0,0")).unwrap();
    ok(&dir, 5);
    ok(&dir, 6);
    let before = ids(&dir);
    assert_eq!(named(&dir, 5), ["maps/e.tsx#anim1", "maps/e.tsx#anim2"]);
    let cells = ground(&dir, 5);

    // Tile 0 animates now, over tile 1's first cell; only m6 is exported again.
    std::fs::write(dir.join("maps/e.tsx"), e_tsx(&[0, 1, 2])).unwrap();
    ok(&dir, 6);
    assert_eq!(ids(&dir), before, "no new source: tile 0 fits in #anim2");
    let has_tile =
        |name: &str, x: i32, y: i32| section(&dir, name).contains(&format!("\n{x}:{y}/0 = 0\n"));
    assert!(
        has_tile("maps/e.tsx#anim1", 1, 0)
            && has_tile("maps/e.tsx#anim2", 2, 0)
            && has_tile("maps/e.tsx#anim2", 0, 0)
    );
    // m5's scene, exported before, finds a tile at each of its cells.
    let by_id: BTreeMap<i64, String> = ids(&dir).into_iter().map(|(name, id)| (id, name)).collect();
    for c in &cells {
        assert!(has_tile(&by_id[&(c[2] as i64)], c[3], c[4]), "{c:?}");
    }
    if let Some(godot) = godot() {
        let scenes = [
            "res://levels/m5.tscn".to_owned(),
            "res://levels/m6.tscn".to_owned(),
        ];
        let (result, stderr) = godot_run(&godot, &dir, LOAD, &scenes);
        assert_eq!(stderr, "");
        for scene in &scenes {
            assert_eq!(
                result[scene]["missing"].as_i64(),
                Some(0),
                "{scene}: {result}"
            );
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_setup_or_the_map_says_where_it_goes() {
    // tilesetResPath, without a setup: there, and consumed.
    let dir = maps("shared-respath");
    std::fs::remove_file(dir.join("better-tiled.json")).unwrap();
    let text = std::fs::read_to_string(dir.join("maps/m1.tmx")).unwrap();
    let text = text.replacen(" <tileset", " <properties>\n  <property name=\"tilesetResPath\" value=\"res://world/tiles.tres\"/>\n </properties>\n <tileset", 1);
    std::fs::write(dir.join("maps/m1.tmx"), text).unwrap();
    let report = ok(&dir, 1);
    assert!(report.consumed.contains("tilesetResPath"));
    assert!(dir.join("world/tiles.tres").is_file() && !tres(&dir).exists());
    let tscn = std::fs::read_to_string(scene(&dir, 1)).unwrap();
    assert!(
        tscn.contains("path=\"res://world/tiles.tres\"") && !tscn.contains("tilesetResPath"),
        "{tscn}"
    );
    // Without either, the TileSet is in the scene, as before.
    ok(&dir, 2);
    let tscn = std::fs::read_to_string(scene(&dir, 2)).unwrap();
    assert!(
        tscn.contains("[sub_resource type=\"TileSet\" id=\"TileSet_0\"]")
            && !tscn.contains(".tres"),
        "{tscn}"
    );
    std::fs::remove_dir_all(&dir).ok();

    // A setup that keeps TileSets in the scenes.
    let dir = maps("shared-embedded");
    std::fs::write(
        dir.join("better-tiled.json"),
        r#"{"betterTiled": 1, "godot": {"project": ".", "tileSets": "embedded"}}"#,
    )
    .unwrap();
    ok(&dir, 1);
    assert!(
        std::fs::read_to_string(scene(&dir, 1))
            .unwrap()
            .contains("[sub_resource type=\"TileSet\" id=\"TileSet_0\"]")
            && !dir.join("tilesets").exists()
    );
    // Another folder.
    std::fs::write(
        dir.join("better-tiled.json"),
        r#"{"betterTiled": 1, "godot": {"project": ".", "tileSetFolder": "res://art/sets/"}}"#,
    )
    .unwrap();
    run(
        &dir,
        1,
        &Options {
            replace: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(dir.join("art/sets/ortho-16x16.tres").is_file());
    std::fs::remove_dir_all(&dir).ok();
}

/// Runs `script` headless in the project (after importing it) with `args`: the JSON it prints
/// and what Godot wrote on stderr. Gives up after two minutes rather than hang on a script error.
fn godot_run(godot: &Path, dir: &Path, script: &str, args: &[String]) -> (Value, String) {
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
    std::fs::write(dir.join("shared.gd"), script).unwrap();
    let (out, err) = (dir.join("shared.out"), dir.join("shared.err"));
    let mut child = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .args(["--script", "res://shared.gd", "--"])
        .args(args)
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

/// Loads each scene: its cells, those without tile data, and the TileSets its layers use.
const LOAD: &str = r#"extends SceneTree

func _init() -> void:
	var result := {}
	for path: String in OS.get_cmdline_user_args():
		var room := (load(path) as PackedScene).instantiate()
		var total := 0
		var missing := 0
		var tilesets := []
		for node: Node in room.find_children("*", "TileMapLayer", true, false):
			var layer := node as TileMapLayer
			tilesets.append(layer.tile_set.resource_path)
			for c: Vector2i in layer.get_used_cells():
				total += 1
				if layer.get_cell_tile_data(c) == null:
					missing += 1
		result[path] = {"total": total, "missing": missing, "tilesets": tilesets}
		room.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// Saves a resource again where it is, as Godot's editor does.
const RESAVE: &str = r#"extends SceneTree

func _init() -> void:
	var path: String = OS.get_cmdline_user_args()[0]
	var error := ResourceSaver.save(load(path), path)
	print("BT-JSON-BEGIN", JSON.stringify({"error": error}), "BT-JSON-END")
	quit()
"#;

/// Godot loads every scene after each map's export, with tile data for every cell, from the one
/// shared file; and once Godot saved that file again, the next export refuses to replace it.
#[test]
fn godot_loads_every_scene_after_each_export() {
    let Some(godot) = godot() else { return };
    let dir = maps("shared-godot");
    for last in 1..=4 {
        ok(&dir, last);
        let scenes: Vec<String> = (1..=last)
            .map(|n| format!("res://levels/m{n}.tscn"))
            .collect();
        let (result, stderr) = godot_run(&godot, &dir, LOAD, &scenes);
        assert_eq!(stderr, "", "after m{last}");
        for scene in &scenes {
            let r = &result[scene];
            assert_eq!(
                (r["missing"].as_i64(), r["tilesets"].clone()),
                (
                    Some(0),
                    serde_json::json!(["res://tilesets/ortho-16x16.tres"])
                ),
                "{scene} after m{last}: {r}"
            );
            assert!(r["total"].as_i64().unwrap() >= 3, "{scene}: {r}");
        }
    }

    let (result, stderr) = godot_run(
        &godot,
        &dir,
        RESAVE,
        &["res://tilesets/ortho-16x16.tres".to_owned()],
    );
    assert_eq!((result["error"].as_i64(), stderr.as_str()), (Some(0), ""));
    let Err(Error::Refused(refusals)) = run(&dir, 1, &Options::default()) else {
        panic!("not refused")
    };
    assert_eq!(
        (refusals[0].path.clone(), refusals[0].found.clone()),
        (tres(&dir), Found::ResavedByGodot)
    );
    // Replacing it keeps the ids Godot kept.
    let before = ids(&dir);
    run(
        &dir,
        1,
        &Options {
            replace: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(ids(&dir), before);
    assert!(dir.join("tilesets/ortho-16x16.tres.bak").is_file());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn instanced_scenes_are_named_by_their_uid_too() {
    let dir = maps("shared-instance");
    std::fs::create_dir_all(dir.join("things")).unwrap();
    std::fs::write(
        dir.join("things/door.tscn"),
        "[gd_scene format=3 uid=\"uid://cyncsb\"]\n\n[node name=\"door\" type=\"Node2D\"]\n",
    )
    .unwrap();
    let objects = r#" <objectgroup id="2" name="things">
  <object id="1" name="door" x="16" y="16" width="16" height="16">
   <properties>
    <property name="resPath" value="res://things/door.tscn"/>
   </properties>
  </object>
 </objectgroup>
</map>"#;
    let text = std::fs::read_to_string(dir.join("maps/m1.tmx"))
        .unwrap()
        .replace("</map>", objects);
    std::fs::write(dir.join("maps/m1.tmx"), text).unwrap();
    ok(&dir, 1);
    let tscn = std::fs::read_to_string(scene(&dir, 1)).unwrap();
    assert!(tscn.contains("[ext_resource type=\"PackedScene\" uid=\"uid://cyncsb\" path=\"res://things/door.tscn\" id=\""), "{tscn}");
    std::fs::remove_dir_all(&dir).ok();
}
