//! The runner's map operations (`10-agents.md`, §8.8): `resize` and what it leaves to do,
//! `draw` in its order, layers and properties, definitions from a `--with` file, the editor's
//! selection, and copies from other maps.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{LayerId, ObjectId, Value};
use bt_script::{RunInput, Selection, parse_definitions, parse_script, run_script};
use common::*;
use glam::{DVec2, IVec2};

#[test]
fn a_resize_says_what_it_leaves_to_do() {
    let level = level("bt-script-run-resize");
    let result = ran(&level, "object wall rect 11,0 1x8\nresize 14x8");
    assert_eq!(
        notes(&result),
        [
            "line 2: resize: new cells on the right (12,0 2x8)",
            "line 2: resize: 1 object reaches the old right edge (x=12): #1 wall on design",
            "line 2: resize: the new cells (12,0 2x8) are still empty on ground, which had a tile \
             in every cell",
        ]
    );
    let resized = result.outcome.changes.resized.unwrap();
    assert_eq!(
        (resized.before, resized.after),
        (IVec2::new(12, 8), IVec2::new(14, 8))
    );
    assert!(result.outcome.changed, "a resize alone changes the map");

    // Filled in: nothing is left to do, and the changes are said of the old cells moved.
    let filled = ran(
        &level,
        "resize 12x10 shift 0,2\ntile ground:0 rect 0,0 12x2 layer ground",
    );
    assert_eq!(
        notes(&filled),
        ["line 1: resize: new cells on the top (0,0 12x2)"]
    );
    let text = bt_script::Outcome::report(&filled.outcome).text();
    assert!(
        text.contains("map size 12x8 → 12x10, the old cells moved by 0,2 (what follows compares with them there)"),
        "{text}"
    );
    assert!(
        text.contains("ground: 24 cells changed in 0,0 12x2"),
        "{text}"
    );
    let mut infinite = level.map.clone();
    infinite.infinite = true;
    let error = run_on(&infinite, &level.path, "resize 4x4", 1).unwrap_err();
    assert_eq!(
        error.failure.to_string(),
        "an infinite map has no size to change"
    );
}

#[test]
fn a_drawing_paints_terrains_then_clears_then_tiles_then_sprites_and_objects() {
    let level = level("bt-script-run-draw");
    let result = ran(
        &level,
        "draw 1,1\n  w = water layer ground\n  x = clear layer ground\n  t = tile ground:31 layer \
         ice\n  T = sprite tree\n  S = object spawn wave=1\n  |ww..\n  |wwxT\n  |t..S\nend",
    );
    let map = &result.shown;
    let rows = terrain_rows(map, "ground");
    assert_eq!(&rows[1][1..3], "ww");
    assert_eq!(
        &rows[2][1..4],
        "ww.",
        "the cell to clear is cleared after the paint"
    );
    let ice = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    assert_eq!(ice.cell(IVec2::new(1, 3)).tile().0, 31);
    let tree = &objects(map, "props")[0];
    assert!(
        close(stands(map, tree), DVec2::new(4.5, 3.0)),
        "the bottom middle of its cell"
    );
    let spawn = &objects(map, "design")[0];
    assert_eq!(
        spawn.pos,
        DVec2::new(4.5 * 16.0, 3.5 * 16.0),
        "the centre of its cell"
    );
    assert_eq!(spawn.properties["wave"].value, Value::Int(1));

    let outside = ran(&level, "draw 10,0\n  r = sprite rock\n  |rrr\nend");
    assert_eq!(objects(&outside.shown, "props").len(), 2);
    assert_eq!(
        notes(&outside),
        ["line 1: draw: the rows (from line 3) reach outside the map; those cells are left out"]
    );
}

#[test]
fn layers_are_added_named_shown_locked_and_moved_and_the_map_takes_properties() {
    let level = level("bt-script-run-layers-ops");
    let result = ran(
        &level,
        "layer add sky tiles below ground\nlayer add markers objects above design\nlayer \
         rename ice frost\nlayer hide frost\nlayer lock frost\nlayer move props above \
         markers\nproperty title=\"Ice Cave\" level=2",
    );
    let map = &result.shown;
    let names: Vec<&str> = map.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["sky", "ground", "frost", "design", "markers", "props"]
    );
    let frost = map.layer(LayerId(2)).unwrap();
    assert!(!frost.visible && frost.locked);
    assert_eq!(
        map.properties["title"].value,
        Value::String("Ice Cave".into())
    );
    assert_eq!(map.properties["level"].value, Value::Int(2));
    let text = bt_script::Outcome::report(&result.outcome).text();
    for line in [
        "map property level = 2 (new)",
        "frost: renamed from ice",
        "frost: hidden",
        "frost: locked",
        "sky: new tile layer",
        "markers: new object layer",
        "layer order changed",
    ] {
        assert!(text.contains(line), "{line}: {text}");
    }
    let twice = run(&level, "layer add sky tiles\nlayer add sky tiles").unwrap_err();
    assert_eq!(twice.line, 2);
    assert_eq!(
        twice.failure.to_string(),
        "there is already a tile layer called sky"
    );
    let locked = run(&level, "layer lock ice\nlayer delete ice").unwrap_err();
    assert!(locked.failure.to_string().starts_with("ice is locked"));
}

