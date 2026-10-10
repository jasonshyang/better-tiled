//! Tiled's automapping cases (`reference/tiled/tests/automapping`), run as Tiled's own harness
//! (`test_automapping.cpp`) runs them: the case's `rules.tmx` applied to the whole of its
//! `map.tmx`, and the tile layers compared with `map-result.tmx`. The cases without a result
//! are checked for the behaviour they document, and a test asserts every case is covered, so a
//! new case in Tiled's repository fails here until it is handled.

#![cfg(test)]

mod common;

use std::collections::BTreeSet;

use bt_automap::{ProblemKind, RuleSet, files};
use bt_core::{Cell, Layer, LayerKind, Map, TileId, TileLayer, TilesetSlot};
use bt_tmx::Loader;
use bt_utils::IRect;
use common::{assert_same_tile_layers, load_map, rule_set, tiled_case, tiled_cases_dir};
use glam::{IVec2, UVec2};

/// The cases with an expected result: the thirteen Tiled's harness runs, and `add-tileset`,
/// which has a result but is not in Tiled's list.
const WITH_RESULTS: [&str; 14] = [
    "add-tileset",
    "ignore-empty-region",
    "ignore-flip",
    "infinite-target-map",
    "inputnot",
    "match-type",
    "mod-and-offset",
    "option-ignore-lock",
    "option-no-overlapping-output",
    "option-overflow-border",
    "option-wrap-border",
    "simple-2x2-rule",
    "simple-replace",
    "terrain-corner",
];

/// The cases without one, each checked by a test of its own below.
const DOCUMENTED: [&str; 5] = [
    "grow-region-while-drawing",
    "output-probability",
    "rule-probability",
    "rules-file-recursive",
    "rules-file-references-work-map",
];

/// Runs a case as Tiled's harness does, compares with its result and returns the report.
fn run_case(name: &str) -> Option<(Map, bt_automap::Report)> {
    let dir = tiled_case(name)?;
    let mut loader = Loader::default();
    let mut map = load_map(&mut loader, &dir.join("map.tmx"));
    let expected = load_map(&mut loader, &dir.join("map-result.tmx"));
    let rules: RuleSet = rule_set(&mut loader, &[dir.join("rules.tmx")]);
    let report = rules.apply(&mut map, None, 1);
    assert_same_tile_layers(name, &map, &expected);
    let unexpected: Vec<String> = report
        .problems
        .iter()
        .filter(|p| p.kind != ProblemKind::LockedOutput)
        .map(ToString::to_string)
        .collect();
    assert!(unexpected.is_empty(), "{name}: {unexpected:?}");
    Some((map, report))
}

macro_rules! tiled_case_test {
    ($test:ident, $name:literal) => {
        #[test]
        fn $test() {
            run_case($name);
        }
    };
}

tiled_case_test!(ignore_empty_region_equals_tileds, "ignore-empty-region");
tiled_case_test!(ignore_flip_equals_tileds, "ignore-flip");
tiled_case_test!(inputnot_equals_tileds, "inputnot");
tiled_case_test!(match_type_equals_tileds, "match-type");
tiled_case_test!(mod_and_offset_equals_tileds, "mod-and-offset");
tiled_case_test!(
    option_no_overlapping_output_equals_tileds,
    "option-no-overlapping-output"
);
tiled_case_test!(
    option_overflow_border_equals_tileds,
    "option-overflow-border"
);
tiled_case_test!(option_wrap_border_equals_tileds, "option-wrap-border");
tiled_case_test!(simple_2x2_rule_equals_tileds, "simple-2x2-rule");
tiled_case_test!(simple_replace_equals_tileds, "simple-replace");
tiled_case_test!(terrain_corner_equals_tileds, "terrain-corner");

#[test]
fn add_tileset_equals_tileds_and_adds_the_tileset() {
    let Some((map, report)) = run_case("add-tileset") else {
        return;
    };
    assert_eq!(map.tilesets.len(), 1);
    assert_eq!(report.tilesets_added.len(), 1);
    let file = map.tilesets[0].file.as_ref().unwrap();
    assert!(file.path.ends_with("spr_test_tileset.tsx"));
}

#[test]
fn infinite_target_map_equals_tileds() {
    let Some((map, report)) = run_case("infinite-target-map") else {
        return;
    };
    assert!(map.infinite);
    assert!(report.matches > 0);
}

#[test]
fn option_ignore_lock_equals_tileds_and_reports_the_lock() {
    let Some((_, report)) = run_case("option-ignore-lock") else {
        return;
    };
    // The rule without IgnoreLock matched twice and wrote nothing.
    assert_eq!(report.locked_outputs.len(), 1);
    assert_eq!(report.locked_outputs[0].name, "set");
    assert_eq!(report.locked_outputs[0].matches, 2);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::LockedOutput)
    );
}

