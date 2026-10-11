//! The runner's terrain and tiles (`10-agents.md`, §8.2, §8.3): painted terrain covers exactly
//! the cells and fits around; what a set lacks is said; terrains pushed back and other sets'
//! cells replaced are named; which layer; locked layers refuse; mistakes change nothing and name
//! their line; the same seed gives the same map.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{LayerId, TileId, Tileset, WangSetType};
use bt_script::{RunFailure, parse_script};
use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, corners as corner_label};
use common::*;
use glam::{IVec2, UVec2};

#[test]
fn painted_terrain_covers_exactly_the_cells_and_fits_around() {
    let level = level("bt-script-run-paint");
    let result = ran(&level, "paint water rect 3,2 4x3 layer ground");
    let rows = terrain_rows(&result.shown, "ground");
    assert_eq!(
        rows,
        [
            "gggggggggggg",
            "ggg++++ggggg",
            "gg+wwww+gggg",
            "gg+wwww+gggg",
            "gg+wwww+gggg",
            "ggg++++ggggg",
            "gggggggggggg",
            "gggggggggggg",
        ],
        "the painted cells are water, the cells around their edges"
    );
    assert!(
        seams(&result.shown, "ground").is_empty(),
        "the tiles around fit"
    );
    let lines: Vec<String> = result
        .outcome
        .changes
        .layers
        .iter()
        .flat_map(|layer| {
            layer
                .cells
                .as_ref()
                .map(|c| format!("{}: {}", layer.name, c.count))
        })
        .collect();
    assert_eq!(lines, ["ground: 30"]);
    let ground = result.outcome.changes.layers[0].cells.as_ref().unwrap();
    assert_eq!(ground.terrains[0].terrain, "grass");
    assert_eq!(
        (ground.terrains[0].before, ground.terrains[0].after),
        (96, 70)
    );
    assert_eq!(ground.terrains[1].terrain, "water");
    assert_eq!(
        (ground.terrains[1].before, ground.terrains[1].after),
        (0, 12)
    );
    assert!(result.outcome.changed);
    assert!(result.outcome.unfilled.is_empty());
    assert_eq!(
        result.outcome.area.map(|a| (a.x, a.y, a.w, a.h)),
        Some((2, 1, 6, 5))
    );
    // The caller's map is untouched: the script ran on a copy.
    assert!(
        level
            .map
            .layer(LayerId(1))
            .unwrap()
            .as_tile_layer()
            .unwrap()
            .cells()
            .all(|c| c.cell.tile().0 == 0)
    );
}

#[test]
fn a_terrain_goes_where_its_set_is_and_erasing_takes_it_off_its_cells() {
    let level = level("bt-script-run-erase");
    // Water's set is on `ground`; ice's on no layer yet, so it is named once.
    let result = ran(
        &level,
        "paint water rect 1,1 2x2\npaint ice rect 2,2 5x3 layer ice\nerase ice rect 2,2 2x3\nerase \
         ice rect 9,6 2x2",
    );
    assert_eq!(&terrain_rows(&result.shown, "ground")[1][..4], "+ww+");
    assert_eq!(
        terrain_rows(&result.shown, "ice"),
        [
            "............",
            "....++++....",
            "....+ii+....",
            "....+ii+....",
            "....+ii+....",
            "....++++....",
            "............",
            "............",
        ],
        "the erased cells are empty and the ice around them has its edges"
    );
    assert!(seams(&result.shown, "ice").is_empty());
    assert_eq!(
        notes(&result),
        ["line 4: erase ice: ice isn't on those cells of ice"]
    );
}

#[test]
fn what_the_set_lacks_is_said_and_the_cells_are_left_as_they_were() {
    // A set without a tile that is all water.
    let set = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Land")
        .terrains(&["Grass", "Water"])
        .every_combination();
    let mut tileset = set.added_to(Tileset::clone(
        &Atlas::builder()
            .name("ground")
            .grid(8, 4)
            .build_embedded()
            .unwrap()
            .tileset,
    ));
    tileset.wang_sets[0].tiles.remove(&TileId(15));
    let row = ["0"; 8].join(" ");
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(8, 6))
        .tileset(Arc::new(tileset))
        .tile_layer("ground", &vec![row; 6].join("\n"))
        .build()
        .unwrap();
    let result = run_on(
        &map,
        std::path::Path::new("/maps/pond.tmx"),
        "paint water rect 2,2 2x2",
        1,
    )
    .unwrap();
    assert_eq!(
        notes(&result),
        ["line 1: paint water: no tile fits 4 cells: “Land” has no tile that is all water"]
    );
    assert_eq!(
        result.outcome.unfilled,
        [
            IVec2::new(2, 2),
            IVec2::new(3, 2),
            IVec2::new(2, 3),
            IVec2::new(3, 3)
        ]
    );
    let text = bt_script::Outcome::report(&result.outcome).text();
    assert!(
        text.contains("warning: no tile fits 4 cells, left as they were: 2,2 3,2 2,3 3,3"),
        "{text}"
    );
}

