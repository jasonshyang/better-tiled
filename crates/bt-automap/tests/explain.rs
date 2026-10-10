//! Explanations (§8): writes and attempts at a cell, checks at the moment Tiled checks them in
//! both matching modes, positions the run does not try, near misses, overwritten matches,
//! unnamed tiles, layer names, the rules' summaries and pictures; and, on random rule sets and
//! maps, that explanations agree with the runs they describe (`BT_FUZZ_SEED`, `BT_FUZZ_CASES`).

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_automap::{
    AttemptOutcome, Condition, LayerNames, PictureRole, RuleRef, RuleSet, RunRequest, Writer,
};
use bt_core::{Cell, Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Property, Value};
use bt_test_data::Fuzz;
use bt_test_data::fixtures::grid_layer;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use common::random::{RandomRuleMap, below, cell_at, random_map, random_rule_map};
use common::{atlas, rule_map, rules_of, target_map};
use glam::{DVec2, IVec2, UVec2};

fn tile(id: u32) -> Cell {
    Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(id))
}

fn rule(rule_map: usize, rule: usize) -> RuleRef {
    RuleRef { rule_map, rule }
}

/// A rule map of one row from its layers' grids.
fn rules_row(layers: &[(&str, &str)], options: &[(&str, Value)]) -> RuleSet {
    let tiles = atlas();
    let width = layers[0].1.split_whitespace().count() as u32;
    let mut builder = rule_map(&tiles, UVec2::new(width, 1));
    for (name, grid) in layers {
        builder = builder.tile_layer(name, grid);
    }
    let mut map = builder.build().unwrap();
    for (name, value) in options {
        map.properties
            .insert((*name).to_owned(), Property::new(value.clone()));
    }
    rules_of(vec![map])
}

fn row(layers: &[(&str, &str)]) -> Map {
    let tiles = atlas();
    let width = layers[0].1.split_whitespace().count() as u32;
    let mut builder = target_map(&tiles, UVec2::new(width, 1));
    for (name, grid) in layers {
        builder = builder.tile_layer(name, grid);
    }
    builder.build().unwrap()
}

#[test]
fn a_cell_tells_its_layers_writes_and_attempts() {
    // Two rules turn a 1 into a 2, then into a 3: the later write wins.
    let rules = rules_row(&[("input_set", "1 . 1"), ("output_set", "2 . 3")], &[]);
    let map = row(&[("set", "1 .")]);
    let explanation = rules.explain_cell(&map, IVec2::ZERO, 1);
    assert_eq!(explanation.layers.len(), 1);
    let layer = &explanation.layers[0];
    assert_eq!(
        (
            layer.layer.as_str(),
            layer.present,
            layer.before,
            layer.after
        ),
        ("set", true, tile(1), tile(3))
    );
    let writes: Vec<(Cell, Cell, Writer)> = explanation
        .writes
        .iter()
        .map(|w| (w.before, w.after, w.by))
        .collect();
    let by = |r: usize| Writer::Rule {
        rule: rule(0, r),
        position: IVec2::ZERO,
    };
    assert_eq!(
        writes,
        [(tile(1), tile(2), by(0)), (tile(2), tile(3), by(1))]
    );
    let attempts: Vec<(RuleRef, IVec2, bool)> = explanation
        .attempts
        .iter()
        .map(|a| (a.rule, a.position, a.matched))
        .collect();
    assert_eq!(
        attempts,
        [
            (rule(0, 0), IVec2::ZERO, true),
            (rule(0, 1), IVec2::ZERO, true)
        ]
    );
    let check = &explanation.attempts[0].checks[0];
    assert_eq!(
        (
            check.layer.as_str(),
            check.layer_present,
            check.at,
            check.condition,
            check.found,
            check.before,
            check.holds
        ),
        (
            "set",
            true,
            IVec2::ZERO,
            Condition::AnyOf,
            tile(1),
            tile(1),
            true
        )
    );
    assert_eq!(explanation.applied, [vec![1, 1]]);
    assert!(explanation.unnamed.is_empty() && explanation.problems.is_empty());
    // The map is left as it was.
    assert_eq!(map, row(&[("set", "1 .")]));
}

