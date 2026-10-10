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
use bt_core::{Cell, Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Property, Value};
use bt_test_data::Fuzz;
use bt_test_data::fixtures::grid_layer;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use common::{atlas, rule_map, rules_of, target_map};
use glam::{DVec2, IVec2, UVec2};

fn below(rng: &mut Rng, n: u64) -> u64 {
    rng.below(n).unwrap_or(0)
}

fn chance(rng: &mut Rng, percent: u64) -> bool {
    below(rng, 100) < percent
}

fn pick<'a>(rng: &mut Rng, choices: &[&'a str]) -> &'a str {
    choices[below(rng, choices.len() as u64) as usize]
}

/// The layers of a random rule map, in the order they are added.
const RULE_LAYERS: [&str; 8] = [
    "input_set",
    "input_aux",
    "inputnot_set",
    "inputB_set",
    "output_set",
    "output_out",
    "outputX_set",
    "outputY_set",
];

/// A rule map's size: three rule blocks of 3×3 cells a column apart, and a border.
const RULE_SIZE: UVec2 = UVec2::new(13, 5);

/// A random rule map, and how to read it in a failure.
struct RandomRuleMap {
    map: Map,
    text: String,
    /// The options it has that region runs must handle, by name.
    features: Vec<&'static str>,
}

fn random_rule_options(rng: &mut Rng) -> Vec<(&'static str, Value)> {
    let mut options = Vec::new();
    if chance(rng, 15) {
        options.push(("NoOverlappingOutput", Value::Bool(true)));
    }
    if chance(rng, 15) {
        options.push(("ModX", Value::Int(2 + below(rng, 2) as i64)));
        options.push(("OffsetX", Value::Int(below(rng, 3) as i64)));
    }
    if chance(rng, 10) {
        options.push(("ModY", Value::Int(2)));
        options.push(("OffsetY", Value::Int(1)));
    }
    if chance(rng, 15) {
        options.push(("Probability", Value::Float(0.6)));
    }
    options
}

fn random_rule_map(rng: &mut Rng) -> RandomRuleMap {
    let (w, h) = (RULE_SIZE.x as usize, RULE_SIZE.y as usize);
    let mut cells: Vec<Vec<Vec<&str>>> = vec![vec![vec!["."; w]; h]; RULE_LAYERS.len()];
    let rules = 1 + below(rng, 3) as usize;
    for r in 0..rules {
        let x0 = 1 + 4 * r;
        for _ in 0..1 + below(rng, 3) {
            let (x, y) = (x0 + below(rng, 3) as usize, 1 + below(rng, 3) as usize);
            let layer = match below(rng, 10) {
                0..=5 => 0,
                6 => 1,
                7 | 8 => 2,
                _ => 3,
            };
            let tile = pick(
                rng,
                &["1", "2", "3", "4", "1", "2", "E", "N", "O", "X", "I", "2h"],
            );
            cells[layer][y][x] = tile;
        }
        for _ in 0..1 + below(rng, 3) {
            let (x, y) = (x0 + below(rng, 3) as usize, 1 + below(rng, 3) as usize);
            let layer = 4 + below(rng, 4) as usize;
            let tile = pick(rng, &["1", "2", "3", "5", "6", "E", "I", "4v"]);
            cells[layer][y][x] = tile;
        }
    }
    let mut options: Vec<(&'static str, Value)> = random_rule_options(rng);
    if chance(rng, 30) {
        options.push(("MatchInOrder", Value::Bool(chance(rng, 80))));
    }
    match below(rng, 20) {
        0 | 1 => options.push(("MatchOutsideMap", Value::Bool(true))),
        2 | 3 => options.push(("OverflowBorder", Value::Bool(true))),
        4 => options.push(("WrapBorder", Value::Bool(true))),
        _ => {}
    }
    if chance(rng, 10) {
        options.push(("DeleteTiles", Value::Bool(true)));
    }
    if chance(rng, 10) {
        options.push(("AutomappingRadius", Value::Int(1)));
    }
    let tiles = atlas();
    let mut builder = rule_map(&tiles, RULE_SIZE);
    let mut text = Vec::new();
    for (index, name) in RULE_LAYERS.iter().enumerate() {
        let grid: Vec<String> = cells[index].iter().map(|row| row.join(" ")).collect();
        let grid = grid.join("\n");
        let mut layer = Layer::new(
            LayerId(0),
            *name,
            LayerKind::Tile(grid_layer(&grid).unwrap()),
        );
        if index == 0 && chance(rng, 20) {
            layer
                .properties
                .insert("AutoEmpty".into(), Property::new(Value::Bool(true)));
            text.push(format!("{name} (AutoEmpty):\n{grid}"));
        } else if index == 6 && chance(rng, 20) {
            layer
                .properties
                .insert("Probability".into(), Property::new(Value::Float(3.0)));
            text.push(format!("{name} (Probability 3):\n{grid}"));
        } else {
            text.push(format!("{name}:\n{grid}"));
        }
        builder = builder.layer(layer);
    }
    let mut rectangles = Vec::new();
    for r in 0..rules {
        if !chance(rng, 15) {
            continue;
        }
        let mut values = random_rule_options(rng);
        if chance(rng, 20) {
            values.push(("Disabled", Value::Bool(true)));
        }
        if chance(rng, 30) {
            values.push(("IgnoreLock", Value::Bool(true)));
        }
        let mut rectangle = MapObject {
            id: bt_core::ObjectId(r as u32 + 1),
            pos: DVec2::new(((1 + 4 * r) * 16) as f64 - 2.0, 14.0),
            size: DVec2::new(52.0, 52.0),
            ..MapObject::default()
        };
        text.push(format!("rule_options around rule block {r}: {values:?}"));
        for (name, value) in values {
            rectangle
                .properties
                .insert(name.to_owned(), Property::new(value));
        }
        rectangles.push(rectangle);
    }
    if !rectangles.is_empty() {
        builder = builder.layer(Layer::new(
            LayerId(0),
            "rule_options",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(rectangles),
                ..ObjectLayer::default()
            }),
        ));
    }
    let mut map = builder.build().unwrap();
    text.push(format!("options: {options:?}"));
    let features = options
        .iter()
        .filter(|(_, value)| *value != Value::Bool(false))
        .map(|(name, _)| *name)
        .collect();
    for (name, value) in options {
        map.properties.insert(name.to_owned(), Property::new(value));
    }
    RandomRuleMap {
        map,
        text: text.join("\n"),
        features,
    }
}