#[test]
fn every_case_of_tileds_is_covered() {
    let Some(dir) = tiled_cases_dir() else {
        return;
    };
    let found: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let covered: BTreeSet<String> = WITH_RESULTS
        .iter()
        .chain(&DOCUMENTED)
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(found, covered, "a case of Tiled's is not covered here");
    for name in WITH_RESULTS {
        assert!(dir.join(name).join("map-result.tmx").exists(), "{name}");
    }
    for name in DOCUMENTED {
        assert!(!dir.join(name).join("map-result.tmx").exists(), "{name}");
    }
}

/// The tile ids of a layer's column `x`, top to bottom (`None` for empty cells).
fn column(map: &Map, layer: &str, x: i32) -> Vec<Option<u32>> {
    let tiles = tile_layer(map, layer);
    (0..i32::try_from(map.height).unwrap())
        .map(|y| {
            let cell = tiles.cell(IVec2::new(x, y));
            (!cell.is_empty()).then(|| cell.tile().0)
        })
        .collect()
}

fn tile_layer<'m>(map: &'m Map, name: &str) -> &'m TileLayer {
    map.layers
        .iter()
        .find(|l| l.name == name)
        .and_then(Layer::as_tile_layer)
        .unwrap_or_else(|| panic!("no tile layer {name}"))
}

fn tile_layer_mut<'m>(map: &'m mut Map, name: &str) -> &'m mut TileLayer {
    map.layers
        .iter_mut()
        .find(|l| l.name == name)
        .and_then(Layer::as_tile_layer_mut)
        .unwrap_or_else(|| panic!("no tile layer {name}"))
}

fn tile(id: u32) -> Cell {
    Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
}

/// `grow-region-while-drawing`: with AutoMap While Drawing, each rule map's region grows by what
/// the earlier ones wrote, so a later rule map sees cells an earlier one wrote outside the
/// stroke. Here a stroke of two 3s (tile 2) makes rules1.tmx write three 4s (tile 3), one below
/// the stroke, and rules2.tmx turns all three into 5s (tile 4); the 4 further down is outside
/// the grown region and stays.
#[test]
fn grow_region_while_drawing_grows_the_region_by_what_earlier_rule_maps_wrote() {
    let Some(dir) = tiled_case("grow-region-while-drawing") else {
        return;
    };
    let mut loader = Loader::default();
    let rules = files::load(
        &mut loader,
        &dir.join("rules.txt"),
        Some(&dir.join("map.tmx")),
    )
    .unwrap();
    assert_eq!(rules.rule_maps().len(), 2);
    let mut painted = load_map(&mut loader, &dir.join("map.tmx"));
    assert_eq!(
        column(&painted, "set", 1),
        [None, Some(3), Some(4), Some(3), None]
    );
    let set = tile_layer_mut(&mut painted, "set");
    set.set_cell(IVec2::new(1, 0), tile(2)).unwrap();
    set.set_cell(IVec2::new(1, 1), tile(2)).unwrap();

    let mut drawn = painted.clone();
    let stroke = IRect {
        x: 1,
        y: 0,
        w: 1,
        h: 2,
    };
    let report = rules.apply_to(&mut drawn, &[stroke], 1);
    assert!(report.incremental);
    assert_eq!(report.rule_map_matches, [1, 3]);
    assert_eq!(
        column(&drawn, "set", 1),
        [Some(4), Some(4), Some(4), Some(3), None]
    );

    // A full run reaches the last 4 too.
    let mut full = painted.clone();
    rules.apply(&mut full, None, 1);
    assert_eq!(
        column(&full, "set", 1),
        [Some(4), Some(4), Some(4), Some(4), None]
    );
}

/// A copy of a case's map grown to `size`, its `set` layer filled with tile 1 (Tiled's gid 2),
/// its other tile layers emptied.
fn grown(map: &Map, size: u32) -> Map {
    let mut grown = map.clone();
    grown.width = size;
    grown.height = size;
    for layer in &mut grown.layers {
        let name = layer.name.clone();
        if let LayerKind::Tile(tiles) = &mut layer.kind {
            *tiles = TileLayer::finite(UVec2::splat(size));
            if name == "set" {
                for p in IRect::from_min_size(IVec2::ZERO, IVec2::splat(size as i32)).points() {
                    tiles.set_cell(p, tile(1)).unwrap();
                }
            }
        }
    }
    grown
}

/// How many cells of a layer hold each tile id.
fn counts(map: &Map, layer: &str) -> std::collections::BTreeMap<u32, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for placed in tile_layer(map, layer).cells() {
        *counts.entry(placed.cell.tile().0).or_default() += 1;
    }
    counts
}

