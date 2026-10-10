//! The purchased packs (`assets/`, git-ignored; their tests skip on CI and say so).
//!
//! The Highlands pack is the benchmark. Its `rules.txt` lists 24 rule maps: eight families of
//! platform tiles, each with `rule0` ("erases all generated tiles and resets them to their
//! initial state"), `rule1` ("places all tiles") and `rule2` ("applies variations", in order,
//! each variation with a chance), all reading and writing a `walls` layer. `Sample scene.tmx`
//! shows what they make: its `walls` layer is empty and the generated platforms of two families
//! are on `plat1` (platformV2, three tiles tall) and `plat2` (platformV2, two tiles tall). Put
//! back on `walls` and automapped, a family's tiles come back: `rule0` resets them, `rule1`
//! rebuilds the platforms and `rule2` picks variations again, so every tile is the scene's own
//! or another variation of the same tile, and without the variation rule maps the result is
//! exactly the scene with its variations undone.
//!
//! The Desert and Ancient Ruins packs follow the same scheme; their rules load and run.
//!
//! Timings: `cargo test --release -p bt-automap --test packs -- --nocapture`.

#![cfg(test)]

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_automap::{ProblemKind, RuleSet, Severity, files};
use bt_core::{Cell, Map, TileLayer};
use bt_tmx::Loader;
use common::{load_map, tiles};
use glam::IVec2;

const HIGHLANDS: &str = "assets/Epic RPG World - Highlands V1.4.1/TiledMap Editor";

/// A family of the Highlands pack shown in the sample scene.
struct Family {
    /// The scene's layer holding the family's generated tiles.
    layer: &'static str,
    /// The start of the family's tileset name.
    tileset: &'static str,
    /// The start of its rule maps' file names.
    rules: &'static str,
}

const FAMILIES: [Family; 2] = [
    Family {
        layer: "plat1",
        tileset: "platformV2-3t",
        rules: "platformV2-3tiles",
    },
    Family {
        layer: "plat2",
        tileset: "platformV2-2t",
        rules: "platformV2-2tiles",
    },
];

struct Highlands {
    dir: PathBuf,
    loader: Loader,
    scene_path: PathBuf,
    scene: Map,
    rules: RuleSet,
}

fn highlands() -> Option<Highlands> {
    let dir = bt_test_data::test_data(HIGHLANDS)?;
    let mut loader = Loader::default();
    let scene_path = dir.join("Sample scene.tmx");
    let scene = load_map(&mut loader, &scene_path);
    let rules = files::load(&mut loader, &dir.join("rules.txt"), Some(&scene_path)).unwrap();
    Some(Highlands {
        dir,
        loader,
        scene_path,
        scene,
        rules,
    })
}

impl Highlands {
    /// The family's tiles on its layer of the scene.
    fn family_tiles(&self, family: &Family) -> Vec<(IVec2, Cell)> {
        tiles(&self.scene, family.layer)
            .cells()
            .filter(|placed| {
                self.scene
                    .tileset_of(placed.cell)
                    .is_some_and(|t| t.name.starts_with(family.tileset))
            })
            .map(|placed| (placed.at, placed.cell))
            .collect()
    }

    /// The scene with the family's tiles put back on `walls`.
    fn scene_with_walls(&self, family: &Family) -> Map {
        let mut map = self.scene.clone();
        let walls = walls_mut(&mut map);
        for (p, cell) in self.family_tiles(family) {
            walls.set_cell(p, cell).unwrap();
        }
        map
    }

    /// Each variation tile of the family and the tile it varies, read from the family's
    /// `rule2` map: a variation rule's output cell replaces the input cell at its position.
    fn variation_bases(&mut self, family: &Family) -> HashMap<u32, u32> {
        let path = self.dir.join(format!(
            "Rules/{}-rule2-applies variations.tmx",
            family.rules
        ));
        let rule_map = load_map(&mut self.loader, &path);
        let input = tiles(&rule_map, "input_walls");
        let output = tiles(&rule_map, "output_walls");
        let mut bases = HashMap::new();
        for placed in output.cells() {
            let base = input.cell(placed.at);
            if !base.is_empty() && base.tile() != placed.cell.tile() {
                bases.insert(placed.cell.tile().0, base.tile().0);
            }
        }
        bases
    }
}

fn walls_mut(map: &mut Map) -> &mut TileLayer {
    map.layers
        .iter_mut()
        .find(|l| l.name == "walls")
        .and_then(|l| l.as_tile_layer_mut())
        .unwrap()
}

/// The tile a variation varies, following chains of variations.
fn base_of(bases: &HashMap<u32, u32>, mut tile: u32) -> u32 {
    for _ in 0..8 {
        match bases.get(&tile) {
            Some(&base) => tile = base,
            None => break,
        }
    }
    tile
}