/// A grid of `w`×`h` cells, each one of `tiles` with `percent` chance.
fn random_cells(rng: &mut Rng, w: i32, h: i32, percent: u64, tiles: &[&str]) -> String {
    (0..h)
        .map(|_| {
            (0..w)
                .map(|_| {
                    if chance(rng, percent) {
                        pick(rng, tiles)
                    } else {
                        "."
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A random map, where its content is, and how to read it in a failure.
struct RandomMap {
    map: Map,
    infinite: bool,
    /// Where the content is, and edits go.
    content: IRect,
    /// Where results are compared: the map, or a wide area around an infinite map's content.
    compared: IRect,
    text: String,
}

fn random_map(rng: &mut Rng) -> RandomMap {
    let tiles = atlas();
    let infinite = chance(rng, 15);
    let (w, h) = if infinite { (14, 10) } else { (12, 9) };
    let set = random_cells(rng, w, h, 55, &["1", "2", "3", "4", "2h"]);
    let aux = random_cells(rng, w, h, 20, &["1", "2"]);
    let out = chance(rng, 50).then(|| random_cells(rng, w, h, 10, &["5", "6"]));
    let locked = out.is_some() && chance(rng, 20);
    let origin = if infinite {
        IVec2::new(-7, -5)
    } else {
        IVec2::ZERO
    };
    let mut builder = target_map(&tiles, UVec2::new(w as u32, h as u32));
    if infinite {
        builder = builder.infinite();
    }
    builder = builder
        .tile_layer_at("set", origin, &set)
        .tile_layer_at("aux", origin, &aux);
    let mut text = format!("infinite: {infinite}\nset:\n{set}\naux:\n{aux}");
    if let Some(out) = &out {
        builder = builder.tile_layer_at("out", origin, out);
        text.push_str(&format!("\nout (locked: {locked}):\n{out}"));
    }
    let mut map = builder.build().unwrap();
    if locked && let Some(layer) = map.layers.iter_mut().find(|l| l.name == "out") {
        layer.locked = true;
    }
    let content = IRect::from_min_size(origin, IVec2::new(w, h));
    let compared = if infinite {
        IRect::from_min_size(IVec2::splat(-40), IVec2::splat(80))
    } else {
        content
    };
    RandomMap {
        map,
        infinite,
        content,
        compared,
        text,
    }
}

/// A random edit of the map's content: a rectangle repainted on `set` or `aux`.
struct Edit {
    rect: IRect,
    text: String,
}

fn random_edit(rng: &mut Rng, map: &mut Map, content: IRect) -> Edit {
    let (w, h) = (1 + below(rng, 4) as i32, 1 + below(rng, 3) as i32);
    let rect = IRect {
        x: content.x + below(rng, (content.w - w + 1) as u64) as i32,
        y: content.y + below(rng, (content.h - h + 1) as u64) as i32,
        w,
        h,
    };
    let mut text = format!("edit {rect:?}:");
    for p in rect.points() {
        let layer = if chance(rng, 80) { "set" } else { "aux" };
        let tile = [None, Some(1), Some(2), Some(3), Some(4)][below(rng, 5) as usize];
        let cell = tile.map_or(Cell::EMPTY, |id| {
            Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(id))
        });
        if let Some(tiles) = map
            .layers
            .iter_mut()
            .find(|l| l.name == layer)
            .and_then(Layer::as_tile_layer_mut)
        {
            tiles.set_cell(p, cell).unwrap();
        }
        text.push_str(&format!(" {layer}({}, {})={tile:?}", p.x, p.y));
    }
    Edit { rect, text }
}

/// The cell at `p` of the tile layer named `name`, empty when there is none.
fn cell_at(map: &Map, name: &str, p: IVec2) -> Cell {
    let mut found = Cell::EMPTY;
    map.for_each_layer(|layer, _| {
        if layer.name == name
            && let Some(tiles) = layer.as_tile_layer()
        {
            found = tiles.cell(p);
        }
    });
    found
}

/// The names of the tile layers of the maps.
fn tile_layer_names(maps: &[&Map]) -> Vec<String> {
    let mut names = Vec::new();
    for map in maps {
        map.for_each_layer(|layer, _| {
            if layer.as_tile_layer().is_some() && !names.contains(&layer.name) {
                names.push(layer.name.clone());
            }
        });
    }
    names
}

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