#[test]
fn checks_see_the_cells_when_tiled_checks_them() {
    // Rule 1 turns 1 into 2, rule 2 turns 2 into 3.
    let layers = [("input_set", "1 . 2"), ("output_set", "2 . 3")];
    let map = row(&[("set", "1")]);
    let second = |explanation: &bt_automap::Explanation| {
        explanation
            .attempts
            .iter()
            .find(|a| a.rule == rule(0, 1))
            .cloned()
            .unwrap()
    };
    // Matched together, rule 2 checks the cell as it was before the rule map.
    let together = rules_row(&layers, &[]).explain_cell(&map, IVec2::ZERO, 1);
    let attempt = second(&together);
    assert_eq!(attempt.outcome, AttemptOutcome::NoMatch);
    assert_eq!(
        (attempt.checks[0].found, attempt.checks[0].holds),
        (tile(1), false)
    );
    assert_eq!(together.layers[0].after, tile(2));
    // In order, it checks the cell after rule 1 wrote it.
    let in_order = rules_row(&layers, &[("MatchInOrder", Value::Bool(true))]).explain_cell(
        &map,
        IVec2::ZERO,
        1,
    );
    let attempt = second(&in_order);
    assert!(matches!(attempt.outcome, AttemptOutcome::Applied { .. }));
    let check = &attempt.checks[0];
    assert_eq!(
        (check.found, check.before, check.holds),
        (tile(2), tile(1), true)
    );
    assert_eq!(in_order.layers[0].after, tile(3));
}

#[test]
fn positions_the_run_does_not_try_say_why() {
    let tiles = atlas();
    // Rule 1 every second column; rule 2 disabled; rule 3 asks for and forbids a 1; rule 4 two
    // cells wide.
    let mut disabled = MapObject {
        pos: DVec2::new(32.0, 0.0),
        size: DVec2::new(16.0, 16.0),
        ..MapObject::default()
    };
    disabled
        .properties
        .insert("Disabled".into(), Property::new(Value::Bool(true)));
    let rules = rule_map(&tiles, UVec2::new(9, 1))
        .tile_layer("input_set", "1 . 1 . 1 . 1 1 .")
        .tile_layer("inputnot_set", ". . . . 1 . . . .")
        .tile_layer("output_set", "2 . 2 . 2 . 2 . .")
        .layer(Layer::new(
            LayerId(0),
            "rule_options",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![disabled]),
                ..ObjectLayer::default()
            }),
        ))
        .build()
        .unwrap();
    let mut rules = rules;
    rules
        .properties
        .insert("ModX".into(), Property::new(Value::Int(2)));
    let rules = rules_of(vec![rules]);
    let map = row(&[("set", "1 1")]);
    let explanation = rules.explain_cell(&map, IVec2::new(1, 0), 1);
    let outcomes: Vec<(RuleRef, AttemptOutcome)> = explanation
        .attempts
        .iter()
        .map(|a| (a.rule, a.outcome.clone()))
        .collect();
    assert_eq!(
        outcomes,
        [
            (rule(0, 0), AttemptOutcome::OffLattice),
            (rule(0, 1), AttemptOutcome::Disabled),
            (rule(0, 2), AttemptOutcome::CanNeverMatch),
            (rule(0, 3), AttemptOutcome::OffLattice),
        ]
    );
    // On an even column the two-cell rule does not fit in the map.
    let explanation = rules.explain_cell(&row(&[("set", "1 1 1")]), IVec2::new(2, 0), 1);
    let wide = explanation
        .attempts
        .iter()
        .find(|a| a.rule == rule(0, 3))
        .unwrap();
    assert_eq!(wide.outcome, AttemptOutcome::OutsideMap);
    assert!(wide.checks.is_empty() && !wide.matched);
}

#[test]
fn a_rule_its_probability_skipped_shows_whether_it_would_have_matched() {
    let rules = rules_row(
        &[("input_set", "1"), ("output_set", "2")],
        &[("Probability", Value::Float(0.0))],
    );
    let explanation = rules.explain_cell(&row(&[("set", "1 3")]), IVec2::ZERO, 1);
    let attempt = &explanation.attempts[0];
    assert_eq!(attempt.outcome, AttemptOutcome::SkippedByProbability);
    assert!(attempt.matched);
    let elsewhere = rules.explain_cell(&row(&[("set", "1 3")]), IVec2::new(1, 0), 1);
    // Where its tile is not, the rule is tried only because the explanation asks.
    assert_eq!(
        elsewhere.attempts[0].outcome,
        AttemptOutcome::SkippedByProbability
    );
    assert!(!elsewhere.attempts[0].matched);
}

#[test]
fn a_rule_tells_its_matches_and_near_misses() {
    // A 1 followed by a 2 becomes a 3.
    let rules = rules_row(&[("input_set", "1 2"), ("output_set", "3 .")], &[]);
    let map = row(&[("set", "1 2 . 1 4 5 2")]);
    let matches = rules.explain_rule(&map, rule(0, 0), 1, 1);
    let positions = |attempts: &[bt_automap::RuleAttempt]| -> Vec<i32> {
        attempts.iter().map(|a| a.position.x).collect()
    };
    assert_eq!(positions(&matches.matches), [0]);
    assert_eq!(positions(&matches.near_misses), [3, 5]);
    for miss in &matches.near_misses {
        assert_eq!(miss.failures(), Some(1));
        assert!(miss.holds_a_tile(), "{miss:?}");
    }
    assert_eq!(matches.applied, [vec![1]]);
    let exact = rules.explain_rule(&map, rule(0, 0), 1, 0);
    assert!(exact.near_misses.is_empty());
    assert_eq!(positions(&exact.matches), [0]);
}