#[test]
fn a_terrain_pushed_back_is_named_with_the_tile_that_would_have_kept_it() {
    // Grass borders water and sand, but water and sand have no tile together.
    let mut labels = Vec::new();
    for pair in [[1u8, 2u8], [1, 3]] {
        for a in pair {
            for b in pair {
                for c in pair {
                    for d in pair {
                        let label = [a, b, c, d];
                        if !labels.contains(&label) {
                            labels.push(label);
                        }
                    }
                }
            }
        }
    }
    let set = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Land")
        .terrains(&["Grass", "Water", "Sand"])
        .labels(labels.iter().map(|l| corner_label(*l)));
    let tileset = set.added_to(Tileset::clone(
        &Atlas::builder()
            .name("ground")
            .grid(8, 4)
            .build_embedded()
            .unwrap()
            .tileset,
    ));
    let grass = labels.iter().position(|l| *l == [1, 1, 1, 1]).unwrap();
    let row = vec![grass.to_string(); 10].join(" ");
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(10, 6))
        .tileset(Arc::new(tileset))
        .tile_layer("ground", &vec![row; 6].join("\n"))
        .build()
        .unwrap();
    let path = std::path::Path::new("/maps/coast.tmx");
    let result = run_on(
        &map,
        path,
        "paint water rect 1,1 4x4\npaint sand rect 5,1 3x4",
        1,
    )
    .unwrap();
    let pushed: Vec<&String> = result
        .outcome
        .notes
        .iter()
        .map(|n| &n.text)
        .filter(|text| text.starts_with("painting sand pushed back water"))
        .collect();
    assert_eq!(pushed.len(), 1, "{:?}", notes(&result));
    assert!(
        pushed[0].contains("“Land” has no tile with"),
        "{}",
        pushed[0]
    );
}

#[test]
fn painting_over_another_sets_tiles_says_where() {
    // A second set, of stone, in the same tileset, labelled with stone and nothing.
    let mut stone = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Rocks")
        .terrains(&["Stone"]);
    let mut id = 16;
    for a in 0..2u8 {
        for b in 0..2u8 {
            for c in 0..2u8 {
                for d in 0..2u8 {
                    if [a, b, c, d] != [0, 0, 0, 0] {
                        stone = stone.label(id, corner_label([a, b, c, d]));
                        id += 1;
                    }
                }
            }
        }
    }
    let tileset = stone.added_to(ground_tileset());
    let row = ["0"; 8].join(" ");
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(8, 6))
        .tileset(Arc::new(tileset))
        .tile_layer("ground", &vec![row; 6].join("\n"))
        .build()
        .unwrap();
    let result = run_on(
        &map,
        std::path::Path::new("/maps/rocks.tmx"),
        "paint stone rect 2,2 2x2",
        1,
    )
    .unwrap();
    let texts: Vec<String> = notes(&result);
    assert!(
        texts.iter().any(|t| t.contains("paint stone: painted over")
            && t.ends_with("of “Land”: paint each terrain set on a layer of its own")),
        "{texts:?}"
    );
}

#[test]
fn materials_paint_their_parts_in_order_on_their_layers() {
    let level = level("bt-script-run-material");
    let result = ran(
        &level,
        "define pond = water layer ground + water layer ice\npaint pond rect 1,1 3x2",
    );
    for layer in ["ground", "ice"] {
        let rows = terrain_rows(&result.shown, layer);
        assert_eq!(&rows[1][1..4], "www", "{layer}: {rows:?}");
    }
}

