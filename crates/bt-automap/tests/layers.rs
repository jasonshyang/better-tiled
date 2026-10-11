//! AutoMap layers: the painted map run through the setup's runs, chained rule maps with no map
//! layer in between, bindings on other layer names, refused bindings, several runs, and the
//! project-file record written and restored, including a layer changed elsewhere.

#![cfg(test)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use bt_automap::layers::{AutoLayers, Setup, painted_record, restore};
use bt_automap::{LayerNames, ProblemKind, RuleSet};
use bt_core::{LayerId, Map, MapObject, ObjectId};
use common::{atlas, grid_of, rules_of, target_map};
use glam::{DVec2, UVec2};

/// Rules that turn tile 1 into tile 2 on `walls`, reading and writing the same name, as the
/// Highlands rules do.
fn walls_rules(atlas: &Arc<bt_core::Tileset>) -> RuleSet {
    rules_of(vec![
        common::rule_map(atlas, UVec2::new(1, 1))
            .tile_layer("input_walls", "1")
            .tile_layer("output_walls", "2")
            .build()
            .unwrap(),
    ])
}

fn setup(runs: Vec<LayerNames>) -> Setup {
    Setup {
        rules: PathBuf::from("/rules/rules.txt"),
        runs,
        seed: 7,
    }
}

fn layer_named(map: &Map, name: &str) -> LayerId {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name {
            found = Some(layer.id);
        }
    });
    found.unwrap()
}

#[test]
fn chained_rule_maps_need_no_layer_in_between() {
    let atlas = atlas();
    // The first rule map makes `mid`, the second reads it: the map has no `mid` layer.
    let rules = rules_of(vec![
        common::rule_map(&atlas, UVec2::new(1, 1))
            .tile_layer("input_set", "1")
            .tile_layer("output_mid", "2")
            .build()
            .unwrap(),
        common::rule_map(&atlas, UVec2::new(1, 1))
            .tile_layer("input_mid", "2")
            .tile_layer("output_Ground", "3")
            .build()
            .unwrap(),
    ]);
    let rule_layers = rules.rule_layers();
    assert_eq!(rule_layers.made, ["mid"]);
    let painted = target_map(&atlas, UVec2::new(3, 1))
        .tile_layer("set", "1 . 1")
        .tile_layer("Ground", ". . .")
        .build()
        .unwrap();
    let names = LayerNames::found_in(&painted, &rule_layers);
    assert_eq!(names.tile_layer("mid"), None, "made, so left temporary");
    let layers = AutoLayers::derive(Arc::new(painted.clone()), &setup(vec![names]), &rules);
    let result = layers.result();
    assert_eq!(grid_of(result, "Ground"), "3 . 3");
    assert_eq!(result.layers.len(), 2, "no layer for the name in between");
    assert_eq!(
        grid_of(layers.painted(), "Ground"),
        ". . .",
        "what was painted stays"
    );
    assert!(layers.problems().next().is_none());
}

#[test]
fn a_run_binds_the_rules_names_to_other_layers_and_runs_one_after_another() {
    let atlas = atlas();
    let rules = walls_rules(&atlas);
    let painted = target_map(&atlas, UVec2::new(2, 1))
        .tile_layer("walls", ". .")
        .tile_layer("plat1", "1 .")
        .tile_layer("plat2", ". 1")
        .build()
        .unwrap();
    let plat1 = layer_named(&painted, "plat1");
    let plat2 = layer_named(&painted, "plat2");
    let runs = vec![
        LayerNames::temporary().rebound("walls", false, Some(plat1)),
        LayerNames::temporary().rebound("walls", false, Some(plat2)),
    ];
    let layers = AutoLayers::derive(Arc::new(painted), &setup(runs), &rules);
    assert_eq!(grid_of(layers.result(), "plat1"), "2 .");
    assert_eq!(grid_of(layers.result(), "plat2"), ". 2");
    assert_eq!(
        grid_of(layers.result(), "walls"),
        ". .",
        "the layer named walls is not read"
    );
    assert_eq!(
        grid_of(layers.input_of(1).unwrap(), "plat1"),
        "2 .",
        "the second run reads what the first made"
    );
    assert!(layers.input_of(2).is_none());
    assert_eq!(layers.reports().len(), 2);
}

