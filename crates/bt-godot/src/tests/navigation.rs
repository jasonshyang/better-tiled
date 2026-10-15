//! Navigation and occlusion from tiles' collision shapes (docs/design/12-godot-export.md): `godotNavigationLayer`
//! makes a shape walkable, as a `NavigationPolygon` of convex parts, and `godotOcclusionLayer`
//! makes it an `OccluderPolygon2D`, on the TileSet's layers that the setup describes.

use std::path::{Path, PathBuf};

use glam::DVec2;
use serde_json::Value as Json;

use super::super::collision::convex_parts;
use super::super::report::Severity;
use super::super::{Options, Report, export, read};
use super::support::*;

/// Twice a ring's area, positive when it turns clockwise on screen.
fn twice_area(points: &[DVec2]) -> f64 {
    (0..points.len())
        .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
        .sum()
}

/// Checks `outline`'s parts: each convex and turning clockwise, together covering the outline
/// exactly, joined edge to edge into one piece, and using every point. Returns how many there are.
fn check_parts(outline: &[DVec2]) -> usize {
    let (ring, parts) = convex_parts(outline).unwrap_or_else(|| panic!("{outline:?} has parts"));
    let part = |p: &Vec<usize>| p.iter().map(|&i| ring[i]).collect::<Vec<_>>();
    for p in &parts {
        let points = part(p);
        let m = points.len();
        assert!(m >= 3, "{parts:?}");
        for i in 0..m {
            let (a, b, c) = (points[(i + m - 1) % m], points[i], points[(i + 1) % m]);
            assert!(
                (b - a).perp_dot(c - b) >= -1e-9,
                "part {points:?} turns back at {b}"
            );
        }
    }
    let total: f64 = parts.iter().map(|p| twice_area(&part(p))).sum();
    assert!(
        (total - twice_area(&ring)).abs() < 1e-9 && twice_area(&ring) > 0.0,
        "the parts cover the outline: {total} vs {}",
        twice_area(&ring)
    );
    let mut used: Vec<usize> = parts.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    assert_eq!(
        used,
        (0..ring.len()).collect::<Vec<_>>(),
        "every point is in a part"
    );
    // Joined: each part shares an edge with one reached before.
    let edges = |p: &Vec<usize>| {
        (0..p.len())
            .map(|i| (p[i], p[(i + 1) % p.len()]))
            .collect::<Vec<_>>()
    };
    let mut reached = vec![0];
    while reached.len() < parts.len() {
        let next = (0..parts.len()).find(|j| {
            !reached.contains(j)
                && reached.iter().any(|&r| {
                    edges(&parts[r])
                        .iter()
                        .any(|&(u, v)| edges(&parts[*j]).contains(&(v, u)))
                })
        });
        reached.push(next.unwrap_or_else(|| panic!("parts {parts:?} aren't joined edge to edge")));
    }
    parts.len()
}

fn ring(points: &[(f64, f64)]) -> Vec<DVec2> {
    points.iter().map(|&(x, y)| DVec2::new(x, y)).collect()
}