#[test]
fn tiles_go_by_id_or_name_and_clear_empties_cells() {
    let level = level("bt-script-run-tiles");
    let result = ran(
        &level,
        "define stones = tile ground:30,31 layer ground\ntile stones rect 0,0 2x2\nclear 5,5 layer ground\ntile ground:30,31 9,1",
    );
    let tiles = result
        .shown
        .layer(LayerId(1))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .clone();
    for cell in [IVec2::new(0, 0), IVec2::new(1, 1)] {
        assert!([30, 31].contains(&tiles.cell(cell).tile().0));
    }
    assert!(tiles.cell(IVec2::new(5, 5)).is_empty());
    assert!(
        [30, 31].contains(&tiles.cell(IVec2::new(9, 1)).tile().0),
        "the layer holding most of them"
    );
    let ice = result
        .shown
        .layer(LayerId(2))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .clone();
    assert_eq!(ice.cells().count(), 0);
}

#[test]
fn which_layer_is_asked_for_when_nothing_says_and_locked_layers_refuse() {
    let level = level("bt-script-run-layers");
    // The ground tileset's tiles are on `ground` only: a terrain goes there.
    let error = run(&level, "tile ground:30 1,1").unwrap_err();
    assert!(
        matches!(*error.failure, RunFailure::SayWhichLayer { ref layers, .. } if layers == &["ground".to_owned(), "ice".to_owned()]),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "line 1 (tile ground:30 1,1): say which layer to put the tiles on: add `layer L` (tile \
         layers: ground, ice)"
    );
    let locked = run(&level, "layer lock ice\npaint water 1,1 layer ice").unwrap_err();
    assert_eq!(locked.line, 2);
    assert_eq!(
        locked.failure.to_string(),
        "ice is locked (unlock it first: layer unlock ice)"
    );
    ran(
        &level,
        "layer lock ice\nlayer unlock ice\npaint water 1,1 layer ice",
    );
}

#[test]
fn a_mistake_changes_nothing_and_names_its_line() {
    let level = level("bt-script-run-mistake");
    let before = level.map.clone();
    let error = run(&level, "# a pond\npaint water rect 1,1 2x2\npaint lava 3,3").unwrap_err();
    assert_eq!(error.line, 3);
    assert_eq!(error.operation, "paint lava 3,3");
    assert!(!error.in_definitions);
    assert!(
        error
            .to_string()
            .starts_with("line 3 (paint lava 3,3): unknown terrain “lava”"),
        "{error}"
    );
    assert_eq!(
        level.map, before,
        "all or nothing: the caller's map is as it was"
    );
    let typo = run(&level, "paint watr 1,1").unwrap_err();
    assert!(typo.to_string().contains("did you mean water?"), "{typo}");
}

#[test]
fn the_same_seed_gives_the_same_map_and_another_seed_another() {
    let level = level("bt-script-run-seeds");
    let script = "tile ground:28-31 rect 0,0 12x8 layer ground\nscatter rock in all count 12";
    let first = run_on(&level.map, &level.path, script, 7).unwrap();
    let again = run_on(&level.map, &level.path, script, 7).unwrap();
    assert_eq!(first.shown, again.shown);
    let other = run_on(&level.map, &level.path, script, 8).unwrap();
    assert_ne!(first.shown, other.shown);
    // An operation's own seed does not depend on the script's.
    let own = "tile ground:28-31 rect 0,0 12x8 layer ground seed 5";
    let a = run_on(&level.map, &level.path, own, 1).unwrap();
    let b = run_on(&level.map, &level.path, &format!("seed 9\n{own}"), 3).unwrap();
    assert_eq!(a.shown, b.shown);
    // A `seed N` line sets every following operation's randomness.
    let lined = "seed 4\ntile ground:28-31 rect 0,0 12x8 layer ground";
    let c = run_on(&level.map, &level.path, lined, 1).unwrap();
    let d = run_on(&level.map, &level.path, lined, 2).unwrap();
    assert_eq!(c.shown, d.shown);
}

#[test]
fn the_label_is_the_first_line_comment_else_the_first_change() {
    let level = level("bt-script-run-label");
    assert_eq!(
        ran(&level, "# grow the lake\npaint water 1,1")
            .outcome
            .label,
        "grow the lake"
    );
    assert_eq!(
        ran(
            &level,
            "seed 7\ndefine x = water\npaint water 1,1\npaint water 2,2"
        )
        .outcome
        .label,
        "paint water 1,1 (+2 more)"
    );
    let script = parse_script("paint water 1,1").unwrap();
    assert_eq!(script.ops.len(), 1);
}