#[test]
fn applied_matches_later_overwritten_are_found() {
    // Rule 1 writes a 2 where rule 2 then writes a 3.
    let rules = rules_row(&[("input_set", "1 . 1"), ("output_set", "2 . 3")], &[]);
    let map = row(&[("set", "1 . 1")]);
    let first = rules.explain_rule(&map, rule(0, 0), 1, 0);
    assert_eq!(first.overwritten, [IVec2::ZERO, IVec2::new(2, 0)]);
    let second = rules.explain_rule(&map, rule(0, 1), 1, 0);
    assert!(second.overwritten.is_empty());
}

#[test]
fn tiles_no_condition_names_are_unnamed() {
    let rules = rules_row(
        &[("input_set", "1"), ("input_aux", "2"), ("output_set", "3")],
        &[],
    );
    let map = row(&[("set", "1 1"), ("aux", "9 2")]);
    let unknown = rules.explain_cell(&map, IVec2::ZERO, 1);
    assert_eq!(unknown.unnamed.len(), 1);
    let unnamed = &unknown.unnamed[0];
    assert_eq!((unnamed.layer.as_str(), unnamed.tile), ("aux", tile(9)));
    let named: Vec<u32> = unnamed.named.iter().map(|t| t.tile.0).collect();
    assert_eq!(named, [2]);
    // A tile the rules name, and the layer they write, are not unnamed.
    assert!(
        rules
            .explain_cell(&map, IVec2::new(1, 0), 1)
            .unnamed
            .is_empty()
    );
}

#[test]
fn explanations_speak_the_maps_layer_names() {
    let rules = rules_row(&[("input_set", "1"), ("output_set", "2")], &[]);
    let map = row(&[("plat1", "1")]);
    let id = map.layers[0].id;
    let names = LayerNames::builder().tiles("set", id).build();
    let request = RunRequest::builder(1).layer_names(names.clone()).build();
    let explanation = rules
        .explain_cell_with(&map, IVec2::ZERO, &request)
        .with_layer_names(&names, &map);
    assert_eq!(explanation.layers[0].layer, "plat1");
    assert_eq!(explanation.layers[0].after, tile(2));
    assert_eq!(explanation.writes[0].layer, "plat1");
    assert_eq!(explanation.attempts[0].checks[0].layer, "plat1");
    let matches = rules
        .explain_rule_with(&map, rule(0, 0), &request, 0)
        .with_layer_names(&names, &map);
    assert_eq!(matches.matches[0].checks[0].layer, "plat1");
    // A binding the map cannot have refuses the run, and the explanation says so.
    let refused = RunRequest::builder(1)
        .layer_names(LayerNames::builder().tiles("set", LayerId(99)).build())
        .build();
    let explanation = rules.explain_cell_with(&map, IVec2::ZERO, &refused);
    assert!(explanation.attempts.iter().all(|a| a.checks.is_empty()));
    assert_eq!(explanation.problems.len(), 1);
}

#[test]
fn rules_are_summarised_and_drawn_as_written() {
    let tiles = atlas();
    let mut chance = Layer::new(
        LayerId(0),
        "outputA_set",
        LayerKind::Tile(grid_layer(". 4 .").unwrap()),
    );
    chance
        .properties
        .insert("Probability".into(), Property::new(Value::Float(2.0)));
    let rules = rule_map(&tiles, UVec2::new(3, 1))
        .tile_layer("input_set", "1 E .")
        .tile_layer("inputnot_aux", ". 5 .")
        .tile_layer("output_set", ". 2 .")
        .layer(chance)
        .tile_layer("outputB_deco", ". 6 .")
        .build()
        .unwrap();
    let rules = rules_of(vec![rules]);
    let summaries = rules.rule_summaries();
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.rule, rule(0, 0));
    assert_eq!(
        summary.area,
        IRect {
            x: 0,
            y: 0,
            w: 2,
            h: 1
        }
    );
    assert_eq!(summary.input, summary.area);
    assert_eq!(
        summary.output,
        IRect {
            x: 1,
            y: 0,
            w: 1,
            h: 1
        }
    );
    assert_eq!(summary.reads, ["aux", "set"]);
    assert_eq!(summary.writes, ["set", "deco"]);
    assert_eq!((summary.input_sets, summary.random_outputs), (1, 2));
    assert_eq!(summary.inactive, None);
    let picture = rules.rule_picture(rule(0, 0)).unwrap();
    let layers: Vec<(&str, PictureRole, usize)> = picture
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.role, l.cells.len()))
        .collect();
    assert_eq!(
        layers,
        [
            ("input_set", PictureRole::Input, 2),
            ("inputnot_aux", PictureRole::InputNot, 1),
            ("output_set", PictureRole::Output, 1),
            ("outputA_set", PictureRole::Output, 1),
            ("outputB_deco", PictureRole::Output, 1),
        ]
    );
    assert!(rules.rule_picture(rule(0, 1)).is_none());
}

