#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
mod cases {
    // Terrains (docs/design/12-godot-export.md, §7): Tiled's Wang sets as the TileSet's terrain sets, checked against
    // Godot's own peering bits, neighbours and terrain painting.

    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};

    use bt_core::{LayerKind, Map, Orientation, StaggerAxis, TilesetSlot, WangId, WangSetType};
    use glam::IVec2;
    use serde_json::Value as Json;

    use crate::report::Element;
    use crate::terrain::{NEIGHBOURS, mapping};
    use crate::tests::oracle::{json_between_markers, run_godot};
    use crate::tests::support::*;
    use crate::{Options, Report, export, read};

    /// The peering bits Godot 4.7.2 takes (`TileData.is_valid_terrain_peering_bit`) on the grid of a
    /// map of `orientation`, in terrain mode 0 (corners and sides), 1 (corners) or 2 (sides), as
    /// Godot printed them headless for each tile shape and offset axis.
    fn valid(orientation: Orientation, axis: StaggerAxis, mode: u8) -> Vec<usize> {
        let (corners, sides): (&[usize], &[usize]) = match orientation {
            Orientation::Orthogonal => (&[3, 7, 11, 15], &[0, 4, 8, 12]),
            Orientation::Isometric | Orientation::Staggered => (&[1, 5, 9, 13], &[2, 6, 10, 14]),
            // Hexagons and half-offset squares alike.
            Orientation::Hexagonal if axis == StaggerAxis::X => {
                (&[1, 3, 7, 9, 11, 15], &[2, 4, 6, 10, 12, 14])
            }
            Orientation::Hexagonal => (&[3, 5, 7, 11, 13, 15], &[0, 2, 6, 8, 10, 14]),
            Orientation::Oblique => (&[], &[]),
        };
        let mut bits: Vec<usize> = match mode {
            0 => corners.iter().chain(sides).copied().collect(),
            1 => corners.to_vec(),
            _ => sides.to_vec(),
        };
        bits.sort_unstable();
        bits
    }

    const GRIDS: [(Orientation, StaggerAxis); 6] = [
        (Orientation::Orthogonal, StaggerAxis::Y),
        (Orientation::Isometric, StaggerAxis::Y),
        (Orientation::Staggered, StaggerAxis::Y),
        (Orientation::Staggered, StaggerAxis::X),
        (Orientation::Hexagonal, StaggerAxis::Y),
        (Orientation::Hexagonal, StaggerAxis::X),
    ];

    #[test]
    fn wang_indexes_go_on_bits_godot_takes_on_every_grid() {
        let kinds = [WangSetType::Corner, WangSetType::Edge, WangSetType::Mixed];
        for (orientation, axis) in GRIDS {
            let hex = orientation == Orientation::Hexagonal;
            for kind in kinds {
                let what = format!("{orientation:?} {axis:?} {kind:?}");
                let Some((mode, bits)) = mapping(orientation, axis, kind) else {
                    assert!(
                        hex && kind == WangSetType::Corner,
                        "{what}: Godot has a set for it"
                    );
                    continue;
                };
                let expected_mode = match (hex, kind) {
                    (true, _) | (false, WangSetType::Edge) => 2,
                    (false, WangSetType::Corner) => 1,
                    (false, WangSetType::Mixed) => 0,
                };
                assert_eq!(mode, expected_mode, "{what}");
                let on: Vec<usize> = bits.iter().flatten().copied().collect();
                let good = valid(orientation, axis, mode);
                assert!(
                    on.iter().all(|b| good.contains(b)),
                    "{what}: {on:?} within {good:?}"
                );
                let mut distinct = on.clone();
                distinct.sort_unstable();
                distinct.dedup();
                assert_eq!(distinct.len(), on.len(), "{what}: one index per bit");
                // A colour's opposite index goes on the opposite bit, where the neighbour across has it.
                for i in 0..4 {
                    if let (Some(a), Some(b)) = (bits[i], bits[i + 4]) {
                        assert_eq!((a + 8) % 16, b, "{what}: indexes {i} and {}", i + 4);
                    }
                }
                let used: Vec<usize> = (0..8).filter(|&i| bits[i].is_some()).collect();
                let expected: Vec<usize> = match (hex, kind) {
                    (_, WangSetType::Edge) => vec![0, 2, 4, 6],
                    (false, WangSetType::Corner) => vec![1, 3, 5, 7],
                    (false, _) => (0..8).collect(),
                    // Without the corners toward the cells two rows or columns away.
                    (true, _) if axis == StaggerAxis::Y => vec![0, 1, 2, 4, 5, 6],
                    (true, _) => vec![0, 2, 3, 4, 6, 7],
                };
                assert_eq!(used, expected, "{what}");
                if !hex {
                    // Edge and mixed sets on hexagonal maps use every bit there is: the 6 sides.
                    assert!(
                        kind != WangSetType::Mixed || on.len() == good.len(),
                        "{what}"
                    );
                } else {
                    assert!(kind != WangSetType::Mixed || on.len() == 6, "{what}");
                }
            }
        }
        let mixed = |orientation, axis| {
            mapping(orientation, axis, WangSetType::Mixed)
                .unwrap()
                .1
                .map(Option::unwrap)
        };
        assert_eq!(
            mixed(Orientation::Orthogonal, StaggerAxis::Y),
            [12, 15, 0, 3, 4, 7, 8, 11]
        );
        assert_eq!(
            mixed(Orientation::Isometric, StaggerAxis::Y),
            [14, 1, 2, 5, 6, 9, 10, 13]
        );
        assert_eq!(
            mixed(Orientation::Staggered, StaggerAxis::X),
            [14, 1, 2, 5, 6, 9, 10, 13]
        );
    }

    /// Three Wang sets on one atlas: a corner set (with a tile of a colour it lacks), an edge set
    /// sharing tile 0 with it, and a mixed set of three colours with an animated tile.
    const W_TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="w" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="8">
  <animation>
   <frame tileid="8" duration="100"/>
   <frame tileid="9" duration="100"/>
  </animation>
 </tile>
 <wangsets>
  <wangset name="Corners" type="corner" tile="-1">
   <wangcolor name="Grass" color="#00ff00" tile="-1" probability="1"/>
   <wangcolor name="Water" color="#0000ff" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,2,0,2"/>
   <wangtile tileid="1" wangid="0,2,0,2,0,2,0,1"/>
   <wangtile tileid="2" wangid="0,3,0,1,0,1,0,1"/>
   <wangtile tileid="3" wangid="1,1,1,1,1,1,1,1"/>
  </wangset>
  <wangset name="Edges" type="edge" tile="-1">
   <wangcolor name="Road" color="#ffffff" tile="-1" probability="1"/>
   <wangcolor name="Mud" color="#000000" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="1,0,1,0,1,0,1,0"/>
   <wangtile tileid="4" wangid="1,0,2,0,1,0,2,0"/>
  </wangset>
  <wangset name="Mixed" type="mixed" tile="-1">
   <wangcolor name="A" color="#ff0000" tile="-1" probability="1"/>
   <wangcolor name="B" color="#00ffff" tile="-1" probability="1"/>
   <wangcolor name="C" color="#ffff00" tile="-1" probability="1"/>
   <wangtile tileid="5" wangid="1,2,3,1,2,3,1,2"/>
   <wangtile tileid="8" wangid="1,1,1,1,1,1,1,1"/>
  </wangset>
 </wangsets>