#[test]
fn highlands_rules_load_with_only_notes() {
    let Some(pack) = highlands() else {
        return;
    };
    let rules = &pack.rules;
    assert_eq!(rules.rule_maps().len(), 24);
    assert_eq!(rules.rule_count(), 1850);
    assert!(rules.problems().is_empty(), "{:?}", rules.problems());
    for rule_map in rules.rule_maps() {
        assert!(rule_map.is_used());
        assert_eq!(rule_map.inputs(), ["walls"]);
        assert_eq!(rule_map.tile_outputs(), ["walls"]);
        // The "places all tiles" maps of the three-tile families keep six stray input tiles
        // the author left beside the rules; they make no rule, which a note says.
        for problem in rule_map.problems() {
            assert_eq!(problem.severity(), Severity::Note, "{problem}");
            assert_eq!(problem.kind, ProblemKind::OrphanInput);
            assert!(rule_map.path().to_string_lossy().contains("3tiles-rule1"));
        }
    }
    assert!(rules.tileset_copies(&pack.scene).is_empty());
    assert!(rules.missing_layers(&pack.scene).is_empty());
}

#[test]
fn highlands_rules_reproduce_the_sample_scene_up_to_the_variations_they_pick() {
    let Some(mut pack) = highlands() else {
        return;
    };
    for family in &FAMILIES {
        let bases = pack.variation_bases(family);
        let expected = pack.family_tiles(family);
        assert!(!expected.is_empty());
        for seed in [1, 2, 3, 99] {
            let mut map = pack.scene_with_walls(family);
            let report = pack.rules.apply(&mut map, None, seed);
            assert!(report.problems.is_empty(), "{:?}", report.problems);
            let walls = tiles(&map, "walls");
            let (mut same, mut varied, mut wrong) = (0, 0, Vec::new());
            for &(p, want) in &expected {
                let got = walls.cell(p);
                if got == want {
                    same += 1;
                } else if got.slot() == want.slot()
                    && got.flip() == want.flip()
                    && base_of(&bases, got.tile().0) == base_of(&bases, want.tile().0)
                {
                    varied += 1;
                } else {
                    wrong.push((p, want.tile().0, got.tile().0));
                }
            }
            let extra = walls.cells().count() - expected.len() + wrong.len();
            println!(
                "{} (seed {seed}): {} tiles, {same} the same, {varied} other variations of the \
                 same tile, {} wrong, {extra} extra; {} matches",
                family.layer,
                expected.len(),
                wrong.len(),
                report.matches
            );
            assert!(wrong.is_empty(), "{} seed {seed}: {wrong:?}", family.layer);
            assert_eq!(walls.cells().count(), expected.len(), "no tile added");
        }
    }
}

#[test]
fn highlands_rules_without_variations_give_the_scenes_base_tiles_exactly() {
    let Some(mut pack) = highlands() else {
        return;
    };
    let entries = bt_tmx::parse_rules_file(&pack.dir.join("rules.txt"))
        .unwrap()
        .entries;
    let mut builder = RuleSet::builder();
    for entry in entries
        .iter()
        .filter(|e| !e.path.to_string_lossy().contains("rule2"))
    {
        let map = load_map(&mut pack.loader, &entry.path);
        builder = builder.rule_map(entry.path.clone(), Arc::new(map));
    }
    let rules = builder.build();
    assert_eq!(rules.rule_maps().len(), 16);
    for family in &FAMILIES {
        let bases = pack.variation_bases(family);
        let mut map = pack.scene_with_walls(family);
        rules.apply(&mut map, None, 0);
        let walls = tiles(&map, "walls");
        let mismatched: Vec<(IVec2, u32, u32)> = pack
            .family_tiles(family)
            .into_iter()
            .filter_map(|(p, want)| {
                let got = walls.cell(p);
                let base = base_of(&bases, want.tile().0);
                (got.tile().0 != base || got.slot() != want.slot()).then_some((
                    p,
                    base,
                    got.tile().0,
                ))
            })
            .collect();
        assert!(mismatched.is_empty(), "{}: {mismatched:?}", family.layer);
    }
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times.get(times.len() / 2).copied().unwrap_or_default()
}