/// Explanations against the runs they describe, on random rule sets and maps: each layer's
/// cell before and after is the map's and the full run's; the last write to a layer is its
/// after; every rule's write comes from an applied attempt there and every applied attempt has
/// an input set that holds; a miss has none; the applied counts are the run's; and a rule's
/// matches are where it applied.
#[test]
fn explanations_agree_with_the_runs_they_describe() {
    let fuzz = Fuzz::from_env(0xE7A1_A140, 120);
    let (mut cells, mut attempts, mut applied, mut writes) = (0, 0, 0, 0);
    for case in 0..fuzz.cases {
        let mut rng = Rng::new(fuzz.case_seed(case));
        let rule_maps: Vec<RandomRuleMap> = (0..1 + below(&mut rng, 3))
            .map(|_| random_rule_map(&mut rng))
            .collect();
        let rules_text: Vec<String> = rule_maps
            .iter()
            .enumerate()
            .map(|(i, r)| format!("rule map {i}:\n{}", r.text))
            .collect();
        let rules = rules_of(rule_maps.into_iter().map(|r| r.map).collect());
        let random = random_map(&mut rng);
        let map = random.map;
        let seed = rng.next_u64();
        let context = || {
            format!(
                "case {case} ({}), seed {seed}\n{}\n{}",
                fuzz.reproduce(),
                rules_text.join("\n"),
                random.text
            )
        };
        let mut full = map.clone();
        let report = rules.apply(&mut full, None, seed);
        for p in random.content.points() {
            cells += 1;
            let explanation = rules.explain_cell(&map, p, seed);
            let total: usize = explanation.applied.iter().flatten().sum();
            assert_eq!(total, report.matches, "{}", context());
            for layer in &explanation.layers {
                let at = |m: &Map| cell_at(m, &layer.layer, p);
                assert_eq!(
                    layer.before,
                    at(&map),
                    "{} at {p}: {}",
                    layer.layer,
                    context()
                );
                assert_eq!(
                    layer.after,
                    at(&full),
                    "{} at {p}: {}",
                    layer.layer,
                    context()
                );
                let last = explanation
                    .writes
                    .iter()
                    .rev()
                    .find(|w| w.layer == layer.layer);
                assert_eq!(
                    last.map_or(layer.before, |w| w.after),
                    layer.after,
                    "{} at {p}: {}",
                    layer.layer,
                    context()
                );
            }
            for write in &explanation.writes {
                writes += 1;
                if let Writer::Rule { rule, position } = write.by {
                    assert!(
                        explanation.attempts.iter().any(|a| a.rule == rule
                            && a.position == position
                            && matches!(a.outcome, AttemptOutcome::Applied { .. })),
                        "{write:?} at {p} without its applied attempt: {}",
                        context()
                    );
                }
            }
            for attempt in &explanation.attempts {
                attempts += 1;
                match attempt.outcome {
                    AttemptOutcome::Applied { .. } | AttemptOutcome::Overlapping => {
                        applied += 1;
                        assert!(attempt.matched, "{attempt:?} at {p}: {}", context());
                    }
                    AttemptOutcome::NoMatch => {
                        assert!(!attempt.matched, "{attempt:?}: {}", context());
                        assert!(
                            attempt.failures().is_some_and(|f| f > 0),
                            "{attempt:?}: {}",
                            context()
                        );
                    }
                    _ => {}
                }
            }
        }
        // A rule's matches are where it applied (or overlapped, or was skipped by chance).
        let summaries = rules.rule_summaries();
        if !summaries.is_empty() {
            let chosen = summaries[below(&mut rng, summaries.len() as u64) as usize].rule;
            let matches = rules.explain_rule(&map, chosen, seed, 1);
            let applied_here = matches
                .matches
                .iter()
                .filter(|a| matches!(a.outcome, AttemptOutcome::Applied { .. }))
                .count();
            assert_eq!(
                applied_here,
                matches.applied[chosen.rule_map][chosen.rule],
                "{chosen:?}: {}",
                context()
            );
            assert!(
                matches.near_misses.iter().all(|a| a.failures() == Some(1)),
                "{}",
                context()
            );
        }
    }
    println!("{cells} cells explained: {attempts} attempts ({applied} applied), {writes} writes");
    assert!(applied > 0 && writes > 0);
}
