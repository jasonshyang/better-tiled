//! The autotile layouts: their labels against Tiled's own sets, detection on pictures drawn in
//! code, on Tiled's examples and on the purchased packs (every block they draw as a layout and
//! nothing else), and Label All, which gives blocks that look alike the same terrains.

#![cfg(test)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use bt_core::{TileId, Tileset, TilesetImage, WangId, WangSetType};
use bt_image::DecodedImage;
use bt_terrain::TerrainColor;
use bt_terrain::layouts::{Fit, KnownTerrains, LAYOUTS, Layout, Samples, detect, places};
use glam::UVec2;

use common::pictures::*;

fn layout(name: &str) -> (usize, &'static Layout) {
    LAYOUTS
        .iter()
        .enumerate()
        .find(|(_, l)| l.name == name)
        .unwrap_or_else(|| panic!("no layout {name}"))
}

/// A tileset of the reference corpus or the packs, with its image decoded.
fn load(relative: &str) -> Option<(std::sync::Arc<Tileset>, DecodedImage)> {
    let path = bt_test_data::test_data(relative)?;
    Some(load_path(&path))
}

fn load_path(path: &Path) -> (std::sync::Arc<Tileset>, DecodedImage) {
    let tileset = bt_tmx::Loader::default().load_tileset(path).unwrap().value;
    let TilesetImage::Atlas { image, .. } = &tileset.image else {
        panic!("{} is not an atlas", path.display());
    };
    let decoded = bt_image::decode_file(image.path().unwrap(), None).unwrap();
    (tileset, decoded)
}

/// A fit as the tables below write it.
fn described(fit: &Fit) -> String {
    let nothing = if fit.over_nothing {
        " over nothing"
    } else {
        ""
    };
    format!(
        "{} at {},{}{nothing}",
        LAYOUTS[fit.layout].name, fit.origin.x, fit.origin.y
    )
}

fn labels_of(
    layout: &Layout,
    origin: UVec2,
    columns: u32,
    terrain: u8,
    under: u8,
) -> BTreeMap<TileId, WangId> {
    layout
        .labels(origin, columns, TerrainColor(terrain), TerrainColor(under))
        .into_iter()
        .filter_map(|(tile, label)| Some((tile, label?)))
        .collect()
}

#[test]
fn the_blob_layout_gives_tileds_blob_labels() {
    let Some((tileset, image)) = load("reference/tiled/tests/wangtiles/wangblob.tsx") else {
        return;
    };
    let (_, blob) = layout("Blob 7×7");
    for (set, terrain, under) in [
        ("Blob", 1, 2),
        ("Blob (only Yellow)", 1, 0),
        ("Blob (only Blue)", 0, 1),
    ] {
        let theirs = &tileset
            .wang_sets
            .iter()
            .find(|s| s.name == set)
            .unwrap()
            .tiles;
        assert_eq!(
            &labels_of(blob, UVec2::ZERO, 7, terrain, under),
            theirs,
            "{set}"
        );
    }
    // And detection finds it, the yellow blob over the blue ground.
    let found = detect(&Samples::from_tileset(&tileset, &image));
    let found: Vec<String> = found.iter().map(described).collect();
    assert_eq!(found, ["Blob 7×7 at 0,0"]);
}