/// The full run of the Highlands rules on the sample scene's platforms, timed. The bound is
/// asserted only in optimised builds (`cargo test --release`); a debug run reports.
#[test]
fn highlands_full_run_stays_under_its_time_bound() {
    let Some(pack) = highlands() else {
        return;
    };
    let family = &FAMILIES[0];
    let map = pack.scene_with_walls(family);
    let mut times = Vec::new();
    for seed in 0..9 {
        let mut fresh = map.clone();
        let start = Instant::now();
        pack.rules.apply(&mut fresh, None, seed);
        times.push(start.elapsed());
    }
    let full = median(times);
    let start = Instant::now();
    let _ = files::load(
        &mut Loader::default(),
        &pack.dir.join("rules.txt"),
        Some(&pack.scene_path),
    )
    .unwrap();
    let load = start.elapsed();
    // The scene's platforms repeated 8×8 times: anchors keep a run proportional to the tiles
    // the rules look for, not to the rules times the cells.
    let large = repeated(&map, 8);
    let start = Instant::now();
    let report = pack.rules.apply(&mut large.clone(), None, 1);
    let large_run = start.elapsed();
    println!(
        "Highlands: loading the rules {load:?}; a full run on the 60×60 scene {full:?}, on a \
         480×480 map {large_run:?} ({} matches)",
        report.matches
    );
    if !cfg!(debug_assertions) {
        assert!(full < Duration::from_millis(10), "full run {full:?}");
        assert!(load < Duration::from_millis(100), "loading {load:?}");
        assert!(
            large_run < Duration::from_millis(100),
            "large run {large_run:?}"
        );
    }
}

/// A region run after a stroke, on the Highlands rules and the scene's platforms repeated to a
/// 480×480 map: exact (a full run's result inside the area the stroke can change, nothing
/// changed outside it) and timed. The bound is asserted only in optimised builds.
#[test]
fn highlands_region_run_after_a_stroke_is_exact_and_stays_under_its_time_bound() {
    let Some(pack) = highlands() else {
        return;
    };
    let family = &FAMILIES[0];
    assert!(pack.rules.supports_incremental());
    let mut map = repeated(&pack.scene_with_walls(family), 8);
    pack.rules.apply(&mut map, None, 1);
    // The stroke: three cells of a platform's row in the middle of the map painted with the
    // tile of its first cell, as a pen drags along it.
    let (start, tile) = tiles(&map, "walls")
        .cells()
        .find(|placed| placed.at.x >= 240 && placed.at.y >= 240)
        .map(|placed| (placed.at, placed.cell))
        .unwrap();
    let stroke = bt_utils::IRect {
        x: start.x,
        y: start.y,
        w: 3,
        h: 1,
    };
    for p in stroke.points() {
        walls_mut(&mut map).set_cell(p, tile).unwrap();
    }
    let area = pack.rules.affected_area(&map, stroke).unwrap();
    let mut full = map.clone();
    pack.rules.apply(&mut full, None, 1);
    let mut times = Vec::new();
    let mut region = map.clone();
    for _ in 0..9 {
        region = map.clone();
        let start = Instant::now();
        let report = pack.rules.apply(&mut region, Some(stroke), 1);
        times.push(start.elapsed());
        assert!(report.incremental, "{:?}", report.problems);
    }
    let time = median(times);
    let (walls, full_walls, before) = (
        tiles(&region, "walls"),
        tiles(&full, "walls"),
        tiles(&map, "walls"),
    );
    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let p = IVec2::new(x, y);
            let want = if area.contains(p) {
                full_walls.cell(p)
            } else {
                before.cell(p)
            };
            assert_eq!(walls.cell(p), want, "at {p} (inside: {})", area.contains(p));
        }
    }
    println!(
        "Highlands: a region run after a 3-cell stroke on a 480×480 map {time:?}; it may change \
         {}×{} cells (margin {:?})",
        area.w,
        area.h,
        pack.rules.margin()
    );
    if !cfg!(debug_assertions) {
        assert!(time < Duration::from_millis(5), "region run {time:?}");
    }
}

/// Explanations on the Highlands pack: what made a platform tile of the sample scene (the last
/// write, by a rule applied there, the result a full run leaves), and where that rule matches.
#[test]
fn highlands_explanations_tell_what_made_a_platform_tile() {
    let Some(pack) = highlands() else {
        return;
    };
    let family = &FAMILIES[0];
    let map = pack.scene_with_walls(family);
    let mut full = map.clone();
    pack.rules.apply(&mut full, None, 1);
    // A tile the run changed.
    let cell = tiles(&full, "walls")
        .cells()
        .map(|placed| placed.at)
        .find(|&p| tiles(&map, "walls").cell(p) != tiles(&full, "walls").cell(p))
        .unwrap();
    let start = Instant::now();
    let explanation = pack.rules.explain_cell(&map, cell, 1);
    let cell_time = start.elapsed();
    assert_eq!(explanation.layers.len(), 1);
    assert_eq!(
        explanation.layers[0].after,
        tiles(&full, "walls").cell(cell)
    );
    let Some(bt_automap::Writer::Rule { rule, position }) = explanation.writes.last().map(|w| w.by)
    else {
        panic!("a rule wrote the platform tile: {:?}", explanation.writes);
    };
    let attempt = explanation
        .attempts
        .iter()
        .find(|a| a.rule == rule && a.position == position)
        .unwrap();
    assert!(matches!(
        attempt.outcome,
        bt_automap::AttemptOutcome::Applied { .. }
    ));
    let start = Instant::now();
    let matches = pack.rules.explain_rule(&map, rule, 1, 1);
    let rule_time = start.elapsed();
    assert!(matches.matches.iter().any(|a| a.position == position));
    println!(
        "Highlands: explaining a platform tile {cell_time:?} ({} attempts, {} writes); where \
         {rule} matches {rule_time:?} ({} matches, {} near misses)",
        explanation.attempts.len(),
        explanation.writes.len(),
        matches.matches.len(),
        matches.near_misses.len()
    );
}