#[test]
fn concave_outlines_split_into_joined_convex_parts() {
    // An L: two parts, the fewest.
    let l = ring(&[
        (0.0, 0.0),
        (16.0, 0.0),
        (16.0, 6.0),
        (6.0, 6.0),
        (6.0, 16.0),
        (0.0, 16.0),
    ]);
    assert_eq!(check_parts(&l), 2);
    // Turning the other way: the same, turned round.
    let mut back = l.clone();
    back.reverse();
    assert_eq!(check_parts(&back), 2);
    let (turned, _) = convex_parts(&back).unwrap();
    assert!(twice_area(&turned) > 0.0);
    // A U needs three; a five-pointed star five or more; a comb of three teeth four or more.
    let u = ring(&[
        (0.0, 0.0),
        (3.0, 0.0),
        (3.0, 3.0),
        (2.0, 3.0),
        (2.0, 1.0),
        (1.0, 1.0),
        (1.0, 3.0),
        (0.0, 3.0),
    ]);
    assert_eq!(check_parts(&u), 3);
    let star: Vec<DVec2> = (0..10)
        .map(|i| {
            let a = i as f64 * std::f64::consts::PI / 5.0;
            let r = if i % 2 == 0 { 10.0 } else { 4.0 };
            DVec2::new(a.sin() * r, -a.cos() * r)
        })
        .collect();
    assert!((5..=8).contains(&check_parts(&star)));
    let comb = ring(&[
        (0.0, 0.0),
        (1.0, 0.0),
        (1.0, 2.0),
        (2.0, 2.0),
        (2.0, 0.0),
        (3.0, 0.0),
        (3.0, 2.0),
        (4.0, 2.0),
        (4.0, 0.0),
        (5.0, 0.0),
        (5.0, 3.0),
        (0.0, 3.0),
    ]);
    assert!((4..=6).contains(&check_parts(&comb)));

    // A convex outline is one part, as it was; repeated points and points along an edge go.
    let square = ring(&[(-8.0, -8.0), (8.0, -8.0), (8.0, 8.0), (-8.0, 8.0)]);
    assert_eq!(
        convex_parts(&square),
        Some((square.clone(), vec![vec![0, 1, 2, 3]]))
    );
    let noisy = ring(&[
        (-8.0, -8.0),
        (0.0, -8.0),
        (8.0, -8.0),
        (8.0, -8.0),
        (8.0, 8.0),
        (-8.0, 8.0),
        (-8.0, -8.0),
    ]);
    assert_eq!(convex_parts(&noisy), Some((square, vec![vec![0, 1, 2, 3]])));

    // Edges that cross or touch, and no area: nothing to walk on.
    assert_eq!(
        convex_parts(&ring(&[(0.0, 0.0), (16.0, 16.0), (16.0, 0.0), (0.0, 16.0)])),
        None,
        "a bow tie"
    );
    assert_eq!(
        convex_parts(&ring(&[
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 4.0),
            (2.0, 0.0),
            (0.0, 4.0)
        ])),
        None,
        "a point on another edge"
    );
    assert_eq!(
        convex_parts(&ring(&[(0.0, 0.0), (4.0, 4.0), (8.0, 8.0)])),
        None,
        "a line"
    );
    assert_eq!(
        convex_parts(&ring(&[(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)])),
        None,
        "a point"
    );
}

/// Tiles with shapes on navigation and occlusion layers: a walkable square (on physics too), an
/// L walkable on layer 1 only, occluders on two layers, a bow tie nothing can walk on, and a second
/// walkable square like the first.
const NAV_TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="nav" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="0">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0" width="16" height="16">
    <properties>
     <property name="godotNavigationLayer" type="int" value="0"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
 <tile id="1">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotNavigationLayer" type="int" value="1"/>
    </properties>
    <polygon points="0,0 16,0 16,6 6,6 6,16 0,16"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="2">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0" width="16" height="8">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotOcclusionLayer" type="int" value="0"/>
    </properties>
   </object>
   <object id="2" x="4" y="4" width="8" height="8">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotOcclusionLayer" type="int" value="1"/>
    </properties>
   </object>
   <object id="3" x="0" y="8">
    <properties>
     <property name="godotOcclusionLayer" type="int" value="0"/>
    </properties>
    <polygon points="0,0 16,0 0,8"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="3">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0">
    <properties>
     <property name="godotCollision" type="bool" value="false"/>
     <property name="godotNavigationLayer" type="int" value="0"/>
    </properties>
    <polygon points="0,0 16,16 16,0 0,16"/>
   </object>
  </objectgroup>
 </tile>
 <tile id="4">
  <objectgroup draworder="index">
   <object id="1" x="0" y="0" width="16" height="16">
    <properties>
     <property name="godotNavigationLayer" type="int" value="0"/>
    </properties>
   </object>
  </objectgroup>
 </tile>
</tileset>
"##;

/// The tiles in a row, a row of walkable squares, and a tile bigger than the grid, walkable and
/// casting a shadow from its bottom-left quarter, painted plain and flipped sideways (so it gets
/// an alternative tile, its shapes moved).
const NAV_TMX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="6" height="3" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="nav.tsx"/>
 <tileset firstgid="17" name="big" tilewidth="32" tileheight="32" tilecount="4" columns="2">
  <image source="tiles.png" width="64" height="64"/>
  <tile id="0">
   <objectgroup draworder="index">
    <object id="1" x="0" y="16" width="16" height="16">
     <properties>
      <property name="godotNavigationLayer" type="int" value="0"/>
      <property name="godotOcclusionLayer" type="int" value="0"/>
     </properties>
    </object>
   </objectgroup>
  </tile>
 </tileset>
 <layer id="1" name="cells" width="6" height="3">
  <data encoding="csv">