/// The cross islands give the labels the Ancient Ruins pack gives its own blocks: light grass
/// over mid-tone grass with the diagonal tiles, paving stones over nothing without them.
#[test]
fn the_cross_islands_give_the_packs_own_labels() {
    let Some(path) = bt_test_data::test_data(
        "assets/ERW - Ancient Ruins V 2.3/TiledMap Editor/Tilesets/Terrain - Ancient Ruins.tsx",
    ) else {
        return;
    };
    let tileset = bt_tmx::Loader::default().load_tileset(&path).unwrap().value;
    let set = |name: &str| {
        &tileset
            .wang_sets
            .iter()
            .find(|s| s.name == name)
            .unwrap()
            .tiles
    };
    let (_, diagonals) = layout("5×5 cross island, diagonals");
    let (_, plain) = layout("5×5 cross island");
    for (l, name, origin) in [
        (diagonals, "light grass to mid tone grass", UVec2::new(0, 2)),
        (diagonals, "light grass to mid tone grass", UVec2::new(0, 7)),
        (plain, "stone ground2 to transparency", UVec2::new(26, 37)),
    ] {
        let theirs = set(name);
        let ours = labels_of(l, origin, 47, 1, 0);
        assert_eq!(ours.len(), l.tile_count(), "{name}: every tile is labelled");
        for (tile, label) in ours {
            assert_eq!(theirs.get(&tile), Some(&label), "{name}: tile {}", tile.0);
        }
    }
}

/// Tiled's desert example is three 3×3 islands and three 2×2 holes of brick, cobblestone and
/// dirt on sand, in an image with spacing and a margin.
#[test]
fn the_desert_example_is_three_islands_and_three_holes() {
    let Some((tileset, image)) = load("reference/tiled/examples/desert.tsx") else {
        return;
    };
    let theirs = &tileset.wang_sets[0].tiles;
    let (_, island) = layout("3×3 island");
    let (_, hole) = layout("2×2 hole");
    let mut ours = BTreeMap::new();
    for (l, x, y, terrain) in [
        (island, 0, 0, 2),
        (island, 0, 3, 3),
        (island, 5, 0, 4),
        (hole, 3, 0, 4),
        (hole, 3, 2, 2),
        (hole, 3, 4, 3),
    ] {
        ours.extend(labels_of(l, UVec2::new(x, y), 8, terrain, 1));
    }
    for (tile, label) in &ours {
        assert_eq!(theirs.get(tile), Some(label), "tile {}", tile.0);
    }
    let sand = WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1]);
    assert!(
        theirs
            .iter()
            .all(|(tile, label)| ours.contains_key(tile) || *label == sand),
        "the rest is sand"
    );
    let mut found: Vec<String> = detect(&Samples::from_tileset(&tileset, &image))
        .iter()
        .map(described)
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "2×2 hole at 3,0",
            "2×2 hole at 3,2",
            "2×2 hole at 3,4",
            "3×3 island at 0,0",
            "3×3 island at 0,3",
            "3×3 island at 5,0",
        ]
    );
}

/// Whether layout `b` is layout `a` with its terrain and the rest exchanged (between two opaque
/// terrains an island of one is a hole in the other: the same tiles).
fn inverse(a: &Layout, b: &Layout) -> bool {
    let mask: u8 = match a.kind {
        WangSetType::Corner => 0b1010_1010,
        WangSetType::Edge => 0b0101_0101,
        WangSetType::Mixed => 0xFF,
    };
    a.kind == b.kind
        && a.size() == b.size()
        && a.slots().iter().zip(b.slots()).all(|(x, y)| match (x, y) {
            (Some(x), Some(y)) => {
                y.terrain == !x.terrain & mask
                    && ((x.terrain != 0 && x.terrain != mask) || y.middle != x.middle)
            }
            (x, y) => x.is_none() && y.is_none(),
        })
}

#[test]
fn each_layout_is_found_where_it_is_drawn() {
    let grass = rgb(60, 170, 60);
    let dirt = rgb(150, 100, 50);
    for (index, l) in LAYOUTS.iter().enumerate() {
        let size = l.size();
        let (columns, rows) = (size.x + 3, size.y + 2);
        for (under, over_nothing) in [(Some(dirt), false), (None, true)] {
            let mut picture = blank(columns, rows, 16);
            draw_layout(&mut picture, l, UVec2::new(2, 1), 16, grass, under);
            let tileset = atlas_for(columns, rows, 16);
            let samples = Samples::from_tileset(&tileset, &picture);
            let found = detect(&samples);
            let best = found
                .first()
                .unwrap_or_else(|| panic!("{} over {under:?}: nothing found", l.name));
            let same = best.layout == index || (!over_nothing && inverse(l, &LAYOUTS[best.layout]));
            assert!(
                same && best.origin == UVec2::new(2, 1) && best.over_nothing == over_nothing,
                "{} over {under:?}: {found:?}",
                l.name
            );
            assert!(
                places(&samples, index)
                    .iter()
                    .any(|f| f.origin == UVec2::new(2, 1)),
                "{}: its own places",
                l.name
            );
        }
    }
}