</tileset>
"##;

    const H: u32 = 0x8000_0000;

    /// A 4×2 map of `w.tsx` on a grid of `grid` (the map element's attributes): tiles 0, 1, 3 and
    /// the animated 8, then 0 and 3 flipped sideways and 5.
    fn w_map(grid: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" {grid} renderorder="right-down" width="4" height="2" infinite="0" nextlayerid="2" nextobjectid="1">
 <tileset firstgid="1" source="w.tsx"/>
 <layer id="1" name="ground" width="4" height="2">
  <data encoding="csv">
1,2,4,9,
{},{},6,0
</data>
 </layer>
</map>
"#,
            1 | H,
            4 | H
        )
    }

    const ORTHO: &str = r#"orientation="orthogonal" tilewidth="16" tileheight="16""#;
    const HEX: &str = r#"orientation="hexagonal" tilewidth="14" tileheight="12" hexsidelength="6" staggeraxis="y" staggerindex="odd""#;

    fn w_export(name: &str, grid: &str) -> (PathBuf, Report, String) {
        let dir = project(name);
        std::fs::write(dir.join("maps/w.tsx"), W_TSX).unwrap();
        std::fs::write(dir.join("maps/w.tmx"), w_map(grid)).unwrap();
        let target = dir.join("w.tscn");
        let report = export(
            &dir.join("maps/w.tmx"),
            &mut loader(&dir),
            &target,
            &Options::default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let tscn = std::fs::read_to_string(&target).unwrap();
        (dir, report, tscn)
    }

    /// The lines of a resource's TileSet (its own lines) or of its atlas source named `name`.
    fn lines(text: &str, name: &str) -> Vec<(String, String)> {
        let resource = read::parse(text).unwrap();
        let section = match name {
            "TileSet" => resource
                .sub_resources()
                .find(|(sub, _)| sub.class == "TileSet")
                .map(|(_, s)| s)
                .or_else(|| resource.resource()),
            _ => resource
                .sub_resources()
                .find(|(_, s)| {
                    s.get("resource_name").and_then(read::string).as_deref() == Some(name)
                })
                .map(|(_, s)| s),
        };
        section
            .unwrap_or_else(|| panic!("no {name}"))
            .props
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// The terrain lines among `lines`, those under `prefix` (`0:0/0/`) without it.
    fn terrain(lines: &[(String, String)], prefix: &str) -> Vec<(String, String)> {
        lines
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix(prefix)?.to_owned(), v.clone())))
            .filter(|(k, _)| k.starts_with("terrain"))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Every peering bit in the resource is one Godot takes in its tile's terrain set's mode.
    fn assert_bits_valid(text: &str, orientation: Orientation, axis: StaggerAxis) -> usize {
        let resource = read::parse(text).unwrap();
        let tileset = lines(text, "TileSet");
        let mode = |set: &str| {
            tileset
                .iter()
                .find(|(k, _)| *k == format!("terrain_set_{set}/mode"))
                .map(|(_, v)| v.parse::<u8>().unwrap())
                .unwrap_or_else(|| panic!("no terrain set {set}"))
        };
        let mut checked = 0;
        for (sub, section) in resource
            .sub_resources()
            .filter(|(sub, _)| sub.class == "TileSetAtlasSource")
        {
            for (key, _) in &section.props {
                let Some((tile, name)) = key.split_once("/terrains_peering_bit/") else {
                    continue;
                };
                let set = section
                    .get(&format!("{tile}/terrain_set"))
                    .unwrap_or_else(|| panic!("{}: {key} without a terrain set", sub.id));
                let bit = NEIGHBOURS
                    .iter()
                    .position(|n| n == &name)
                    .unwrap_or_else(|| panic!("{key}: no such bit"));
                assert!(
                    valid(orientation, axis, mode(set)).contains(&bit),
                    "{key}: not a bit of mode {}",
                    mode(set)
                );
                checked += 1;
            }
        }
        checked
    }

    #[test]
    fn wang_sets_become_terrain_sets_with_their_colours() {
        let (dir, report, tscn) = w_export("terrain-sets", ORTHO);
        let tileset: Vec<(String, String)> = lines(&tscn, "TileSet")
            .into_iter()
            .filter(|(k, _)| k.starts_with("terrain_set_"))
            .collect();
        let expected = pairs(&[
            ("terrain_set_0/mode", "1"),
            ("terrain_set_0/terrain_0/name", "\"Grass\""),
            ("terrain_set_0/terrain_0/color", "Color(0, 1, 0, 1)"),
            ("terrain_set_0/terrain_1/name", "\"Water\""),
            ("terrain_set_0/terrain_1/color", "Color(0, 0, 1, 1)"),
            ("terrain_set_1/mode", "2"),
            ("terrain_set_1/terrain_0/name", "\"Road\""),
            ("terrain_set_1/terrain_0/color", "Color(1, 1, 1, 1)"),
            ("terrain_set_1/terrain_1/name", "\"Mud\""),
            ("terrain_set_1/terrain_1/color", "Color(0, 0, 0, 1)"),
            ("terrain_set_2/mode", "0"),
            ("terrain_set_2/terrain_0/name", "\"A\""),
            ("terrain_set_2/terrain_0/color", "Color(1, 0, 0, 1)"),
            ("terrain_set_2/terrain_1/name", "\"B\""),
            ("terrain_set_2/terrain_1/color", "Color(0, 1, 1, 1)"),
            ("terrain_set_2/terrain_2/name", "\"C\""),
            ("terrain_set_2/terrain_2/color", "Color(1, 1, 0, 1)"),
        ]);
        assert_eq!(tileset, expected);
        assert_eq!(report.counts.terrain_sets, 3);

        let base = lines(&tscn, "maps/w.tsx");
        let corner = |centre: &str, br: &str, bl: &str, tl: &str, tr: &str| {
            pairs(&[
                ("terrain_set", "0"),
                ("terrain", centre),
                ("terrains_peering_bit/bottom_right_corner", br),
                ("terrains_peering_bit/bottom_left_corner", bl),
                ("terrains_peering_bit/top_left_corner", tl),
                ("terrains_peering_bit/top_right_corner", tr),
            ])
        };
        // Two of each colour: the lower one is the tile's own.
        assert_eq!(
            terrain(&base, "0:0/0/"),
            corner("0", "0", "1", "1", "0"),
            "tile 0, the corner set's (the first) alone"
        );
        assert_eq!(
            terrain(&base, "1:0/0/"),
            corner("1", "1", "1", "0", "1"),
            "tile 1: three of colour 2"
        );
        assert_eq!(
            terrain(&base, "2:0/0/"),
            [],
            "tile 2 has a colour the set lacks: Tiled has it in no set"
        );
        assert_eq!(
            terrain(&base, "3:0/0/"),
            corner("0", "0", "0", "0", "0"),
            "tile 3: an edge set's indexes aren't a corner set's"
        );
        let edge = pairs(&[
            ("terrain_set", "1"),
            ("terrain", "0"),
            ("terrains_peering_bit/right_side", "1"),
            ("terrains_peering_bit/bottom_side", "0"),
            ("terrains_peering_bit/left_side", "1"),
            ("terrains_peering_bit/top_side", "0"),
        ]);
        assert_eq!(terrain(&base, "0:1/0/"), edge, "tile 4");
        let mixed = pairs(&[
            ("terrain_set", "2"),
            ("terrain", "0"),
            ("terrains_peering_bit/right_side", "2"),
            ("terrains_peering_bit/bottom_right_corner", "0"),
            ("terrains_peering_bit/bottom_side", "1"),
            ("terrains_peering_bit/bottom_left_corner", "2"),
            ("terrains_peering_bit/left_side", "0"),
            ("terrains_peering_bit/top_left_corner", "1"),
            ("terrains_peering_bit/top_side", "0"),
            ("terrains_peering_bit/top_right_corner", "1"),
        ]);
        assert_eq!(
            terrain(&base, "1:1/0/"),
            mixed,
            "tile 5: three each of A and B, so A"
        );
        // The animated tile's cells use its animation source: Godot's terrain painting places it.
        assert_eq!(
            terrain(&base, "0:2/0/"),
            [],
            "the still copy of the animated tile"
        );
        let animated = terrain(&lines(&tscn, "maps/w.tsx#anim1"), "0:2/0/");
        assert_eq!(
            animated[..2],
            pairs(&[("terrain_set", "2"), ("terrain", "0")])
        );
        assert_eq!(animated.len(), 10, "{animated:?}");
        assert_eq!(
            assert_bits_valid(&tscn, Orientation::Orthogonal, StaggerAxis::Y),
            4 + 4 + 4 + 4 + 8 + 8,
            "every bit is one Godot takes"
        );

        let notes: BTreeMap<&str, (&str, &[Element])> = report
            .notes
            .iter()
            .filter(|n| n.code.as_str().starts_with("godot/wang"))
            .map(|n| (n.code.as_str(), (n.message.as_str(), n.elements.as_slice())))
            .collect();
        // The note names the terrain sets that lack a tile (the ids, many at times, are cut short).
        let several = "tileset w: tile 0 is in more than one Wang set, and a Godot tile is in one terrain set: it's in the first one's alone, so the terrain set of Edges lacks it";
        let flipped = "tileset w: 1 flipped cell of Wang set Corners has other colours than its tile, and Godot's terrains don't flip: its terrain painting matches the cell by the tile's own colours";
        let expected = BTreeMap::from([
            (
                "godot/wang-several-sets",
                (several, &[Element::Tile { tileset: 0, id: 0 }][..]),
            ),
            ("godot/wang-flipped", (flipped, &[Element::Tileset(0)][..])),
        ]);
        assert_eq!(
            notes, expected,
            "tile 3 is the same flipped: no note for it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Tiles in several Wang sets: one note per tileset naming the sets whose terrain sets lack them,
    /// with the tiles (a pack's grass sets share hundreds of tiles, and the note listed each one).
    #[test]
    fn tiles_in_several_wang_sets_name_the_terrain_sets_that_lack_them() {
        let dir = project("terrain-several");
        // Edges also has tiles 1 and 3 of Corners, Mixed tiles 0 and 3.
        let tsx = W_TSX
        .replace(r#"   <wangtile tileid="4" wangid="1,0,2,0,1,0,2,0"/>"#, "   <wangtile tileid=\"4\" wangid=\"1,0,2,0,1,0,2,0\"/>\n   <wangtile tileid=\"1\" wangid=\"1,0,1,0,1,0,1,0\"/>\n   <wangtile tileid=\"3\" wangid=\"2,0,2,0,2,0,2,0\"/>")
        .replace(r#"   <wangtile tileid="5" wangid="1,2,3,1,2,3,1,2"/>"#, "   <wangtile tileid=\"5\" wangid=\"1,2,3,1,2,3,1,2\"/>\n   <wangtile tileid=\"0\" wangid=\"1,1,1,1,1,1,1,1\"/>\n   <wangtile tileid=\"3\" wangid=\"2,2,2,2,2,2,2,2\"/>");
        std::fs::write(dir.join("maps/w.tsx"), tsx).unwrap();
        std::fs::write(dir.join("maps/w.tmx"), w_map(ORTHO)).unwrap();
        let report = export(
            &dir.join("maps/w.tmx"),
            &mut loader(&dir),
            &dir.join("w.tscn"),
            &Options::default(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let notes: Vec<(&str, &[Element])> = report
            .notes
            .iter()
            .filter(|n| n.code == "godot/wang-several-sets")
            .map(|n| (n.message.as_str(), n.elements.as_slice()))
            .collect();
        let message = "tileset w: 3 tiles are in more than one Wang set, and a Godot tile is in one terrain set: each is in the first one's alone, so the terrain sets of Edges and Mixed lack some of them (0–1, 3)";
        assert_eq!(
            notes,
            [(
                message,
                &[
                    Element::Tile { tileset: 0, id: 0 },
                    Element::Tile { tileset: 0, id: 1 },
                    Element::Tile { tileset: 0, id: 3 }
                ][..]
            )]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hexagonal_maps_get_terrain_sets_of_sides() {
        let (dir, report, tscn) = w_export("terrain-hex", HEX);
        let modes: Vec<(String, String)> = lines(&tscn, "TileSet")
            .into_iter()
            .filter(|(k, _)| k.ends_with("/mode"))
            .collect();
        assert_eq!(
            modes,
            pairs(&[("terrain_set_0/mode", "2"), ("terrain_set_1/mode", "2")]),
            "the edge and mixed sets; the corner set is left out"
        );
        assert_eq!(report.counts.terrain_sets, 2);
        let base = lines(&tscn, "maps/w.tsx");
        let sides = |set: &str, values: &[(&str, &str)]| {
            let mut expected = pairs(&[("terrain_set", set), ("terrain", "0")]);
            expected.extend(
                values
                    .iter()
                    .map(|(k, v)| (format!("terrains_peering_bit/{k}"), v.to_string())),
            );
            expected
        };
        let edge = [
            ("bottom_right_side", "0"),
            ("bottom_left_side", "0"),
            ("top_left_side", "0"),
            ("top_right_side", "0"),
        ];
        assert_eq!(
            terrain(&base, "0:0/0/"),
            sides("0", &edge),
            "tile 0 is in the edge set: the corner set has none"
        );
        assert_eq!(
            terrain(&base, "1:0/0/"),
            [],
            "tile 1 is in the corner set alone"
        );
        let mixed = [
            ("right_side", "1"),
            ("bottom_right_side", "2"),
            ("bottom_left_side", "1"),
            ("left_side", "2"),
            ("top_left_side", "0"),
            ("top_right_side", "0"),
        ];
        assert_eq!(
            terrain(&base, "1:1/0/"),
            sides("1", &mixed),
            "tile 5, without the colours two rows away"
        );
        assert_eq!(
            assert_bits_valid(&tscn, Orientation::Hexagonal, StaggerAxis::Y),
            4 + 4 + 6 + 6,
            "tiles 0, 4, 5 and 8"
        );

        let notes: Vec<(&str, &str)> = report
            .notes
            .iter()
            .filter(|n| n.code.as_str().starts_with("godot/wang"))
            .map(|n| (n.code.as_str(), n.message.as_str()))
            .collect();
        let expected = [
            (
                "godot/wang-hex-corners",
                "tileset w: Wang set Corners is left out: Tiled's corners on a hexagonal map aren't a hexagon's, so Godot has no terrain set for them",
            ),
            (
                "godot/wang-hex-sides",
                "tileset w: Wang set Edges becomes a terrain set of a cell's 6 sides, as Godot's hexagonal terrains are, and Tiled's edges are 4 of them: its tiles have no terrain on the left and right sides",
            ),
            (
                "godot/wang-hex-sides",
                "tileset w: Wang set Mixed becomes a terrain set of a cell's 6 sides, as Godot's hexagonal terrains are: its colours toward the cells two rows away, which Tiled matches too, are left out",
            ),
        ];
        assert_eq!(notes, expected);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A corner set on p.tsx; an edge set on q.tsx, with an animated tile.
    const P_TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="p" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <wangsets>
  <wangset name="P" type="corner" tile="-1">
   <wangcolor name="a" color="#ff0000" tile="-1" probability="1"/>
   <wangcolor name="b" color="#00ff00" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/>
   <wangtile tileid="1" wangid="0,2,0,2,0,2,0,2"/>
  </wangset>
 </wangsets>
</tileset>
"##;

    const Q_TSX: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="q" tilewidth="16" tileheight="16" tilecount="16" columns="4">
 <image source="tiles.png" width="64" height="64"/>
 <tile id="1">
  <animation>
   <frame tileid="1" duration="100"/>
   <frame tileid="2" duration="100"/>
  </animation>
 </tile>
 <wangsets>
  <wangset name="Q" type="edge" tile="-1">
   <wangcolor name="c" color="#0000ff" tile="-1" probability="1"/>
   <wangcolor name="d" color="#ffffff" tile="-1" probability="1"/>
   <wangtile tileid="0" wangid="1,0,1,0,1,0,1,0"/>
   <wangtile tileid="1" wangid="2,0,2,0,2,0,2,0"/>
  </wangset>
 </wangsets>
</tileset>
"##;

    fn shared_map(tilesets: &[(u32, &str)], cells: &str) -> String {
        let tilesets: Vec<String> = tilesets
            .iter()
            .map(|(gid, file)| format!(r#" <tileset firstgid="{gid}" source="{file}"/>"#))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="orthogonal" renderorder="right-down" width="4" height="1" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="1">
{}
 <layer id="1" name="ground" width="4" height="1">
  <data encoding="csv">
{cells}
</data>
 </layer>
</map>
"#,
            tilesets.join("\n")
        )
    }

    #[test]
    fn a_shared_tilesets_terrain_sets_follow_its_registry() {
        let dir = project("terrain-shared");
        std::fs::write(
            dir.join("better-tiled.json"),
            r#"{"betterTiled": 1, "godot": {"project": "."}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("maps/p.tsx"), P_TSX).unwrap();
        std::fs::write(dir.join("maps/q.tsx"), Q_TSX).unwrap();
        std::fs::write(
            dir.join("maps/m1.tmx"),
            shared_map(&[(1, "q.tsx")], "1,2,0,0"),
        )
        .unwrap();
        std::fs::write(
            dir.join("maps/m2.tmx"),
            shared_map(&[(1, "p.tsx"), (17, "q.tsx")], "1,2,17,18"),
        )
        .unwrap();
        std::fs::write(
            dir.join("maps/m3.tmx"),
            shared_map(&[(1, "p.tsx")], "1,0,0,0"),
        )
        .unwrap();
        let tres = dir.join("tilesets/ortho-16x16.tres");
        let run = |n: usize| {
            export(
                &dir.join(format!("maps/m{n}.tmx")),
                &mut loader(&dir),
                &dir.join(format!("levels/m{n}.tscn")),
                &Options::default(),
            )
            .unwrap_or_else(|e| panic!("m{n}: {e}"))
        };
        let set = |text: &str, name: &str, tile: &str| {
            terrain(&lines(text, name), &format!("{tile}/0/"))
                .into_iter()
                .find(|(k, _)| k == "terrain_set")
                .map(|(_, v)| v)
        };
        run(1);
        let report = run(2);
        assert_eq!(report.counts.terrain_sets, 2);
        let text = std::fs::read_to_string(&tres).unwrap();
        let modes: Vec<(String, String)> = lines(&text, "TileSet")
            .into_iter()
            .filter(|(k, _)| k.ends_with("/mode"))
            .collect();
        assert_eq!(
            modes,
            pairs(&[("terrain_set_0/mode", "2"), ("terrain_set_1/mode", "1")]),
            "q's first: m1 registered it, though m2 has p first"
        );
        assert_eq!(set(&text, "maps/p.tsx", "0:0").as_deref(), Some("1"));
        assert_eq!(set(&text, "maps/q.tsx", "0:0").as_deref(), Some("0"));
        assert_eq!(
            (
                set(&text, "maps/q.tsx", "1:0"),
                set(&text, "maps/q.tsx#anim1", "1:0").as_deref()
            ),
            (None, Some("0")),
            "the animated tile's terrain is where its cells are"
        );
        assert_bits_valid(&text, Orientation::Orthogonal, StaggerAxis::Y);
        for n in [1, 3] {
            run(n);
            assert_eq!(
                std::fs::read_to_string(&tres).unwrap(),
                text,
                "the same from m{n}, which has the other tileset from the file"
            );
        }

        // q's file gone: m3 keeps its sources, in its terrain set, as they were.
        std::fs::rename(dir.join("maps/q.tsx"), dir.join("maps/q.away")).unwrap();
        let report = run(3);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.code == "godot/shared-tileset-missing"),
            "{:?}",
            report.notes
        );
        assert_eq!(report.counts.terrain_sets, 2);
        assert_eq!(
            std::fs::read_to_string(&tres).unwrap(),
            text,
            "the same file"
        );
        std::fs::rename(dir.join("maps/q.away"), dir.join("maps/q.tsx")).unwrap();

        // q no longer animating tile 1: the animation source kept for the scenes made before is in
        // no terrain set, and the tile itself is.
        let still = " <tile id=\"1\">\n  <animation>\n   <frame tileid=\"1\" duration=\"100\"/>\n   <frame tileid=\"2\" duration=\"100\"/>\n  </animation>\n </tile>\n";
        assert!(Q_TSX.contains(still));
        std::fs::write(dir.join("maps/q.tsx"), Q_TSX.replace(still, "")).unwrap();
        let report = run(2);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.code == "godot/shared-source-kept"),
            "{:?}",
            report.notes
        );
        let text = std::fs::read_to_string(&tres).unwrap();
        let kept = lines(&text, "maps/q.tsx#anim1");
        assert!(
            kept.iter().any(|(k, _)| k == "1:0/0"),
            "the tile is kept: {kept:?}"
        );
        assert_eq!(terrain(&kept, "1:0/0/"), []);
        assert_eq!(set(&text, "maps/q.tsx", "1:0").as_deref(), Some("0"));
        assert_bits_valid(&text, Orientation::Orthogonal, StaggerAxis::Y);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The cell across Wang index `lane` of `p`, as Tiled's terrain tools have it (bt-terrain's
    /// topology): the 8 cells around on square and isometric grids; on staggered and hexagonal maps
    /// a Wang tile is a diamond whose top faces the top-right neighbour.
    fn around(map: &Map, p: IVec2, lane: usize) -> IVec2 {
        const AROUND: [(i32, i32); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        if !matches!(
            map.orientation,
            Orientation::Staggered | Orientation::Hexagonal
        ) {
            return p + IVec2::from(AROUND[lane]);
        }
        let g = map.geometry();
        let x = map.stagger_axis == StaggerAxis::X;
        match lane {
            0 => g.top_right(p),
            2 => g.bottom_right(p),
            4 => g.bottom_left(p),
            6 => g.top_left(p),
            1 => {
                p + if x {
                    IVec2::new(2, 0)
                } else {
                    IVec2::new(1, 0)
                }
            }
            3 => {
                p + if x {
                    IVec2::new(0, 1)
                } else {
                    IVec2::new(0, 2)
                }
            }
            5 => {
                p + if x {
                    IVec2::new(-2, 0)
                } else {
                    IVec2::new(-1, 0)
                }
            }
            _ => {
                p + if x {
                    IVec2::new(0, -1)
                } else {
                    IVec2::new(0, -2)
                }
            }
        }
    }

    /// Tiled's own count of disagreements among `cells` (a Wang set's cells, their colours as
    /// placed): for each colour a cell has at an index of `lanes`, each cell sharing that side or
    /// corner whose colour there differs. With `sides` (Godot's hexagonal sets) every index is a side
    /// toward one cell.
    fn tiled_mismatches(
        map: &Map,
        cells: &HashMap<IVec2, WangId>,
        lanes: &[usize],
        sides: bool,
    ) -> usize {
        let mut bad = 0;
        for (&p, w) in cells {
            for &i in lanes.iter().filter(|&&i| w.lanes()[i] != 0) {
                let mut across = vec![(around(map, p, i), (i + 4) % 8)];
                if i % 2 == 1 && !sides {
                    across.push((around(map, p, (i + 1) % 8), (i + 6) % 8));
                    across.push((around(map, p, (i + 7) % 8), (i + 2) % 8));
                }
                bad += across
                    .into_iter()
                    .filter(|(q, j)| cells.get(q).is_some_and(|o| o.lanes()[*j] != w.lanes()[i]))
                    .count();
            }
        }
        bad
    }

    /// What `MISMATCHES` counts, by Godot's neighbours and the peering bits it loaded.
    const MISMATCH_FN: &str = r#"
# For each peering bit a cell has, each neighbour in its terrain set sharing that side (or, for a
# corner, the 3 cells around it) whose bit there differs.
func mismatches(layer: TileMapLayer) -> int:
	var bad := 0
	for c: Vector2i in layer.get_used_cells():
		var td := layer.get_cell_tile_data(c)
		if td == null or td.terrain_set < 0:
			continue
		for b in 16:
			if not td.is_valid_terrain_peering_bit(b):
				continue
			var mine := td.get_terrain_peering_bit(b)
			if mine == -1:
				continue
			var others := [[layer.get_neighbor_cell(c, b), (b + 8) % 16]]
			if b % 2 == 1:
				others.append([layer.get_neighbor_cell(c, (b + 1) % 16), (b + 12) % 16])
				others.append([layer.get_neighbor_cell(c, (b + 13) % 16), (b + 4) % 16])
			for o in others:
				var tn := layer.get_cell_tile_data(o[0])
				if tn != null and tn.terrain_set == td.terrain_set and tn.get_terrain_peering_bit(o[1]) != mine:
					bad += 1
	return bad
"#;

    /// Loads each scene given and counts, per tile layer, its cells' mismatches.
    const MISMATCHES: &str = r#"extends SceneTree

func _init() -> void:
	var out := {}
	for path in OS.get_cmdline_user_args():
		var root := (load(path) as PackedScene).instantiate()
		var layers := {}
		for node: Node in root.find_children("*", "TileMapLayer", true, false):
			layers[String(node.name)] = mismatches(node as TileMapLayer)
		out[path] = layers
		root.free()
	print("BT-JSON-BEGIN", JSON.stringify(out), "BT-JSON-END")
	quit()
"#;

    /// Imports the project and runs `script` with `args`: its JSON and Godot's stderr. Godot is
    /// stopped after 2 minutes (a script error leaves it running).
    fn godot_json(godot: &Path, dir: &Path, script: &str, args: &[String]) -> (Json, String) {
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
        std::fs::write(dir.join("terrain.gd"), script).unwrap();
        let args: Vec<&str> = ["--script", "res://terrain.gd", "--"]
            .into_iter()
            .chain(args.iter().map(String::as_str))
            .collect();
        let (stdout, stderr) = run_godot(godot, dir, &args);
        (json_between_markers(&stdout, &stderr), stderr)
    }

    /// The Wang cells of each tile layer of `map`, by layer name, with the lanes of their set: what
    /// bt-terrain sees in them (flips applied).
    fn wang_layers(map: &Map) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        let mut terrains: HashMap<TilesetSlot, bt_terrain::Terrain> = HashMap::new();
        for layer in &map.layers {
            let LayerKind::Tile(tiles) = &layer.kind else {
                continue;
            };
            let mut cells = HashMap::new();
            let mut lanes = Vec::new();
            for placed in tiles.cells() {
                let (p, cell) = (placed.at, placed.cell);
                let Some(slot) = cell.slot() else { continue };
                let ts = &map.tilesets[slot.index()].tileset;
                assert!(ts.wang_sets.len() <= 1, "one Wang set per tileset");
                let Some(set) = ts
                    .wang_sets
                    .first()
                    .filter(|s| s.tiles.contains_key(&cell.tile()))
                else {
                    continue;
                };
                let (_, bits) = mapping(map.orientation, map.stagger_axis, set.kind).unwrap();
                lanes = (0..8).filter(|&i| bits[i].is_some()).collect();
                let terrain = terrains
                    .entry(slot)
                    .or_insert_with(|| bt_terrain::Terrain::new(map, slot, 0).unwrap());
                cells.insert(p, terrain.wang_of_cell(cell));
            }
            out.insert(
                layer.name.clone(),
                tiled_mismatches(
                    map,
                    &cells,
                    &lanes,
                    map.orientation == Orientation::Hexagonal,
                ),
            );
        }
        out
    }

    /// On Tiled's examples, Godot finds as many neighbours whose terrains disagree as Tiled does:
    /// none where Tiled's terrain tools painted them, and the few the hand-painted isometric maps have.
    #[test]
    fn godot_counts_the_examples_terrain_mismatches_as_tiled_does() {
        let Some(godot) = godot() else { return };
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let dir = project("terrain-examples");
        let maps = [
            ("desert", 0),
            ("orthogonal-outside", 0),
            ("isometric_grass_and_water", 6),
            ("isometric_staggered_grass_and_water", 428),
        ];
        let mut tiled = BTreeMap::new();
        let mut scenes = Vec::new();
        for (name, _) in maps {
            let path = examples.join(format!("{name}.tmx"));
            let target = dir.join(format!("levels/{name}.tscn"));
            export(
                &path,
                &mut bt_tmx::Loader::builder().build(),
                &target,
                &Options::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            let map = bt_tmx::Loader::builder()
                .build()
                .load_map(&path)
                .unwrap()
                .value;
            let text = std::fs::read_to_string(&target).unwrap();
            assert!(
                assert_bits_valid(&text, map.orientation, map.stagger_axis) > 0,
                "{name}"
            );
            tiled.insert(format!("res://levels/{name}.tscn"), wang_layers(&map));
            scenes.push(format!("res://levels/{name}.tscn"));
        }
        let (found, stderr) =
            godot_json(&godot, &dir, &format!("{MISMATCHES}{MISMATCH_FN}"), &scenes);
        assert_eq!(stderr, "", "Godot loads them cleanly");
        for (name, expected) in maps {
            let scene = format!("res://levels/{name}.tscn");
            let godot: BTreeMap<String, usize> = found[&scene]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_u64().unwrap() as usize))
                .collect();
            assert_eq!(
                godot, tiled[&scene],
                "{name}: Godot's count per layer, and Tiled's"
            );
            assert_eq!(godot.values().sum::<usize>(), expected, "{name}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A tileset of complete two-colour sets on `size` tiles: the 16 corner tiles (0–15), the 16
    /// edge tiles (16–31) and the 256 mixed ones (32–287), each tile's colours the bits of its number.
    fn complete(size: IVec2) -> (String, BTreeMap<u32, WangId>) {
        let mut wang = BTreeMap::new();
        let mut sets = String::new();
        for (name, kind, first, lanes) in [
            ("Corners", "corner", 0, vec![1, 3, 5, 7]),
            ("Edges", "edge", 16, vec![0, 2, 4, 6]),
            ("Mixed", "mixed", 32, (0..8).collect()),
        ] {
            sets += &format!(
                "  <wangset name=\"{name}\" type=\"{kind}\" tile=\"-1\">\n   <wangcolor name=\"Grass\" color=\"#00ff00\" tile=\"-1\" probability=\"1\"/>\n   <wangcolor name=\"Water\" color=\"#0000ff\" tile=\"-1\" probability=\"1\"/>\n"
            );
            for k in 0..1u32 << lanes.len() {
                let mut id = [0; 8];
                for (bit, &lane) in lanes.iter().enumerate() {
                    id[lane] = 1 + (k >> bit & 1) as u8;
                }
                sets += &format!(
                    "   <wangtile tileid=\"{}\" wangid=\"{}\"/>\n",
                    first + k,
                    WangId::from_lanes(id).to_tiled_text()
                );
                wang.insert(first + k, WangId::from_lanes(id));
            }
            sets += "  </wangset>\n";
        }
        let tsx = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tileset version=\"1.10\" tiledversion=\"1.12.2\" name=\"complete\" tilewidth=\"{0}\" tileheight=\"{1}\" tilecount=\"288\" columns=\"16\">\n <image source=\"complete.png\" width=\"{2}\" height=\"{3}\"/>\n <wangsets>\n{sets} </wangsets>\n</tileset>\n",
            size.x,
            size.y,
            size.x * 16,
            size.y * 18
        );
        (tsx, wang)
    }

    /// Paints each terrain set of each scene's TileSet with `set_cells_terrain_connect`: on a 16×16
    /// field of one terrain's tile, either cells 5 apart or a blob of the other terrain, each way
    /// round. Gives each painting's cells (position, atlas coordinates and alternative) and Godot's
    /// count of mismatches among them, and how many peering bits it set again on the TileSet's tiles.
    const PAINT: &str = r#"extends SceneTree

# The tile of terrain set `s` whose every bit (that it has) is terrain `t`: its source and atlas
# coordinates.
func uniform(ts: TileSet, s: int, t: int) -> Array:
	for i in ts.get_source_count():
		var id := ts.get_source_id(i)
		var source := ts.get_source(id) as TileSetAtlasSource
		for n in source.get_tiles_count():
			var at := source.get_tile_id(n)
			var td := source.get_tile_data(at, 0)
			if td.terrain_set != s:
				continue
			var all := true
			for b in 16:
				if td.is_valid_terrain_peering_bit(b) and td.get_terrain_peering_bit(b) not in [-1, t]:
					all = false
			if all and td.terrain == t:
				return [id, at]
	return []

func _init() -> void:
	var out := {}
	for path in OS.get_cmdline_user_args():
		var root := (load(path) as PackedScene).instantiate()
		var ts := (root.find_children("*", "TileMapLayer", true, false)[0] as TileMapLayer).tile_set
		var runs := []
		for s in ts.get_terrain_sets_count():
			for shape in ["cells", "blob"]:
				for into in 2:
					var layer := TileMapLayer.new()
					layer.tile_set = ts
					var ground := uniform(ts, s, 1 - into)
					var painted: Array[Vector2i] = []
					for y in 16:
						for x in 16:
							layer.set_cell(Vector2i(x, y), ground[0], ground[1])
							var blob := (x - 8) * (x - 8) + (y - 8) * (y - 8) <= 8
							var single := x % 5 == 2 and y % 5 == 2
							if blob if shape == "blob" else single:
								painted.append(Vector2i(x, y))
					layer.set_cells_terrain_connect(painted, s, into)
					var cells := []
					for c: Vector2i in layer.get_used_cells():
						var a := layer.get_cell_atlas_coords(c)
						cells.append([c.x, c.y, a.x, a.y, layer.get_cell_alternative_tile(c)])
					runs.append({"set": s, "mode": ts.get_terrain_set_mode(s), "shape": shape, "into": into, "cells": cells, "bad": mismatches(layer)})
					layer.free()
		# Each peering bit set again, now that the tiles are in their TileSet.
		var again := 0
		for i in ts.get_source_count():
			var source := ts.get_source(ts.get_source_id(i)) as TileSetAtlasSource
			for t in source.get_tiles_count():
				var td := source.get_tile_data(source.get_tile_id(t), 0)
				if td.terrain_set < 0:
					continue
				for b in 16:
					if td.is_valid_terrain_peering_bit(b) and td.get_terrain_peering_bit(b) != -1:
						td.set_terrain_peering_bit(b, td.get_terrain_peering_bit(b))
						again += 1
		out[path] = {"runs": runs, "again": again}
		root.free()
	print("BT-JSON-BEGIN", JSON.stringify(out), "BT-JSON-END")
	quit()
"#;

    /// Complete two-colour corner, edge and mixed sets, painted by Godot's own terrain painting on
    /// every kind of grid: edge and mixed sets come out seamless, and whatever Godot paints has as
    /// many seams by Tiled's neighbours as by Godot's.
    #[test]
    fn godot_paints_complete_sets_seamlessly() {
        let Some(godot) = godot() else { return };
        let dir = project("terrain-paint");
        let grids = [
            (
                "ortho",
                r#"orientation="orthogonal" tilewidth="8" tileheight="8""#,
                IVec2::new(8, 8),
            ),
            (
                "iso",
                r#"orientation="isometric" tilewidth="16" tileheight="8""#,
                IVec2::new(16, 8),
            ),
            (
                "stag-y",
                r#"orientation="staggered" tilewidth="16" tileheight="8" staggeraxis="y" staggerindex="odd""#,
                IVec2::new(16, 8),
            ),
            (
                "stag-x",
                r#"orientation="staggered" tilewidth="16" tileheight="8" staggeraxis="x" staggerindex="even""#,
                IVec2::new(16, 8),
            ),
            ("hex-y", HEX, IVec2::new(14, 12)),
            (
                "hex-x",
                r#"orientation="hexagonal" tilewidth="12" tileheight="14" hexsidelength="6" staggeraxis="x" staggerindex="even""#,
                IVec2::new(12, 14),
            ),
        ];
        let mut scenes = Vec::new();
        let mut by_scene = HashMap::new();
        for (name, grid, size) in grids {
            let folder = dir.join(format!("maps/{name}"));
            std::fs::create_dir_all(&folder).unwrap();
            let (tsx, wang) = complete(size);
            std::fs::write(folder.join("complete.tsx"), tsx).unwrap();
            image::RgbaImage::from_pixel(
                size.x as u32 * 16,
                size.y as u32 * 18,
                image::Rgba([90, 160, 60, 255]),
            )
            .save(folder.join("complete.png"))
            .unwrap();
            let map = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<map version=\"1.10\" tiledversion=\"1.12.2\" {grid} renderorder=\"right-down\" width=\"2\" height=\"1\" infinite=\"0\" nextlayerid=\"2\" nextobjectid=\"1\">\n <tileset firstgid=\"1\" source=\"complete.tsx\"/>\n <layer id=\"1\" name=\"ground\" width=\"2\" height=\"1\">\n  <data encoding=\"csv\">\n1,0\n</data>\n </layer>\n</map>\n"
            );
            std::fs::write(folder.join("complete.tmx"), map).unwrap();
            let target = dir.join(format!("levels/{name}.tscn"));
            let report = export(
                &folder.join("complete.tmx"),
                &mut loader(&dir),
                &target,
                &Options::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
            let map = bt_tmx::Loader::builder()
                .build()
                .load_map(&folder.join("complete.tmx"))
                .unwrap()
                .value;
            assert_eq!(
                report.counts.terrain_sets,
                if map.orientation == Orientation::Hexagonal {
                    2
                } else {
                    3
                },
                "{name}"
            );
            assert_bits_valid(
                &std::fs::read_to_string(&target).unwrap(),
                map.orientation,
                map.stagger_axis,
            );
            let scene = format!("res://levels/{name}.tscn");
            scenes.push(scene.clone());
            by_scene.insert(scene, (map, wang));
        }
        let (found, stderr) = godot_json(&godot, &dir, &format!("{PAINT}{MISMATCH_FN}"), &scenes);
        assert_eq!(
            stderr, "",
            "Godot loads, paints and sets peering bits without a message"
        );
        for scene in &scenes {
            let (map, wang) = &by_scene[scene];
            let result = &found[scene];
            assert!(
                result["again"].as_u64().unwrap() > 0,
                "{scene}: peering bits set again"
            );
            let kinds: Vec<WangSetType> = map.tilesets[0]
                .tileset
                .wang_sets
                .iter()
                .map(|s| s.kind)
                .filter(|&k| mapping(map.orientation, map.stagger_axis, k).is_some())
                .collect();
            let runs = result["runs"].as_array().unwrap();
            assert_eq!(runs.len(), kinds.len() * 4, "{scene}");
            for run in runs {
                let kind = kinds[run["set"].as_u64().unwrap() as usize];
                let into = run["into"].as_u64().unwrap() as u8;
                let what = format!(
                    "{scene} {kind:?} {} of colour {}",
                    run["shape"].as_str().unwrap(),
                    into + 1
                );
                let (mode, bits) = mapping(map.orientation, map.stagger_axis, kind).unwrap();
                assert_eq!(run["mode"].as_u64(), Some(u64::from(mode)), "{what}");
                let mut cells = HashMap::new();
                for c in run["cells"].as_array().unwrap() {
                    let n = |i: usize| c[i].as_i64().unwrap() as i32;
                    assert_eq!(n(4), 0, "{what}: a tile itself");
                    cells.insert(IVec2::new(n(0), n(1)), wang[&((n(3) * 16 + n(2)) as u32)]);
                }
                assert_eq!(cells.len(), 16 * 16, "{what}");
                let lanes: Vec<usize> = (0..8).filter(|&i| bits[i].is_some()).collect();
                let painted = cells
                    .values()
                    .filter(|w| lanes.iter().any(|&i| w.lanes()[i] == into + 1))
                    .count();
                assert!(painted >= 9, "{what}: {painted} cells painted");
                let godot = run["bad"].as_u64().unwrap() as usize;
                assert_eq!(
                    godot,
                    tiled_mismatches(
                        map,
                        &cells,
                        &lanes,
                        map.orientation == Orientation::Hexagonal
                    ),
                    "{what}: Godot's mismatches are Tiled's"
                );
                // Godot paints a corner set by the cells' own terrains: a corner takes one only
                // where every cell around it has it, and a cell next to those painted keeps its own,
                // which one terrain per tile can't always give. Single cells of the second colour are
                // what the most common colour suits.
                let suited = run["shape"] == "cells" && into == 1;
                if kind != WangSetType::Corner || suited {
                    assert_eq!(godot, 0, "{what}: seamless");
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
