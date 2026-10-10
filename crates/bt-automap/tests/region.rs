//! Region runs against full runs (§7), on random rule sets and maps: inside the area a region run
//! may change ([`RuleSet::affected_area`]) it gives what a full run gives, outside it changes
//! nothing, and an edit changes a full run's result only inside that area; when the rules cannot
//! be run exactly around an edit, the region run is a full run whose report says so. Rule maps
//! mix every option that bears on locality: `MatchInOrder`, `NoOverlappingOutput`, `ModX`,
//! `ModY`, `Probability`, the borders, `DeleteTiles`, `AutomappingRadius`, locks, random outputs
//! and `rule_options` rectangles. `BT_FUZZ_SEED` and `BT_FUZZ_CASES` choose the cases; a failure
//! prints the case.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_automap::{ProblemKind, RuleSet};
use bt_core::{Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Property, Value};
use bt_test_data::Fuzz;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use common::random::{
    RandomMap, RandomRuleMap, below, cell_at, chance, random_edit, random_map, random_rule_map,
    tile_layer_names,
};
use common::{atlas, rule_map, rules_of, target_map};
use glam::{DVec2, UVec2};

/// The cells where `actual` differs from `inside` within `area` and from `outside` beyond it.
fn differences(
    actual: &Map,
    inside: &Map,
    outside: &Map,
    area: IRect,
    compared: IRect,
) -> Vec<String> {
    let mut found = Vec::new();
    for name in tile_layer_names(&[actual, inside, outside]) {
        for p in compared.points() {
            let reference = if area.contains(p) { inside } else { outside };
            let (got, want) = (cell_at(actual, &name, p), cell_at(reference, &name, p));
            if got != want {
                found.push(format!(
                    "{name} at ({}, {}) {}: {got:?} where {want:?} was expected",
                    p.x,
                    p.y,
                    if area.contains(p) {
                        "inside"
                    } else {
                        "outside"
                    }
                ));
            }
        }
    }
    found
}

#[test]
fn region_runs_equal_full_runs_inside_the_affected_area_and_change_nothing_outside() {
    let fuzz = Fuzz::from_env(0x5E61_0A57, 400);
    let (mut exact, mut fallbacks, mut changed) = (0, 0, 0);
    // The features the exact runs covered, by name.
    let mut covered: std::collections::BTreeMap<&str, usize> = Default::default();
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
        let mut features: Vec<&str> = rule_maps.iter().flat_map(|r| r.features.clone()).collect();
        if rule_maps.len() > 1 {
            features.push("chained rule maps");
        }
        let rules: RuleSet = rules_of(rule_maps.into_iter().map(|r| r.map).collect());
        let RandomMap {
            mut map,
            infinite,
            content,
            compared,
            text: map_text,
        } = random_map(&mut rng);
        if infinite {
            features.push("infinite map");
        }
        let seed = rng.next_u64();
        // Half the maps were automapped before the edit.
        if chance(&mut rng, 50) {
            rules.apply(&mut map, None, seed);
        }
        for round in 0..2 {
            let before_edit = map.clone();
            let edit = random_edit(&mut rng, &mut map, content);
            let context = || {
                format!(
                    "case {case} round {round} ({}), seed {seed}\n{}\n{}\n{}",
                    fuzz.reproduce(),
                    rules_text.join("\n"),
                    map_text,
                    edit.text
                )
            };
            let mut full = map.clone();
            rules.apply(&mut full, None, seed);
            let mut region = map.clone();
            let report = rules.apply(&mut region, Some(edit.rect), seed);
            match rules.affected_area(&map, edit.rect) {
                Some(area) => {
                    exact += 1;
                    for feature in &features {
                        *covered.entry(feature).or_default() += 1;
                    }
                    assert!(report.incremental, "{}", context());
                    assert!(
                        !report
                            .problems
                            .iter()
                            .any(|p| p.kind == ProblemKind::FullRunFallback),
                        "{}",
                        context()
                    );
                    let wrong = differences(&region, &full, &map, area, compared);
                    assert!(
                        wrong.is_empty(),
                        "affected area {area:?}:\n{}\n{}",
                        wrong.join("\n"),
                        context()
                    );
                    if let Some(rect) = report.changed {
                        changed += 1;
                        assert_eq!(rect.intersection(area), rect, "{}", context());
                    }
                    // The edit changes a full run's result only inside the area.
                    let mut full_before = before_edit.clone();
                    rules.apply(&mut full_before, None, seed);
                    let spread = differences(&full, &full, &full_before, area, compared);
                    assert!(
                        spread.is_empty(),
                        "the edit changed a full run outside {area:?}:\n{}\n{}",
                        spread.join("\n"),
                        context()
                    );
                }
                None => {
                    fallbacks += 1;
                    assert!(!report.incremental, "{}", context());
                    assert!(
                        report
                            .problems
                            .iter()
                            .any(|p| p.kind == ProblemKind::FullRunFallback),
                        "{}",
                        context()
                    );
                    assert!(region == full, "{}", context());
                }
            }
            map = region;
        }
    }
    println!(
        "{} region runs: {exact} exact ({changed} changed something), {fallbacks} full runs; \
         exact runs with {covered:?}",
        exact + fallbacks
    );
    assert!(
        exact * 2 > exact + fallbacks,
        "most rule sets should run exactly around an edit: {exact} exact, {fallbacks} full runs"
    );
    // Exact runs cover what makes region runs hard, as long as there are enough cases.
    if fuzz.cases >= 400 {
        for feature in [
            "MatchInOrder",
            "NoOverlappingOutput",
            "OverflowBorder",
            "MatchOutsideMap",
            "DeleteTiles",
            "AutomappingRadius",
            "chained rule maps",
            "infinite map",
        ] {
            assert!(
                covered.get(feature).copied().unwrap_or(0) > 0,
                "no exact region run with {feature}: {covered:?}"
            );
        }
    }
}