#[test]
fn a_see_through_terrain_is_the_layout_the_other_way_around() {
    let (island, island_layout) = layout("3×3 island");
    let (hole, _) = layout("3×3 hole");
    // A see-through island in a ring of grass: a hole in the grass.
    let mut picture = blank(3, 3, 16);
    draw_layout(
        &mut picture,
        island_layout,
        UVec2::ZERO,
        16,
        [0, 0, 0, 0],
        Some(rgb(60, 170, 60)),
    );
    let samples = Samples::from_tileset(&atlas_for(3, 3, 16), &picture);
    assert!(places(&samples, island).is_empty());
    assert_eq!(
        detect(&samples),
        [Fit {
            layout: hole,
            origin: UVec2::ZERO,
            over_nothing: true
        }]
    );
}

#[test]
fn noise_and_empty_sheets_show_no_layout() {
    let (columns, rows, size) = (12, 8, 16);
    let tileset = atlas_for(columns, rows, size);
    let empty = blank(columns, rows, size);
    assert!(detect(&Samples::from_tileset(&tileset, &empty)).is_empty());
    // Every tile a colour of its own.
    let mut tiles = empty.clone();
    let mut seed = 12345_u32;
    for row in 0..rows {
        for column in 0..columns {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let color = rgb((seed >> 8) as u8, (seed >> 16) as u8, (seed >> 24) as u8);
            fill_tile(&mut tiles, column, row, size, color, |_, _| true);
        }
    }
    let found = detect(&Samples::from_tileset(&tileset, &tiles));
    assert!(found.is_empty(), "{found:?}");
}

/// A cross island raised over nothing, as the packs' walls and cliffs draw it: below its lower
/// edges, the island's own wall.
#[test]
fn a_raised_island_is_found_over_its_own_wall() {
    let (cross, cross_layout) = layout("5×5 cross island");
    let size = 16;
    let mut picture = blank(7, 7, size);
    draw_layout(
        &mut picture,
        cross_layout,
        UVec2::new(1, 1),
        size,
        rgb(110, 160, 60),
        None,
    );
    // The wall: grey stone in the lower half of the tiles under the island's lower edges.
    let wall = rgb(120, 110, 105);
    for (column, row) in [(1, 4), (5, 4), (2, 5), (3, 5), (4, 5)] {
        fill_tile(&mut picture, column, row, size, wall, |_, y| y >= size / 2);
    }
    let samples = Samples::from_tileset(&atlas_for(7, 7, size), &picture);
    assert_eq!(
        detect(&samples),
        [Fit {
            layout: cross,
            origin: UVec2::new(1, 1),
            over_nothing: true
        }]
    );
}

/// The packs draw their cross islands with or without the two diagonal tiles in the top
/// corners; an empty corner is not a diagonal tile.
#[test]
fn the_diagonal_tiles_tell_the_two_cross_islands_apart() {
    let (with, with_layout) = layout("5×5 cross island, diagonals");
    let (without, without_layout) = layout("5×5 cross island");
    let grass = rgb(60, 170, 60);
    let dirt = rgb(150, 100, 50);
    for (index, drawn) in [(with, with_layout), (without, without_layout)] {
        let mut picture = blank(5, 5, 16);
        draw_layout(&mut picture, drawn, UVec2::ZERO, 16, grass, Some(dirt));
        let samples = Samples::from_tileset(&atlas_for(5, 5, 16), &picture);
        let found = detect(&samples);
        assert_eq!(
            found.iter().map(|f| f.layout).collect::<Vec<_>>(),
            [index],
            "{}",
            drawn.name
        );
    }
}