1,2,3,4,5,0,
1,1,1,0,0,0,
0,17,0,2147483665,0,0
</data>
 </layer>
</map>
"#;

/// Navigation layer 1 with its own bits, occlusion layer 0 with its own light mask; occlusion
/// layer 1 left to Godot.
const SETUP: &str = r#"{"betterTiled": 1, "godot": {"navigationLayers": [{"layers": 1}, {"layers": 6}], "occlusionLayers": [{"lightMask": 3}]}}"#;

fn nav_project(name: &str, setup: &str) -> PathBuf {
    let dir = project(name);
    std::fs::write(dir.join("maps/nav.tsx"), NAV_TSX).unwrap();
    std::fs::write(dir.join("maps/nav.tmx"), NAV_TMX).unwrap();
    std::fs::write(dir.join("maps/better-tiled.json"), setup).unwrap();
    dir
}

fn export_nav(dir: &Path) -> (Report, String) {
    let target = dir.join("nav.tscn");
    let report = export(
        &dir.join("maps/nav.tmx"),
        &mut loader(dir),
        &target,
        &Options::default(),
    )
    .unwrap();
    (report, std::fs::read_to_string(&target).unwrap())
}

/// The lines of the sub-resource `id`, or of the atlas source named `name` (`resource_name`), or
/// of the TileSet (`TileSet`: in a scene, or a `.tres`'s own).
fn lines_of(text: &str, name: &str) -> Vec<(String, String)> {
    let resource = read::parse(text).unwrap();
    let mut subs = resource.sub_resources();
    let section = match name {
        "TileSet" => subs
            .find(|(sub, _)| sub.class == "TileSet")
            .map(|(_, s)| s)
            .or(resource.resource()),
        _ => subs
            .find(|(sub, section)| {
                sub.id == name
                    || section
                        .get("resource_name")
                        .and_then(read::string)
                        .as_deref()
                        == Some(name)
            })
            .map(|(_, s)| s),
    }
    .unwrap_or_else(|| panic!("no {name}"));
    section
        .props
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn under(lines: &[(String, String)], prefix: &str) -> Vec<(String, String)> {
    lines
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(prefix)?.to_owned(), v.clone())))
        .filter(|(k, _)| k != "texture_origin")
        .collect()
}

/// The sub-resource id a `SubResource("…")` names.
fn sub_id(value: &str) -> &str {
    value
        .strip_prefix("SubResource(\"")
        .and_then(|v| v.strip_suffix("\")"))
        .unwrap_or_else(|| panic!("{value} isn't a sub-resource"))
}

fn get<'l>(lines: &'l [(String, String)], key: &str) -> &'l str {
    lines
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("no {key} in {lines:?}"))
}

fn keys(lines: &[(String, String)]) -> Vec<&str> {
    lines.iter().map(|(k, _)| k.as_str()).collect()
}