#[test]
fn a_run_bound_to_a_layer_that_is_gone_or_twice_does_nothing_and_says_so() {
    let atlas = atlas();
    let rules = rules_of(vec![
        common::rule_map(&atlas, UVec2::new(1, 1))
            .tile_layer("input_walls", "1")
            .tile_layer("output_top", "2")
            .build()
            .unwrap(),
    ]);
    let painted = target_map(&atlas, UVec2::new(1, 1))
        .tile_layer("plat1", "1")
        .build()
        .unwrap();
    let plat1 = layer_named(&painted, "plat1");
    let twice = LayerNames::temporary()
        .rebound("walls", false, Some(plat1))
        .rebound("top", false, Some(plat1));
    let gone = LayerNames::temporary().rebound("walls", false, Some(LayerId(99)));
    let layers = AutoLayers::derive(Arc::new(painted), &setup(vec![twice, gone]), &rules);
    assert_eq!(grid_of(layers.result(), "plat1"), "1", "nothing changed");
    let kinds: Vec<ProblemKind> = layers.problems().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        [ProblemKind::BindingRefused, ProblemKind::BindingRefused]
    );
}

#[test]
fn the_record_rebuilds_what_was_painted_from_the_map_as_saved() {
    let atlas = atlas();
    let rules = walls_rules(&atlas);
    // A tile painted by hand on plat1 (tile 3) stays as it is; the rules turn 1 into 2.
    let painted = target_map(&atlas, UVec2::new(3, 2))
        .tile_layer("plat1", "1 3 .\n. . 1")
        .tile_layer("other", "5 . .\n. . .")
        .build()
        .unwrap();
    let plat1 = layer_named(&painted, "plat1");
    let setup = setup(vec![LayerNames::temporary().rebound(
        "walls",
        false,
        Some(plat1),
    )]);
    let layers = AutoLayers::derive(Arc::new(painted.clone()), &setup, &rules);
    let saved = Map::clone(layers.result());
    assert_eq!(grid_of(&saved, "plat1"), "2 3 .\n. . 2");

    let record = painted_record(&setup, "rules/rules.txt".to_owned(), &layers);
    assert_eq!(record.rules, "rules/rules.txt");
    assert_eq!(record.seed, 7);
    assert_eq!(record.runs.len(), 1);
    assert_eq!(record.runs[0].layers[0].rules, "walls");
    assert_eq!(record.runs[0].layers[0].name, "plat1");
    assert_eq!(record.painted.len(), 1, "only plat1 differs");
    assert_eq!(record.painted[0].name, "plat1");

    let restored = restore(&record, setup.rules.clone(), &saved);
    assert!(restored.notes.is_empty(), "{:?}", restored.notes);
    assert_eq!(restored.setup, setup);
    assert_eq!(grid_of(&restored.painted, "plat1"), "1 3 .\n. . 1");
    assert_eq!(grid_of(&restored.painted, "other"), "5 . .\n. . .");

    // Changed elsewhere: the layer counts as all painted, with a note.
    let mut edited = saved.clone();
    if let Some(bt_core::LayerKind::Tile(tiles)) = edited.layer_mut(plat1).map(|l| &mut l.kind) {
        tiles
            .set_cell(
                glam::IVec2::new(1, 1),
                bt_core::Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(4)),
            )
            .unwrap();
    }
    let restored = restore(&record, setup.rules.clone(), &edited);
    assert_eq!(restored.notes.len(), 1);
    assert_eq!(restored.notes[0].kind, ProblemKind::PaintedNotRestored);
    assert_eq!(grid_of(&restored.painted, "plat1"), "2 3 .\n. 4 2");
}