/// A one-rule rule map placing a 16×16 rectangle on the cell of each 1, and, with
/// `DeleteTiles`, erasing first.
fn object_rules(delete_tiles: bool) -> RuleSet {
    let tiles = atlas();
    let mut rules = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .layer(Layer::new(
            LayerId(0),
            "output_things",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![MapObject {
                    id: bt_core::ObjectId(1),
                    pos: DVec2::ZERO,
                    size: DVec2::splat(16.0),
                    ..MapObject::default()
                }]),
                ..ObjectLayer::default()
            }),
        ))
        .build()
        .unwrap();
    if delete_tiles {
        rules
            .properties
            .insert("DeleteTiles".into(), Property::new(Value::Bool(true)));
    }
    rules_of(vec![rules])
}

/// Where the objects of the `things` layer are.
fn things(map: &Map) -> Vec<DVec2> {
    let mut found = Vec::new();
    map.for_each_layer(|layer, _| {
        if layer.name == "things"
            && let Some(objects) = layer.as_object_layer()
        {
            found.extend(objects.objects.iter().map(|o| o.pos));
        }
    });
    found
}

#[test]
fn a_region_run_places_and_removes_the_objects_of_the_area_it_recomputes() {
    let tiles = atlas();
    let edit = IRect {
        x: 2,
        y: 0,
        w: 1,
        h: 1,
    };
    let row = || {
        target_map(&tiles, UVec2::new(5, 1))
            .tile_layer("set", "1 1 1 1 1")
            .build()
            .unwrap()
    };
    let rules = object_rules(false);
    let mut map = row();
    assert_eq!(
        rules.affected_area(&map, edit),
        Some(edit),
        "the rule reads and places on its own cell"
    );
    let report = rules.apply(&mut map, Some(edit), 1);
    assert!(report.incremental);
    assert_eq!(things(&map), [DVec2::new(32.0, 0.0)]);
    assert_eq!(report.objects_added.len(), 1);

    // DeleteTiles removes the objects on the erased cells of the area only.
    let rules = object_rules(true);
    let mut map = row();
    rules.apply(&mut map, None, 1);
    assert_eq!(things(&map).len(), 5);
    let report = rules.apply(&mut map, Some(edit), 1);
    assert!(report.incremental);
    assert_eq!(report.objects_removed.len(), 1);
    assert_eq!(report.objects_added.len(), 1);
    let mut positions: Vec<f64> = things(&map).iter().map(|p| p.x).collect();
    positions.sort_by(f64::total_cmp);
    assert_eq!(positions, [0.0, 16.0, 32.0, 48.0, 64.0]);
}