#[test]
fn definitions_from_a_with_file_come_first_and_their_mistakes_say_so() {
    let level = level("bt-script-run-with");
    let definitions = parse_definitions("seed 3\ndefine pond = area rect 1,1 2x2").unwrap();
    let script = parse_script("paint water where pond").unwrap();
    let input = RunInput::builder(Arc::new(level.map.clone()), &level.path)
        .definitions(definitions)
        .build();
    let result = run_script(&script, &input, &mut bt_tmx::Loader::default()).unwrap();
    assert_eq!(&terrain_rows(&result.shown, "ground")[1][1..3], "ww");

    let broken = parse_definitions("define pond = area rect 1,1 2x2\ndefine x = lava").unwrap();
    let input = RunInput::builder(Arc::new(level.map.clone()), &level.path)
        .definitions(broken)
        .build();
    let error = run_script(&script, &input, &mut bt_tmx::Loader::default()).unwrap_err();
    assert!(error.in_definitions);
    assert_eq!(error.line, 2);
    assert!(
        error
            .to_string()
            .starts_with("--with file, line 2 (define x = lava): unknown terrain")
    );
    let not_definitions = parse_script("paint water 1,1").unwrap();
    let input = RunInput::builder(Arc::new(level.map.clone()), &level.path)
        .definitions(not_definitions)
        .build();
    let refused = run_script(&script, &input, &mut bt_tmx::Loader::default()).unwrap_err();
    assert_eq!(
        refused.failure.to_string(),
        "a definitions file holds define lines (and seed)"
    );
}

#[test]
fn selection_and_selected_are_what_the_editor_has_selected() {
    let level = level("bt-script-run-selection");
    let with_rock = ran(&level, "place rock at 1.5,2");
    let selection = Selection {
        cells: vec![IVec2::new(5, 5), IVec2::new(6, 5)],
        objects: vec![ObjectId(1)],
    };
    let script =
        parse_script("tile ground:31 selection layer ground\nmove selected by 2,0").unwrap();
    let input = RunInput::builder(Arc::clone(&with_rock.painted), &level.path)
        .selection(selection)
        .build();
    let result = run_script(&script, &input, &mut bt_tmx::Loader::default()).unwrap();
    let ground = result
        .shown
        .layer(LayerId(1))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    assert_eq!(ground.cell(IVec2::new(6, 5)).tile().0, 31);
    assert!(close(
        stands(&result.shown, &objects(&result.shown, "props")[0]),
        DVec2::new(3.5, 2.0)
    ));
    let error = run(&level, "scatter rock in selection").unwrap_err();
    assert!(
        error
            .failure
            .to_string()
            .contains("isn't open in the editor")
    );
}

#[test]
fn copies_come_from_this_map_or_another_with_the_objects_standing_there() {
    let level = level("bt-script-run-copy");
    let other_path = level.scratch.join("other.tmx");
    let other = ran(
        &level,
        "tile ground:31 rect 0,0 2x2 layer ground\nplace rock at 0.5,1",
    );
    bt_tmx::WriteRequest::for_map(&other.shown, &other_path)
        .write()
        .unwrap();
    let result = ran(
        &level,
        "copy other.tmx 0,0 2x2 to 5,5 objects\ncopy . 5,5 1x1 to 0,7",
    );
    let ground = result
        .shown
        .layer(LayerId(1))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    assert_eq!(ground.cell(IVec2::new(6, 6)).tile().0, 31);
    assert_eq!(ground.cell(IVec2::new(0, 7)).tile().0, 31);
    let rocks = objects(&result.shown, "props");
    assert_eq!(rocks.len(), 1);
    assert!(close(
        stands(&result.shown, &rocks[0]),
        DVec2::new(5.5, 6.0)
    ));
    let missing = run(&level, "copy nowhere.tmx 0,0 2x2 to 5,5").unwrap_err();
    assert!(
        missing
            .failure
            .to_string()
            .contains("nowhere.tmx can't be read"),
        "{missing}"
    );
}