#[test]
fn objects_the_rules_placed_are_kept_out_of_what_was_painted() {
    let atlas = atlas();
    let marker = |id: u32| MapObject {
        id: ObjectId(id),
        name: "spawn".into(),
        pos: DVec2::new(4.0, 4.0),
        size: DVec2::new(8.0, 8.0),
        ..MapObject::default()
    };
    let rules = rules_of(vec![
        common::rule_map(&atlas, UVec2::new(1, 1))
            .tile_layer("input_set", "1")
            .object_layer("output_spawns", vec![marker(1)])
            .build()
            .unwrap(),
    ]);
    let mut painted = target_map(&atlas, UVec2::new(2, 1))
        .tile_layer("set", "1 1")
        .object_layer("spawns", vec![marker(1)])
        .build()
        .unwrap();
    painted.next_object_id = 2;
    let names = LayerNames::found_in(&painted, &rules.rule_layers());
    assert!(names.object_layer("spawns").is_some());
    let setup = setup(vec![names]);
    let layers = AutoLayers::derive(Arc::new(painted.clone()), &setup, &rules);
    let saved = Map::clone(layers.result());
    let spawns = layer_named(&saved, "spawns");
    let count = |map: &Map| {
        map.layer(spawns)
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects
            .len()
    };
    assert_eq!(count(&saved), 3, "the painted one and one per match");

    let record = painted_record(&setup, "rules.txt".to_owned(), &layers);
    assert_eq!(record.next_object_id, Some(2));
    let entry = record.painted.iter().find(|e| e.objects).unwrap();
    assert_eq!(entry.generated.len(), 2);
    let restored = restore(&record, setup.rules.clone(), &saved);
    assert!(restored.notes.is_empty(), "{:?}", restored.notes);
    assert_eq!(count(&restored.painted), 1);
    assert_eq!(restored.painted.next_object_id, 2);
}

#[test]
fn objects_the_rules_place_keep_their_ids_from_one_result_to_the_next() {
    let atlas = atlas();
    let marker = |id: u32| MapObject {
        id: ObjectId(id),
        name: "spawn".into(),
        pos: DVec2::new(4.0, 4.0),
        size: DVec2::new(8.0, 8.0),
        ..MapObject::default()
    };
    let rules = rules_of(vec![
        common::rule_map(&atlas, UVec2::new(1, 1))
            .tile_layer("input_set", "1")
            .object_layer("output_spawns", vec![marker(1)])
            .build()
            .unwrap(),
    ]);
    let mut painted = target_map(&atlas, UVec2::new(2, 1))
        .tile_layer("set", "1 1")
        .object_layer("spawns", vec![])
        .build()
        .unwrap();
    painted.next_object_id = 1;
    let setup = setup(vec![LayerNames::found_in(&painted, &rules.rule_layers())]);
    let first = AutoLayers::update(None, Arc::new(painted.clone()), &setup, &rules);
    let spawns = layer_named(first.result(), "spawns");
    let ids = |layers: &AutoLayers| -> Vec<(u32, f64)> {
        let mut ids: Vec<(u32, f64)> = layers
            .result()
            .layer(spawns)
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects
            .iter()
            .map(|o| (o.id.0, o.pos.x))
            .collect();
        ids.sort_by(|a, b| a.1.total_cmp(&b.1));
        ids
    };
    assert_eq!(ids(&first), [(1, 4.0), (2, 20.0)]);

    // A painted object came and went: the next id moved, the placed objects keep theirs.
    let mut moved_on = painted.clone();
    moved_on.next_object_id = 5;
    let second = AutoLayers::update(Some(&first), Arc::new(moved_on), &setup, &rules);
    assert_eq!(ids(&second), [(1, 4.0), (2, 20.0)]);

    // A painted object took id 2: the object that had it moves past every id in use, the other
    // keeps its id.
    let mut taken = painted.clone();
    if let Some(bt_core::LayerKind::Object(objects)) =
        taken.layer_mut(spawns).map(|layer| &mut layer.kind)
    {
        let mut drawn = marker(2);
        drawn.name = "drawn".into();
        drawn.pos = DVec2::new(100.0, 100.0);
        Arc::make_mut(&mut objects.objects).push(drawn);
    }
    taken.next_object_id = 3;
    let third = AutoLayers::update(Some(&first), Arc::new(taken), &setup, &rules);
    let placed: Vec<(u32, f64)> = ids(&third)
        .into_iter()
        .filter(|(_, x)| *x < 100.0)
        .collect();
    assert_eq!(placed[0], (1, 4.0));
    assert!(placed[1].0 > 2, "{placed:?}");
    assert!(third.result().next_object_id > placed[1].0);

    // A result known from elsewhere (the map's file) gives its ids to the next update, though a
    // fresh run would number the placed objects otherwise.
    let mut saved = Map::clone(first.result());
    if let Some(bt_core::LayerKind::Object(objects)) =
        saved.layer_mut(spawns).map(|layer| &mut layer.kind)
    {
        for object in Arc::make_mut(&mut objects.objects) {
            object.id = ObjectId(object.id.0 + 10);
        }
    }
    saved.next_object_id = 13;
    let known = AutoLayers::known(Arc::new(painted.clone()), Arc::new(saved));
    let next = AutoLayers::update(Some(&known), Arc::new(painted), &setup, &rules);
    assert_eq!(ids(&next), [(11, 4.0), (12, 20.0)]);
}