#[test]
fn shapes_become_navigation_polygons_and_occluders_on_the_setups_layers() {
    let dir = nav_project("navigation", SETUP);
    let (report, tscn) = export_nav(&dir);

    // The layers, from the setup, before the sources.
    let tileset = lines_of(&tscn, "TileSet");
    let areas: Vec<(String, String)> = tileset
        .iter()
        .filter(|(k, _)| k.starts_with("occlusion_layer_") || k.starts_with("navigation_layer_"))
        .cloned()
        .collect();
    let pairs = |p: &[(&str, &str)]| {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        areas,
        pairs(&[
            ("occlusion_layer_0/light_mask", "3"),
            ("occlusion_layer_1/light_mask", "1"),
            ("navigation_layer_0/layers", "1"),
            ("navigation_layer_1/layers", "6")
        ])
    );
    let first_source = keys(&tileset)
        .iter()
        .position(|k| k.starts_with("sources/"))
        .unwrap();
    assert!(
        keys(&tileset)[first_source..]
            .iter()
            .all(|k| k.starts_with("sources/")),
        "{tileset:?}"
    );

    // Every sub-resource a tile names comes before the atlas sources, once.
    let first_atlas = tscn
        .find("[sub_resource type=\"TileSetAtlasSource\"")
        .unwrap();
    for kind in ["NavigationPolygon", "OccluderPolygon2D"] {
        let header = format!("[sub_resource type=\"{kind}\"");
        assert!(
            tscn.match_indices(&header).all(|(at, _)| at < first_atlas),
            "{kind} after the atlas sources"
        );
    }

    // Tile 0: a physics polygon and a navigation polygon, one convex part.
    let nav = lines_of(&tscn, "maps/nav.tsx");
    let tile = |x: i32, y: i32| under(&nav, &format!("{x}:{y}/0/"));
    assert_eq!(
        keys(&tile(0, 0)),
        [
            "physics_layer_0/polygon_0/points",
            "navigation_layer_0/polygon"
        ]
    );
    let square = sub_id(get(&tile(0, 0), "navigation_layer_0/polygon")).to_owned();
    assert!(square.starts_with("NavigationPolygon_"), "{square}");
    assert_eq!(
        lines_of(&tscn, &square),
        pairs(&[
            ("vertices", "PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)"),
            (
                "polygons",
                "Array[PackedInt32Array]([PackedInt32Array(0, 1, 2, 3)])"
            ),
            (
                "outlines",
                "Array[PackedVector2Array]([PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)])"
            ),
        ])
    );
    // Tile 4 has the same square: the same sub-resource.
    assert_eq!(
        sub_id(get(&tile(0, 1), "navigation_layer_0/polygon")),
        square
    );
    assert_eq!(tscn.matches(&format!("id=\"{square}\"")).count(), 1);

    // Tile 1: kept out of physics, walkable on layer 1 in two convex parts.
    assert_eq!(keys(&tile(1, 0)), ["navigation_layer_1/polygon"]);
    let l = lines_of(
        &tscn,
        sub_id(get(&tile(1, 0), "navigation_layer_1/polygon")),
    );
    assert_eq!(
        get(&l, "vertices"),
        "PackedVector2Array(-8, -8, 8, -8, 8, -2, -2, -2, -2, 8, -8, 8)"
    );
    assert_eq!(
        get(&l, "polygons").matches("PackedInt32Array(").count(),
        2,
        "{l:?}"
    );

    // Tile 2: occluders numbered per layer, Godot's order (occluders, physics), and the triangle
    // on physics too.
    assert_eq!(
        keys(&tile(2, 0)),
        [
            "occlusion_layer_0/polygon_0/polygon",
            "occlusion_layer_1/polygon_0/polygon",
            "occlusion_layer_0/polygon_1/polygon",
            "physics_layer_0/polygon_0/points"
        ]
    );
    let occluder = |key: &str| lines_of(&tscn, sub_id(get(&tile(2, 0), key)));
    assert_eq!(
        occluder("occlusion_layer_0/polygon_0/polygon"),
        pairs(&[("polygon", "PackedVector2Array(-8, -8, 8, -8, 8, 0, -8, 0)")])
    );
    assert_eq!(
        occluder("occlusion_layer_1/polygon_0/polygon"),
        pairs(&[("polygon", "PackedVector2Array(-4, -4, 4, -4, 4, 4, -4, 4)")])
    );
    assert_eq!(
        occluder("occlusion_layer_0/polygon_1/polygon"),
        pairs(&[("polygon", "PackedVector2Array(-8, 0, 8, 0, -8, 8)")])
    );

    // Tile 3: a bow tie, left out of navigation (and of physics, as it asks).
    assert_eq!(tile(3, 0), []);

    // The big tile's flipped alternative: its polygons moved as its physics polygons are.
    let big = lines_of(&tscn, "maps/nav.tmx::big");
    let (plain, flipped) = (under(&big, "0:0/0/"), under(&big, "0:0/1/"));
    assert_eq!(
        keys(&plain),
        [
            "occlusion_layer_0/polygon_0/polygon",
            "physics_layer_0/polygon_0/points",
            "navigation_layer_0/polygon"
        ]
    );
    assert_eq!(keys(&flipped), keys(&plain));
    let points = |text: &str| -> Vec<DVec2> {
        let numbers: Vec<f64> = text
            .trim_start_matches("PackedVector2Array(")
            .trim_end_matches(')')
            .split(", ")
            .map(|n| n.parse().unwrap())
            .collect();
        numbers.chunks(2).map(|c| DVec2::new(c[0], c[1])).collect()
    };
    let moved = points(get(&flipped, "physics_layer_0/polygon_0/points"))[0]
        - points(get(&plain, "physics_layer_0/polygon_0/points"))[0];
    assert_ne!(moved, DVec2::ZERO);
    for (key, field) in [
        ("navigation_layer_0/polygon", "vertices"),
        ("occlusion_layer_0/polygon_0/polygon", "polygon"),
    ] {
        let (a, b) = (
            points(get(&lines_of(&tscn, sub_id(get(&plain, key))), field)),
            points(get(&lines_of(&tscn, sub_id(get(&flipped, key))), field)),
        );
        assert_eq!(b, a.iter().map(|p| *p + moved).collect::<Vec<_>>(), "{key}");
    }

    // The notes: the L split (a note, not a warning), the bow tie, occlusion layer 1's bits.
    let notes: Vec<(&str, Severity, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.severity, n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "godot/navigation-shape",
                Severity::Warning,
                "tile collision shapes on a navigation layer whose edges cross or touch, or that have no area, can't be split into the convex polygons Godot's navigation takes: they are left out of navigation: redraw them so that their edges don't meet (tileset nav: tile 3)"
            ),
            (
                "godot/navigation-split",
                Severity::Info,
                "tile collision shapes on a navigation layer that aren't convex are split into convex polygons, which is what Godot's navigation takes (tileset nav: tile 1)"
            ),
            (
                "godot/occlusion-layer-unset",
                Severity::Warning,
                "occlusion layer 1 has no entry in occlusionLayers of better-tiled.json's godot setup: Godot gives it light mask 1"
            ),
            (
                "godot/physics-quadrant-size",
                Severity::Info,
                "layer cells: mirrored tiles with collision shapes that can overlap another cell's, so each cell collides on its own (physics_quadrant_size = 1); Godot 4.5 and later merge the shapes of 16×16 cells into one body, which cuts a mirrored tile's shapes away where they overlap another cell's"
            ),
        ]
    );
    let consumed: Vec<&str> = report.consumed.iter().map(String::as_str).collect();
    assert_eq!(
        consumed,
        [
            "godotCollision",
            "godotNavigationLayer",
            "godotOcclusionLayer"
        ]
    );

    // The same bytes on a second run.
    let (_, again) = export_nav(&dir);
    assert_eq!(
        tscn.split_once("[gd_scene").unwrap().1,
        again.split_once("[gd_scene").unwrap().1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn without_a_setup_the_layers_have_godots_defaults() {
    let dir = nav_project("navigation-nosetup", r#"{"betterTiled": 1}"#);
    let (report, tscn) = export_nav(&dir);
    let tileset = lines_of(&tscn, "TileSet");
    let areas: Vec<&str> = tileset
        .iter()
        .filter(|(k, _)| k.starts_with("occlusion_layer_") || k.starts_with("navigation_layer_"))
        .map(|(k, v)| if v == "1" { k.as_str() } else { "not 1" })
        .collect();
    assert_eq!(
        areas,
        [
            "occlusion_layer_0/light_mask",
            "occlusion_layer_1/light_mask",
            "navigation_layer_0/layers",
            "navigation_layer_1/layers"
        ]
    );
    let unset: Vec<&str> = report
        .notes
        .iter()
        .filter(|n| n.code.as_str().ends_with("-layer-unset"))
        .map(|n| n.message.as_str())
        .collect();
    assert_eq!(
        unset,
        [
            "navigation layer 1 has no entry in navigationLayers of better-tiled.json's godot setup: Godot gives it navigation layers 1",
            "occlusion layer 1 has no entry in occlusionLayers of better-tiled.json's godot setup: Godot gives it light mask 1"
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A map of the same grid with a tileset of its own and no navigation.
const OTHER_TMX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="2" height="1" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" name="other" tilewidth="16" tileheight="16" tilecount="16" columns="4">
  <image source="tiles.png" width="64" height="64"/>
 </tileset>
 <layer id="1" name="cells" width="2" height="1">
  <data encoding="csv">
1,2
</data>
 </layer>
</map>
"#;

/// A shared TileSet has the sub-resources too, before its atlas sources, and keeps the layers of
/// the sources it keeps.
#[test]
fn a_shared_tileset_has_the_navigation_polygons_before_its_sources() {
    let dir = nav_project(
        "navigation-shared",
        r#"{"betterTiled": 1, "godot": {"project": ".."}}"#,
    );
    let (_, tscn) = export_nav(&dir);
    assert!(
        !tscn.contains("NavigationPolygon"),
        "the scene holds no TileSet"
    );
    let tres_path = std::fs::read_dir(dir.join("tilesets"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "tres"))
        .unwrap();
    let tres = std::fs::read_to_string(&tres_path).unwrap();
    let first_atlas = tres
        .find("[sub_resource type=\"TileSetAtlasSource\"")
        .unwrap();
    let navs: Vec<usize> = tres
        .match_indices("[sub_resource type=\"NavigationPolygon\"")
        .map(|(at, _)| at)
        .collect();
    assert!(
        navs.len() >= 3,
        "the square, the L and the big tile's flipped ones"
    );
    assert!(navs.iter().all(|&at| at < first_atlas));
    let tileset = lines_of(&tres, "TileSet");
    assert_eq!(get(&tileset, "navigation_layer_1/layers"), "1");
    let nav = lines_of(&tres, "maps/nav.tsx");
    assert_eq!(keys(&under(&nav, "1:0/0/")), ["navigation_layer_1/polygon"]);

    // Another map's export, with nav.tsx gone: its sources are kept as they were, and the
    // TileSet still has their layers.
    std::fs::write(dir.join("maps/other.tmx"), OTHER_TMX).unwrap();
    std::fs::remove_file(dir.join("maps/nav.tsx")).unwrap();
    let report = export(
        &dir.join("maps/other.tmx"),
        &mut loader(&dir),
        &dir.join("other.tscn"),
        &Options::default(),
    );
    assert!(report.is_ok(), "{:?}", report.err().map(String::from));
    let tres = std::fs::read_to_string(&tres_path).unwrap();
    let tileset = lines_of(&tres, "TileSet");
    assert_eq!(
        get(&tileset, "navigation_layer_1/layers"),
        "1",
        "{tileset:?}"
    );
    assert_eq!(get(&tileset, "occlusion_layer_1/light_mask"), "1");
    let kept = lines_of(&tres, "maps/nav.tsx");
    assert_eq!(
        get(&kept, "1:0/0/navigation_layer_1/polygon"),
        get(&nav, "1:0/0/navigation_layer_1/polygon"),
        "the same sub-resource, by the same id"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Loads a scene, adds it to the tree and, once the navigation map has its regions, describes
/// every painted cell (its navigation polygons, occluders and physics polygons per layer, as
/// Godot reads them and as it flips them for the cell) and the TileSet's layers, and finds paths.
const NAVIGATE: &str = r#"extends SceneTree

var room: Node
var frames := 0
var settled := 0

func _initialize() -> void:
	room = (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	root.add_child(room)

func pts(points: PackedVector2Array) -> Array:
	var out := []
	for v: Vector2 in points:
		out.append([v.x, v.y])
	return out

func _process(_delta: float) -> bool:
	frames += 1
	var layer := room.find_children("*", "TileMapLayer", true, false)[0] as TileMapLayer
	var map := layer.get_navigation_map()
	# The map takes a few syncs to have the regions joined: both ways found, then a few frames
	# more (the second can come a sync after the first, more so when Godot runs slowly).
	if frames < 120:
		if NavigationServer2D.map_get_path(map, Vector2(4, 24), Vector2(44, 24), true, 1).is_empty() or NavigationServer2D.map_get_path(map, Vector2(16 + 13, 3), Vector2(16 + 3, 13), true, 2).is_empty():
			return false
		settled += 1
		if settled < 10:
			return false
	var tileset := layer.tile_set
	var result := {"navigation": [], "occlusion": [], "cells": {}, "paths": {}}
	for i in tileset.get_navigation_layers_count():
		result["navigation"].append(tileset.get_navigation_layer_layers(i))
	for i in tileset.get_occlusion_layers_count():
		result["occlusion"].append(tileset.get_occlusion_layer_light_mask(i))
	for c: Vector2i in layer.get_used_cells():
		var data := layer.get_cell_tile_data(c)
		var alt := layer.get_cell_alternative_tile(c)
		var flip_h := alt & TileSetAtlasSource.TRANSFORM_FLIP_H != 0
		var navigation := []
		for l in tileset.get_navigation_layers_count():
			var polygon := data.get_navigation_polygon(l, flip_h)
			if polygon == null:
				navigation.append(null)
				continue
			var polygons := []
			for p in polygon.get_polygon_count():
				polygons.append(Array(polygon.get_polygon(p)))
			navigation.append({"vertices": pts(polygon.vertices), "polygons": polygons})
		var occlusion := []
		for l in tileset.get_occlusion_layers_count():
			var occluders := []
			for p in data.get_occluder_polygons_count(l):
				occluders.append(pts(data.get_occluder_polygon(l, p, flip_h).polygon))
			occlusion.append(occluders)
		var physics := []
		for l in tileset.get_physics_layers_count():
			physics.append(data.get_collision_polygons_count(l))
		var centre := layer.map_to_local(c)
		result["cells"]["%d,%d" % [c.x, c.y]] = {"centre": [centre.x, centre.y], "navigation": navigation, "occlusion": occlusion, "physics": physics}
	# Along the row of squares (navigation layer 0, bit 1), and round the L's corner (layer 1,
	# bits 2 and 3).
	result["paths"]["row"] = pts(NavigationServer2D.map_get_path(map, Vector2(4, 24), Vector2(44, 24), true, 1))
	result["paths"]["corner"] = pts(NavigationServer2D.map_get_path(map, Vector2(16 + 13, 3), Vector2(16 + 3, 13), true, 2))
	result["paths"]["not_on_bit_1"] = pts(NavigationServer2D.map_get_path(map, Vector2(16 + 13, 3), Vector2(16 + 3, 13), true, 1))
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	return true
"#;

fn godot_navigate(godot: &Path, dir: &Path, scene: &str) -> (Json, String) {
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
    std::fs::write(dir.join("navigate.gd"), NAVIGATE).unwrap();
    let output = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(dir)
        .args(["--script", "res://navigate.gd", "--", scene])
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

fn json_points(points: &Json) -> Vec<DVec2> {
    points
        .as_array()
        .unwrap()
        .iter()
        .map(|p| DVec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

/// Whether `p` is inside the L of tile 1 (painted at 1,0), its arms 6 px wide.
fn in_l(p: DVec2) -> bool {
    let (x, y) = (p.x - 16.0, p.y);
    (-0.01..=16.01).contains(&x) && (-0.01..=16.01).contains(&y) && (x <= 6.01 || y <= 6.01)
}

#[test]
fn godot_reads_the_navigation_polygons_and_occluders_and_finds_its_way() {
    let Some(godot) = godot() else { return };
    let dir = nav_project("navigation-godot", SETUP);
    export_nav(&dir);
    let (scene, stderr) = godot_navigate(&godot, &dir, "res://nav.tscn");
    assert_eq!(stderr, "", "Godot loads it cleanly");
    assert_eq!(scene["navigation"], serde_json::json!([1, 6]));
    assert_eq!(scene["occlusion"], serde_json::json!([3, 1]));
    let cell = |c: &str| &scene["cells"][c];

    // Tile 0: walkable on layer 0, one part, and a physics polygon; tile 1 walkable on layer 1 in
    // two parts, with no physics polygon.
    assert_eq!(
        cell("0,0")["navigation"][0]["polygons"],
        serde_json::json!([[0, 1, 2, 3]])
    );
    assert_eq!(
        json_points(&cell("0,0")["navigation"][0]["vertices"]),
        [
            DVec2::new(-8.0, -8.0),
            DVec2::new(8.0, -8.0),
            DVec2::new(8.0, 8.0),
            DVec2::new(-8.0, 8.0)
        ]
    );
    assert_eq!(cell("0,0")["physics"], serde_json::json!([1]));
    assert!(cell("1,0")["navigation"][0].is_null());
    assert_eq!(
        cell("1,0")["navigation"][1]["polygons"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        json_points(&cell("1,0")["navigation"][1]["vertices"]).len(),
        6
    );
    assert_eq!(
        cell("1,0")["physics"],
        serde_json::json!([0]),
        "godotCollision = false"
    );

    // Tile 2's occluders on their layers; tile 3 (the bow tie) not walkable.
    let occlusion = &cell("2,0")["occlusion"];
    assert_eq!(
        json_points(&occlusion[0][0]),
        [
            DVec2::new(-8.0, -8.0),
            DVec2::new(8.0, -8.0),
            DVec2::new(8.0, 0.0),
            DVec2::new(-8.0, 0.0)
        ]
    );
    assert_eq!(
        json_points(&occlusion[0][1]),
        [
            DVec2::new(-8.0, 0.0),
            DVec2::new(8.0, 0.0),
            DVec2::new(-8.0, 8.0)
        ]
    );
    assert_eq!(
        json_points(&occlusion[1][0]),
        [
            DVec2::new(-4.0, -4.0),
            DVec2::new(4.0, -4.0),
            DVec2::new(4.0, 4.0),
            DVec2::new(-4.0, 4.0)
        ]
    );
    assert!(cell("3,0")["navigation"][0].is_null() && cell("3,0")["navigation"][1].is_null());

    // The big tile's shapes are where Tiled draws them, plain and flipped: its bottom-left
    // quarter, or mirrored, its bottom-right one (the image's bottom is the cell's).
    for (c, flip) in [("1,2", false), ("3,2", true)] {
        let centre = DVec2::new(
            cell(c)["centre"][0].as_f64().unwrap(),
            cell(c)["centre"][1].as_f64().unwrap(),
        );
        let x = centre.x - 8.0;
        let left = if flip { x + 16.0 } else { x };
        let expected = [
            DVec2::new(left, centre.y - 8.0),
            DVec2::new(left + 16.0, centre.y - 8.0),
            DVec2::new(left + 16.0, centre.y + 8.0),
            DVec2::new(left, centre.y + 8.0),
        ];
        let found = |points: Vec<DVec2>| {
            let mut points: Vec<(i64, i64)> = points
                .iter()
                .map(|p| {
                    (
                        (p.x + centre.x).round() as i64,
                        (p.y + centre.y).round() as i64,
                    )
                })
                .collect();
            points.sort_unstable();
            points
        };
        let mut want: Vec<(i64, i64)> = expected.iter().map(|p| (p.x as i64, p.y as i64)).collect();
        want.sort_unstable();
        assert_eq!(
            found(json_points(&cell(c)["navigation"][0]["vertices"])),
            want,
            "{c} navigation"
        );
        assert_eq!(
            found(json_points(&cell(c)["occlusion"][0][0])),
            want,
            "{c} occluder"
        );
    }

    // Godot finds its way: along the row, and round the L's corner, only on the L's bits.
    let near = |path: &[DVec2], from: DVec2, to: DVec2| {
        path.len() >= 2 && path[0].distance(from) < 0.01 && path[path.len() - 1].distance(to) < 0.01
    };
    let row = json_points(&scene["paths"]["row"]);
    assert!(
        near(&row, DVec2::new(4.0, 24.0), DVec2::new(44.0, 24.0)),
        "{row:?}"
    );
    let corner = json_points(&scene["paths"]["corner"]);
    assert!(
        corner.len() >= 3 && near(&corner, DVec2::new(29.0, 3.0), DVec2::new(19.0, 13.0)),
        "it turns the corner: {corner:?}"
    );
    assert!(
        corner.iter().all(|p| in_l(*p)),
        "it stays on the L: {corner:?}"
    );
    // On bit 1 there are only the squares: the way starts at the nearest of them, not on the L.
    let other = json_points(&scene["paths"]["not_on_bit_1"]);
    assert!(
        other
            .first()
            .is_none_or(|p| p.distance(DVec2::new(29.0, 3.0)) > 10.0),
        "{other:?}"
    );
    std::fs::remove_dir_all(&dir).ok();

    // A shared TileSet gives Godot the same.
    let shared = SETUP.replace(r#""godot": {"#, r#""godot": {"project": "..", "#);
    let dir = nav_project("navigation-godot-shared", &shared);
    export_nav(&dir);
    assert!(dir.join("tilesets").is_dir());
    let (from_shared, stderr) = godot_navigate(&godot, &dir, "res://nav.tscn");
    assert_eq!(stderr, "", "Godot loads it cleanly");
    assert_eq!(from_shared, scene);
    std::fs::remove_dir_all(&dir).ok();
}