/// `output-probability`: the unnamed output (`auto_base`, tile 4) is always applied; one of the
/// indexed ones is chosen per match by its `Probability`: A (tile 5) three times as often as B
/// (tile 6).
#[test]
fn output_probability_applies_the_unnamed_output_and_weighs_the_others() {
    let Some(dir) = tiled_case("output-probability") else {
        return;
    };
    let mut loader = Loader::default();
    let rules = rule_set(&mut loader, &[dir.join("rules.tmx")]);
    let map = load_map(&mut loader, &dir.join("map.tmx"));
    let mut big = grown(&map, 40);
    let report = rules.apply(&mut big, None, 7);
    assert_eq!(report.matches, 1600);
    assert_eq!(counts(&big, "auto_base"), [(4, 1600)].into());
    let auto = counts(&big, "auto");
    assert_eq!(auto.keys().copied().collect::<Vec<_>>(), [5, 6]);
    let share_a = auto[&5] as f64 / 1600.0;
    assert!((0.71..0.79).contains(&share_a), "{auto:?}");
    // The same seed gives the same map; another seed another one.
    let mut again = grown(&map, 40);
    rules.apply(&mut again, None, 7);
    assert_eq!(again, big);
    let mut other = grown(&map, 40);
    rules.apply(&mut other, None, 8);
    assert_ne!(other, big);
}

/// `rule-probability`: a rule's `Probability` (set with `rule_options` rectangles) is the chance
/// it is tried at a position; the three rules write tiles 4, 5 and 6 in that order, with
/// chances 1, 0.25 and 0.1, later writes winning.
#[test]
fn rule_probability_skips_rules_by_their_chance_at_each_position() {
    let Some(dir) = tiled_case("rule-probability") else {
        return;
    };
    let mut loader = Loader::default();
    let rules = rule_set(&mut loader, &[dir.join("rules.tmx")]);
    let map = load_map(&mut loader, &dir.join("map.tmx"));
    let mut big = grown(&map, 40);
    let report = rules.apply(&mut big, None, 3);
    let auto = counts(&big, "auto");
    let share = |id: u32| auto.get(&id).copied().unwrap_or(0) as f64 / 1600.0;
    // Tile 6 where rule 3 applied (10%), else 5 where rule 2 did (22.5%), else 4.
    assert!((0.07..0.13).contains(&share(6)), "{auto:?}");
    assert!((0.19..0.26).contains(&share(5)), "{auto:?}");
    assert!((0.63..0.72).contains(&share(4)), "{auto:?}");
    assert_eq!(report.rule_map_matches[0], report.matches);
}

/// `rules-file-recursive`: a rules file that includes itself is read once, with a note; Tiled
/// would follow it for ever.
#[test]
fn rules_file_recursive_reads_a_self_including_list_once() {
    let Some(dir) = tiled_case("rules-file-recursive") else {
        return;
    };
    let mut loader = Loader::default();
    let rules = files::load(
        &mut loader,
        &dir.join("rules.txt"),
        Some(&dir.join("map.tmx")),
    )
    .unwrap();
    assert!(rules.rule_maps().is_empty());
    let kinds: Vec<ProblemKind> = rules.problems().iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [ProblemKind::IncludeCycle]);
    assert!(rules.watched_files().contains(&dir.join("rules.txt")));
    let mut map = load_map(&mut loader, &dir.join("map.tmx"));
    let before = map.clone();
    let report = rules.apply(&mut map, None, 1);
    assert_eq!(map, before);
    assert_eq!(report.matches, 0);
}

/// `rules-file-references-work-map`: a rules file listing the map itself works: the map reads as
/// a rule map (legacy `regions`, a `//` note layer) with no rules, and nothing happens.
#[test]
fn rules_file_references_work_map_works_and_does_nothing() {
    let Some(dir) = tiled_case("rules-file-references-work-map") else {
        return;
    };
    let mut loader = Loader::default();
    let map_path = dir.join("map.tmx");
    let rules = files::load(&mut loader, &dir.join("rules.txt"), Some(&map_path)).unwrap();
    assert_eq!(rules.rule_maps().len(), 1);
    let rule_map = &rules.rule_maps()[0];
    assert!(rule_map.is_used());
    assert_eq!(rule_map.rule_count(), 0);
    let kinds: Vec<ProblemKind> = rule_map.problems().iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [ProblemKind::NoRules]);
    let mut map = load_map(&mut loader, &map_path);
    let before = map.clone();
    rules.apply(&mut map, None, 1);
    assert_eq!(map, before);
}