/// A terrain drawn loosely over nothing (scattered stones) spills over a few of the other
/// side's lanes, and is still found.
#[test]
fn a_loosely_drawn_terrain_is_still_found() {
    let (cross, cross_layout) = layout("5×5 cross island, diagonals");
    let stone = rgb(200, 190, 140);
    let mut picture = blank(5, 5, 16);
    draw_layout(&mut picture, cross_layout, UVec2::ZERO, 16, stone, None);
    // Stones in the see-through top-left corners of five outer tiles: more than one lane in
    // thirty of the block's 92.
    for (column, row) in [(1, 0), (2, 0), (3, 0), (0, 1), (4, 1)] {
        fill_tile(&mut picture, column, row, 16, stone, |x, y| x < 4 && y < 4);
    }
    let samples = Samples::from_tileset(&atlas_for(5, 5, 16), &picture);
    assert_eq!(
        detect(&samples),
        [Fit {
            layout: cross,
            origin: UVec2::ZERO,
            over_nothing: true
        }]
    );
}

/// Label All on a sheet of four blocks: two grass islands on dirt, a hole in grass showing dirt,
/// and a water island on dirt. Alike blocks get alike terrains.
#[test]
fn label_all_groups_blocks_that_look_alike() {
    let (island, island_layout) = layout("3×3 island");
    let (hole, hole_layout) = layout("3×3 hole");
    let grass = rgb(60, 170, 60);
    let dirt = rgb(150, 100, 50);
    let water = rgb(40, 90, 220);
    let mut picture = blank(16, 3, 16);
    draw_layout(
        &mut picture,
        island_layout,
        UVec2::new(0, 0),
        16,
        grass,
        Some(dirt),
    );
    draw_layout(
        &mut picture,
        hole_layout,
        UVec2::new(4, 0),
        16,
        grass,
        Some(dirt),
    );
    draw_layout(
        &mut picture,
        island_layout,
        UVec2::new(8, 0),
        16,
        water,
        Some(dirt),
    );
    draw_layout(
        &mut picture,
        island_layout,
        UVec2::new(12, 0),
        16,
        grass,
        Some(dirt),
    );
    let tileset = atlas_for(16, 3, 16);
    let samples = Samples::from_tileset(&tileset, &picture);
    let fits = vec![
        Fit {
            layout: island,
            origin: UVec2::new(0, 0),
            over_nothing: false,
        },
        Fit {
            layout: hole,
            origin: UVec2::new(4, 0),
            over_nothing: false,
        },
        Fit {
            layout: island,
            origin: UVec2::new(8, 0),
            over_nothing: false,
        },
        Fit {
            layout: island,
            origin: UVec2::new(12, 0),
            over_nothing: false,
        },
    ];
    let mut known = KnownTerrains::default();
    let blocks = known.label_all(&samples, &fits);
    let terrains: Vec<[u8; 2]> = blocks.iter().map(|b| [b.terrain.0, b.under.0]).collect();
    assert_eq!(
        terrains,
        [[1, 2], [1, 2], [3, 2], [1, 2]],
        "grass, dirt, then water"
    );
    assert_eq!(known.terrain_count(), 3);
    // A set that already labels grass as its second terrain: grass blocks get it.
    let mut set = bt_core::WangSet {
        name: "Ground".to_owned(),
        class: String::new(),
        kind: WangSetType::Corner,
        tile: None,
        colors: vec![
            common::terrain_color("Dirt"),
            common::terrain_color("Grass"),
        ],
        tiles: Default::default(),
        properties: Default::default(),
    };
    // The grass island's middle tile, all grass.
    set.tiles
        .insert(TileId(16 + 1), WangId::from_lanes([0, 2, 0, 2, 0, 2, 0, 2]));
    let known = KnownTerrains::from_set(&samples, &set, &[]);
    let guessed = known.guess(&samples, &fits[0]);
    assert_eq!(
        (guessed.terrain, guessed.under),
        (TerrainColor(2), TerrainColor(1)),
        "grass is the set's second terrain; dirt takes the first, which no tile shows"
    );
}