#[test]
fn updating_after_painting_equals_deriving_again() {
    use bt_test_data::Fuzz;
    use bt_utils::rng::Rng;
    use common::random::{
        RandomMap, below, cell_at, chance, random_edit, random_map, random_rule_map,
        tile_layer_names,
    };

    let fuzz = Fuzz::from_env(0x1A7E_5ED1, 200);
    let mut around = 0;
    for case in 0..fuzz.cases {
        let mut rng = Rng::new(fuzz.case_seed(case));
        let rule_maps: Vec<_> = (0..1 + below(&mut rng, 3))
            .map(|_| random_rule_map(&mut rng))
            .collect();
        let rules_text: Vec<String> = rule_maps.iter().map(|r| r.text.clone()).collect();
        let rules = rules_of(rule_maps.into_iter().map(|r| r.map).collect());
        let RandomMap {
            map,
            content,
            compared,
            text: map_text,
            ..
        } = random_map(&mut rng);
        let names = LayerNames::found_in(&map, &rules.rule_layers());
        let runs = if chance(&mut rng, 30) {
            vec![names.clone(), names]
        } else {
            vec![names]
        };
        let setup = Setup {
            rules: PathBuf::from("/rules/rules.txt"),
            runs,
            seed: rng.next_u64(),
        };
        let before = AutoLayers::derive(Arc::new(map.clone()), &setup, &rules);
        let mut edited = map.clone();
        let edit = random_edit(&mut rng, &mut edited, content);
        let edited = Arc::new(edited);
        let updated = before.after_painting(Arc::clone(&edited), &setup, &rules);
        let derived = AutoLayers::derive(edited, &setup, &rules);
        let describe = || {
            format!(
                "case {case} (BT_FUZZ_SEED={} BT_FUZZ_CASES={}), {} runs\n{}\n{map_text}\n{}",
                fuzz.seed,
                fuzz.cases,
                setup.runs.len(),
                rules_text.join("\n"),
                edit.text
            )
        };
        assert_eq!(
            updated.result().layers.len(),
            derived.result().layers.len(),
            "{}",
            describe()
        );
        for name in tile_layer_names(&[updated.result(), derived.result()]) {
            for p in compared.points() {
                let (got, want) = (
                    cell_at(updated.result(), &name, p),
                    cell_at(derived.result(), &name, p),
                );
                assert_eq!(got, want, "{name} at {p}: {}", describe());
            }
        }
        if updated.reports().iter().all(|r| r.incremental) {
            around += 1;
        }
    }
    assert!(around > 0, "no case was updated around its edit");
}
