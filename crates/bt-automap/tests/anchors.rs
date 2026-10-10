//! Anchors only decide where rules are tried: on random rule maps and maps, a run gives the same
//! map and the same matches as a run that tries every rule at every position of its lattice,
//! in either matching mode, with the border options and with `ModX`. `BT_FUZZ_SEED` and
//! `BT_FUZZ_CASES` choose the cases; a failure prints the case.

#![cfg(test)]

mod common;

use bt_automap::{Area, Observer, RuleRef, RunRequest};
use bt_core::{Map, Property, Value};
use bt_test_data::Fuzz;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use common::{atlas, rule_map, rules_of, target_map};
use glam::UVec2;

/// An observer that has every rule tried everywhere and wants no reports.
struct EveryPosition;

impl Observer for EveryPosition {
    fn tries_every_position(&self, _rule: RuleRef) -> bool {
        true
    }
}

fn pick<'a>(rng: &mut Rng, choices: &[&'a str]) -> &'a str {
    let index = rng.below(choices.len() as u64).unwrap() as usize;
    choices[index]
}

/// A grid of `width`×`height` tokens drawn from `choices`.
fn random_grid(rng: &mut Rng, width: usize, height: usize, choices: &[&str]) -> String {
    (0..height)
        .map(|_| {
            (0..width)
                .map(|_| pick(rng, choices))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Three rules side by side, two cells wide and two tall, a column apart.
fn random_rules_grid(rng: &mut Rng, choices: &[&str]) -> String {
    let rows: Vec<String> = (0..2)
        .map(|_| {
            (0..3)
                .map(|_| format!("{} {}", pick(rng, choices), pick(rng, choices)))
                .collect::<Vec<_>>()
                .join(" . ")
        })
        .collect();
    rows.join("\n")
}

struct Case {
    rules: Map,
    map: Map,
    area: Area,
    text: String,
}

fn random_case(seed: u64) -> Case {
    let mut rng = Rng::new(seed);
    let tiles = atlas();
    let input = random_rules_grid(&mut rng, &["0", "1", "2", "E", "N", "O", "X", ".", "."]);
    let inputnot = random_rules_grid(&mut rng, &["0", "1", ".", ".", ".", "."]);
    let output = random_rules_grid(&mut rng, &["0", "1", "3", "E", ".", "."]);
    let target = if rng.below(2) == Some(0) {
        "set"
    } else {
        "out"
    };
    let mut rules = rule_map(&tiles, UVec2::new(8, 2))
        .tile_layer("input_set", &input)
        .tile_layer("inputnot_set", &inputnot)
        .tile_layer(&format!("output_{target}"), &output)
        .build()
        .unwrap();
    let mut options = Vec::new();
    for (name, value) in [
        ("MatchInOrder", Value::Bool(rng.below(2) == Some(0))),
        ("MatchOutsideMap", Value::Bool(rng.below(3) == Some(0))),
        ("OverflowBorder", Value::Bool(rng.below(5) == Some(0))),
        ("WrapBorder", Value::Bool(rng.below(5) == Some(0))),
        ("NoOverlappingOutput", Value::Bool(rng.below(4) == Some(0))),
        ("ModX", Value::Int(1 + rng.below(3).unwrap() as i64)),
        (
            "Probability",
            Value::Float(if rng.below(4) == Some(0) { 0.5 } else { 1.0 }),
        ),
    ] {
        options.push(format!("{name}={value:?}"));
        rules.properties.insert(name.into(), Property::new(value));
    }
    let set = random_grid(&mut rng, 9, 6, &["0", "1", "2", "3", ".", "."]);
    let map = target_map(&tiles, UVec2::new(9, 6))
        .tile_layer("set", &set)
        .tile_layer("out", &random_grid(&mut rng, 9, 6, &["0", ".", "."]))
        .build()
        .unwrap();
    // The whole map, or a selection of a few rectangles (scanned band by band).
    let area = if rng.below(2) == Some(0) {
        Area::Whole
    } else {
        let count = 1 + rng.below(3).unwrap();
        Area::Selection(
            (0..count)
                .map(|_| {
                    let mut at = |limit: u64| rng.below(limit).unwrap() as i32;
                    IRect {
                        x: at(8),
                        y: at(5),
                        w: 1 + at(4),
                        h: 1 + at(3),
                    }
                })
                .collect(),
        )
    };
    let text = format!(
        "options: {}\narea: {area:?}\ninput_set:\n{input}\ninputnot_set:\n{inputnot}\noutput_{target}:\n{output}\nset:\n{set}",
        options.join(", ")
    );
    Case {
        rules,
        map,
        area,
        text,
    }
}

#[test]
fn anchors_never_change_what_a_run_gives() {
    let fuzz = Fuzz::from_env(0xA11C_0125, 300);
    let mut matched = 0;
    for case in 0..fuzz.cases {
        let Case {
            rules,
            map,
            area,
            text,
        } = random_case(fuzz.case_seed(case));
        let rules = rules_of(vec![rules]);
        let request = RunRequest::builder(u64::from(case)).area(area).build();
        let mut anchored = map.clone();
        let report = rules.run(&mut anchored, &request);
        let mut everywhere = map.clone();
        let full = rules.run_observed(&mut everywhere, &request, &mut EveryPosition);
        assert!(
            anchored == everywhere && report.matches == full.matches,
            "case {case} ({}): anchored {} matches, everywhere {}\n{text}",
            fuzz.reproduce(),
            report.matches,
            full.matches
        );
        if report.matches > 0 {
            matched += 1;
        }
    }
    // The cases are worth checking: most of them match somewhere.
    assert!(
        matched * 2 > fuzz.cases,
        "only {matched} of {} cases matched",
        fuzz.cases
    );
}