/// The blocks of Tiled's PathAndObjects tileset, labelled all at once: wherever Tiled's own set
/// labels them the labels agree, and each of our terrains is one of Tiled's.
#[test]
fn label_all_agrees_with_tileds_path_and_objects() {
    let Some((tileset, image)) = load("reference/tiled/tests/wangtiles/PathAndObjects.tsx") else {
        return;
    };
    let samples = Samples::from_tileset(&tileset, &image);
    let fits: Vec<Fit> = detect(&samples)
        .into_iter()
        .filter(|f| LAYOUTS[f.layout].kind == WangSetType::Corner && !f.over_nothing)
        .collect();
    assert_eq!(fits.len(), 15, "8 islands and 7 holes: {fits:?}");
    let blocks = KnownTerrains::default().label_all(&samples, &fits);
    let columns = samples.size().x;
    let mut ours = BTreeMap::new();
    for block in &blocks {
        ours.extend(
            block
                .labels(columns)
                .into_iter()
                .filter_map(|(tile, label)| Some((tile, label?))),
        );
    }
    let theirs = &tileset.wang_sets[0].tiles;
    let mut names: BTreeMap<u8, u8> = BTreeMap::new();
    let mut compared = 0;
    for (tile, label) in &ours {
        let Some(their) = theirs.get(tile) else {
            continue;
        };
        compared += 1;
        for lane in [1, 3, 5, 7] {
            let (a, b) = (label.lanes()[lane], their.lanes()[lane]);
            assert_eq!(
                *names.entry(a).or_insert(b),
                b,
                "tile {}: {label:?} vs {their:?}",
                tile.0
            );
        }
    }
    assert_eq!(compared, 9 * 5 + 4 * 5, "the blocks Tiled labels");
    let distinct: BTreeSet<u8> = names.values().copied().collect();
    assert_eq!(distinct.len(), names.len(), "{names:?}");
}

/// A terrain tileset of the packs and the blocks detection finds in it.
struct PackTileset {
    /// The `.tsx` file's name.
    file: &'static str,
    /// What detection finds, as [`described`] writes it.
    found: &'static [&'static str],
    /// Blocks the pack draws (and its own labels show) that detection does not find.
    missed: &'static [Missed],
}

/// A block detection does not find, and why.
struct Missed {
    /// The block, as [`described`] writes it.
    block: &'static str,
    /// Why its colours do not show it.
    why: &'static str,
}

/// A raised island whose top is see-through, to be laid over a ground: only its rim shows.
const RIM_ONLY: &str = "its top is see-through, so only its rim shows";

/// A raised island whose top is two colours, a wide rim around the grass, darker than the grass
/// and close to its wall.
const TWO_TONE_TOP: &str = "its top is two colours, a wide rim around the grass";

/// An animated terrain's preview whose middle tiles are left see-through, every other one.
const CHECKERBOARD: &str = "its middle tiles are left see-through, every other one";

/// The opaque blocks of the Ancient Ruins terrain sheet, by the terrain over its ground.
const RUINS_GRASS: [&str; 12] = [
    "5×5 cross island, diagonals at 0,2",
    "5×5 cross island, diagonals at 0,7",
    "5×5 cross island, diagonals at 5,2",
    "5×5 cross island, diagonals at 5,7",
    "5×5 cross island, diagonals at 10,2",
    "5×5 cross island, diagonals at 10,10",
    "5×5 cross island, diagonals at 15,2",
    "5×5 cross island, diagonals at 15,7",
    "5×5 cross island, diagonals at 20,2",
    "5×5 cross island, diagonals at 20,7",
    "5×5 cross island, diagonals at 10,16",
    "5×5 cross island, diagonals at 10,21",
];