/// After a run on one family's platforms, the other families' rule maps matched nothing: each
/// says its input held tiles but no rule matched them.
#[test]
fn highlands_rule_maps_of_other_families_match_nothing_and_say_so() {
    let Some(pack) = highlands() else {
        return;
    };
    let map = pack.scene_with_walls(&FAMILIES[0]);
    let request = bt_automap::RunRequest::builder(1).build();
    let mut ran = map.clone();
    let report = pack.rules.run(&mut ran, &request);
    let idle = pack.rules.idle_rule_maps(&map, &request, &report);
    let unmatched: Vec<usize> = (0..report.rule_map_matches.len())
        .filter(|&i| report.rule_map_matches[i] == 0)
        .collect();
    assert_eq!(
        idle.iter().map(|i| i.rule_map).collect::<Vec<_>>(),
        unmatched
    );
    assert!(idle.len() >= 18, "{} idle rule maps", idle.len());
    for rule_map in &idle {
        let problems = rule_map.problems(&pack.rules);
        assert_eq!(problems.len(), 1);
        assert_eq!(
            problems[0].kind,
            ProblemKind::IdleNoMatch,
            "{}",
            problems[0]
        );
    }
}

/// `map` with its `walls` repeated `times`×`times` over a map that many times larger.
fn repeated(map: &Map, times: u32) -> Map {
    let mut large = map.clone();
    large.width = map.width * times;
    large.height = map.height * times;
    let size = glam::UVec2::new(large.width, large.height);
    let walls = tiles(map, "walls").clone();
    large.layers.retain(|l| l.name == "walls");
    let mut repeated = TileLayer::finite(size);
    let step = IVec2::new(map.width as i32, map.height as i32);
    for placed in walls.cells() {
        for j in 0..times as i32 {
            for i in 0..times as i32 {
                let at = placed.at + IVec2::new(i, j) * step;
                repeated.set_cell(at, placed.cell).unwrap();
            }
        }
    }
    *walls_mut(&mut large) = repeated;
    large
}

/// The Desert and Ancient Ruins packs: every rule map loads and is used, the only problems are
/// the notes about stray input tiles their "places all tiles" maps share with Highlands', and
/// the rules run on the packs' maps.
#[test]
fn desert_and_ancient_ruins_rules_load_and_run() {
    for (dir, maps, rule_maps) in [
        (
            "assets/Epic RPG World - Desert V1.6/tiledmap editor",
            ["Sample scene.tmx", "Desert testing map.tmx"],
            7,
        ),
        (
            "assets/ERW - Ancient Ruins V 2.3/TiledMap Editor",
            ["Sample scene.tmx", "Ancient Ruins example map.tmx"],
            48,
        ),
    ] {
        let Some(dir) = bt_test_data::test_data(dir) else {
            return;
        };
        let mut matched = 0;
        for name in maps {
            let mut loader = Loader::default();
            let path = dir.join(name);
            let mut map = load_map(&mut loader, &path);
            let rules = files::load(&mut loader, &dir.join("rules.txt"), Some(&path)).unwrap();
            assert_eq!(rules.rule_maps().len(), rule_maps, "{}", dir.display());
            assert!(rules.problems().is_empty());
            for rule_map in rules.rule_maps() {
                assert!(rule_map.is_used());
                for problem in rule_map.problems() {
                    assert_eq!(problem.kind, ProblemKind::OrphanInput, "{problem}");
                    assert!(problem.text.contains("(27, 14)"), "{problem}");
                }
            }
            assert!(rules.tileset_copies(&map).is_empty());
            let report = rules.apply(&mut map, None, 1);
            assert!(report.problems.is_empty(), "{:?}", report.problems);
            matched += report.matches;
            // A second run on its own result changes nothing but variations it picks again:
            // the rules reset what they made first.
            let mut again = map.clone();
            rules.apply(&mut again, None, 1);
            assert_eq!(again, map, "{name}: a second run with the same seed");
        }
        assert!(matched > 0, "{}: the rules matched", dir.display());
    }
}