const PACKS: &[PackTileset] = &[
    PackTileset {
        file: "Terrain - Ancient Ruins.tsx",
        found: &[
            "5×5 cross island, diagonals at 0,2",
            "5×5 cross island, diagonals at 0,7",
            "5×5 cross island, diagonals at 5,2",
            "5×5 cross island, diagonals at 5,7",
            "5×5 cross island, diagonals at 10,2",
            "5×5 cross island, diagonals at 10,10",
            "5×5 cross island, diagonals at 15,2",
            "5×5 cross island, diagonals at 15,7",
            "5×5 cross island, diagonals at 20,2",
            "5×5 cross island, diagonals at 20,7",
            "5×5 cross island, diagonals at 10,16",
            "5×5 cross island, diagonals at 10,21",
            "5×5 cross island, diagonals at 26,2 over nothing",
            "5×5 cross island, diagonals at 26,9 over nothing",
            "5×5 cross island, diagonals at 31,2 over nothing",
            "5×5 cross island, diagonals at 31,9 over nothing",
            "5×5 cross island, diagonals at 36,2 over nothing",
            "5×5 cross island, diagonals at 36,9 over nothing",
            "5×5 cross island, diagonals at 41,2 over nothing",
            "5×5 cross island, diagonals at 41,9 over nothing",
            // Paving stones scattered over nothing, spilling over a few lanes.
            "5×5 cross island, diagonals at 26,15 over nothing",
            "5×5 cross island, diagonals at 26,26 over nothing",
            "5×5 cross island at 26,37 over nothing",
        ],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset-Terrain2.tsx",
        found: &[
            "5×5 cross island at 0,1 over nothing",
            "5×5 cross island at 18,1 over nothing",
            "5×5 cross island at 36,1 over nothing",
            "5×5 cross island at 36,19 over nothing",
            "5×5 cross island at 36,35 over nothing",
            "5×5 cross island at 41,35 over nothing",
            "5×5 cross island at 18,40 over nothing",
            "5×5 cross island at 57,47 over nothing",
            "5×5 cross island at 36,54 over nothing",
            "5×5 cross island at 42,54 over nothing",
            "5×5 cross island at 57,54 over nothing",
        ],
        missed: &[
            Missed {
                block: "5×5 cross island at 0,38",
                why: TWO_TONE_TOP,
            },
            Missed {
                block: "5×5 cross island at 36,41",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 41,41",
                why: RIM_ONLY,
            },
        ],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains-16 frames.tsx",
        found: &[
            "5×5 cross island at 0,15",
            "5×5 cross island, diagonals at 0,30 over nothing",
            "5×5 cross island, diagonals at 0,37 over nothing",
        ],
        missed: &[Missed {
            block: "5×5 cross island at 0,3",
            why: TWO_TONE_TOP,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains-16 frames-with transparency.tsx",
        found: &[
            "5×5 cross island at 0,15",
            "5×5 cross island, diagonals at 0,30 over nothing",
            "5×5 cross island, diagonals at 0,37 over nothing",
        ],
        missed: &[Missed {
            block: "5×5 cross island at 0,3",
            why: TWO_TONE_TOP,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains2-16 frames.tsx",
        found: &["5×5 cross island at 0,3", "5×5 cross island at 0,15"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains2-16 frames-with transparency.tsx",
        found: &["5×5 cross island at 0,15"],
        missed: &[Missed {
            block: "5×5 cross island at 0,3",
            why: TWO_TONE_TOP,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains-8 frames.tsx",
        found: &["5×5 cross island at 0,3", "5×5 cross island at 0,10"],
        missed: &[
            Missed {
                block: "5×5 cross island at 0,17",
                why: CHECKERBOARD,
            },
            Missed {
                block: "5×5 cross island at 0,25",
                why: CHECKERBOARD,
            },
        ],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains-8 frames-with transparency.tsx",
        found: &[],
        missed: &[
            Missed {
                block: "5×5 cross island at 0,3",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,10",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,17",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,25",
                why: RIM_ONLY,
            },
        ],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains2-8 frames.tsx",
        found: &["5×5 cross island at 0,3", "5×5 cross island at 0,10"],
        missed: &[
            Missed {
                block: "5×5 cross island at 0,17",
                why: CHECKERBOARD,
            },
            Missed {
                block: "5×5 cross island at 0,25",
                why: CHECKERBOARD,
            },
        ],
    },
    PackTileset {
        file: "Ancient Ruins-Animated Terrains2-8 frames-with transparency.tsx",
        found: &[],
        missed: &[
            Missed {
                block: "5×5 cross island at 0,3",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,10",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,17",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 0,25",
                why: RIM_ONLY,
            },
        ],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 1.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 1 test.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 2.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 3.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 4.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 5.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 6.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 7.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 8.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: TWO_TONE_TOP,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 9.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: TWO_TONE_TOP,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 1-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 2-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 3-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 5-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 6-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 7-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        file: "Ancient Ruins-Tileset - wall 8-transp.tsx",
        found: &[],
        missed: &[Missed {
            block: "5×5 cross island at 0,0",
            why: RIM_ONLY,
        }],
    },
    PackTileset {
        // An edge set of garden borders in no layout's arrangement.
        file: "garden border.tsx",
        found: &[],
        missed: &[],
    },
    PackTileset {
        file: "Terrain-Desert.tsx",
        found: &[
            "5×5 cross island at 1,1 over nothing",
            "5×5 cross island at 16,2 over nothing",
            "5×5 cross island, diagonals at 27,3 over nothing",
            "5×5 cross island, diagonals at 1,16",
            "5×5 cross island, diagonals at 15,16",
            "5×5 cross island, diagonals at 29,16",
            "5×5 cross island, diagonals at 1,21 over nothing",
            "5×5 cross island, diagonals at 9,21 over nothing",
            "5×5 cross island, diagonals at 18,21",
            "5×5 cross island, diagonals at 27,21",
            "5×5 cross island, diagonals at 1,26",
            "5×5 cross island, diagonals at 21,27",
            "5×5 cross island, diagonals at 29,27 over nothing",
            "5×5 cross island at 37,27 over nothing",
            "5×5 cross island, diagonals at 1,31",
            "5×5 cross island at 20,32",
            // Sand drifts and sand patches drawn as four inner corners of rocky ground and of
            // brick floor; the pack leaves them unlabelled, as decorations.
            "2×2 hole at 9,18",
            "2×2 hole at 31,32",
            "2×2 hole at 34,35",
        ],
        missed: &[
            Missed {
                block: "5×5 cross island at 1,8",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 16,9",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island at 27,8",
                why: RIM_ONLY,
            },
            Missed {
                block: "5×5 cross island, diagonals at 1,36",
                why: "rocky ground over rocky ground, both sides the same colours; only the outline drawn between them tells them apart",
            },
        ],
    },
    PackTileset {
        file: "Desert-Tileset-cliff1-2T.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        file: "Desert-Tileset-cliff1-3T.tsx",
        found: &["5×5 cross island at 0,0 over nothing"],
        missed: &[],
    },
    PackTileset {
        // Animation frames side by side: no block.
        file: "water-to-sand.tsx",
        found: &[],
        missed: &[],
    },
    PackTileset {
        file: "water-to-rocky-ground.tsx",
        found: &[],
        missed: &[],
    },
];

/// Every tileset file under `folder` with a terrain set.
fn tilesets_with_terrain(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            tilesets_with_terrain(&path, found);
        } else if path.extension().is_some_and(|e| e == "tsx")
            && std::fs::read_to_string(&path).is_ok_and(|text| text.contains("<wangset"))
        {
            found.push(path);
        }
    }
}

/// Every terrain tileset of the Ancient Ruins and Desert packs: detection finds the blocks the
/// table lists and nothing else. The blocks it lists as missed are the packs' raised islands
/// whose top is see-through or two-toned, and one block whose two sides have the same colours.
#[test]
fn detection_finds_the_packs_blocks_and_nothing_else() {
    let mut paths = Vec::new();
    for pack in [
        "assets/ERW - Ancient Ruins V 2.3",
        "assets/Epic RPG World - Desert V1.6",
    ] {
        let Some(folder) = bt_test_data::test_data(pack) else {
            return;
        };
        tilesets_with_terrain(&folder, &mut paths);
    }
    let names: BTreeSet<String> = paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let listed: BTreeSet<String> = PACKS.iter().map(|p| p.file.to_owned()).collect();
    assert_eq!(
        names, listed,
        "the table lists every terrain tileset of the two packs"
    );
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let entry = PACKS.iter().find(|p| p.file == name).unwrap();
        let (tileset, image) = load_path(path);
        let mut found: Vec<String> = detect(&Samples::from_tileset(&tileset, &image))
            .iter()
            .map(described)
            .collect();
        found.sort();
        let mut expected: Vec<String> = entry.found.iter().map(|s| (*s).to_owned()).collect();
        expected.sort();
        assert_eq!(found, expected, "{name}");
        for missed in entry.missed {
            assert!(
                !found.iter().any(|f| f.starts_with(missed.block)),
                "{name}: {} ({}) is found now: move it to the found blocks",
                missed.block,
                missed.why
            );
        }
    }
}

/// Label All on the Ancient Ruins grass blocks: light, mid-tone, dark and extra dark grass, each
/// drawn over another; blocks of the same grass get the same terrain.
#[test]
fn label_all_tells_the_ruins_grass_tones_apart() {
    let Some((tileset, image)) = load(
        "assets/ERW - Ancient Ruins V 2.3/TiledMap Editor/Tilesets/Terrain - Ancient Ruins.tsx",
    ) else {
        return;
    };
    let samples = Samples::from_tileset(&tileset, &image);
    let fits: Vec<Fit> = detect(&samples)
        .into_iter()
        .filter(|f| RUINS_GRASS.contains(&described(f).as_str()))
        .collect();
    assert_eq!(fits.len(), RUINS_GRASS.len());
    // The pack's own set names say which grass each block is: "light grass to mid tone grass".
    let grass_of = |fit: &Fit| -> [&'static str; 2] {
        match (fit.origin.x, fit.origin.y) {
            (0, _) => ["light", "mid"],
            (5, _) => ["mid", "light"],
            (10, 2) | (10, 10) => ["mid", "dark"],
            (15, _) => ["dark", "mid"],
            (20, _) => ["extra dark", "dark"],
            _ => ["dark", "extra dark"],
        }
    };
    let blocks = KnownTerrains::default().label_all(&samples, &fits);
    let mut terrain_of: BTreeMap<&str, u8> = BTreeMap::new();
    for (fit, block) in fits.iter().zip(&blocks) {
        let [terrain, under] = grass_of(fit);
        for (grass, color) in [(terrain, block.terrain), (under, block.under)] {
            let known = *terrain_of.entry(grass).or_insert(color.0);
            assert_eq!(
                known, color.0,
                "{grass} grass in the block at {}",
                fit.origin
            );
        }
    }
    let distinct: BTreeSet<u8> = terrain_of.values().copied().collect();
    assert_eq!(
        distinct.len(),
        4,
        "four grasses, four terrains: {terrain_of:?}"
    );
}
