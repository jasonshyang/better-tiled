//! Each option and setting of rule maps, on rule maps and maps written as ASCII grids: rule tiles
//! by letter (`E` Empty, `N` NonEmpty, `O` Other, `X` Negate, `I` Ignore), atlas tiles by number.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_automap::{Area, LayerNames, ProblemKind, RunRequest, Severity, Unbound};
use bt_core::{
    Layer, LayerId, LayerKind, Map, MapObject, ObjectId, ObjectLayer, Orientation, Property,
    StaggerAxis, StaggerIndex, Value,
};
use bt_test_data::fixtures::grid_layer;
use bt_utils::IRect;
use common::{atlas, grid, grid_of, rule_map, rules_of, target_map, tiles};
use glam::{DVec2, IVec2, UVec2};

fn with_property(mut map: Map, name: &str, value: Value) -> Map {
    map.properties.insert(name.to_owned(), Property::new(value));
    map
}

fn tile_layer(name: &str, cells: &str) -> Layer {
    Layer::new(
        LayerId(0),
        name,
        LayerKind::Tile(grid_layer(cells).unwrap()),
    )
}

fn objects_layer(name: &str, objects: Vec<MapObject>) -> Layer {
    Layer::new(
        LayerId(0),
        name,
        LayerKind::Object(ObjectLayer {
            objects: Arc::new(objects),
            ..ObjectLayer::default()
        }),
    )
}

fn rectangle(id: u32, x: f64, y: f64, w: f64, h: f64) -> MapObject {
    MapObject {
        id: ObjectId(id),
        pos: DVec2::new(x, y),
        size: DVec2::new(w, h),
        ..MapObject::default()
    }
}

/// A one-rule rule map: `input` on layer `input_set`, `output` on `output_set`.
fn simple_rule(atlas: &Arc<bt_core::Tileset>, input: &str, output: &str) -> Map {
    let size = UVec2::new(
        input.lines().next().unwrap().split_whitespace().count() as u32,
        input.lines().count() as u32,
    );
    rule_map(atlas, size)
        .tile_layer("input_set", input)
        .tile_layer("output_set", output)
        .build()
        .unwrap()
}

#[test]
fn delete_tiles_removes_the_objects_on_the_input_cells_too() {
    let tiles = atlas();
    let rules = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .layer(objects_layer(
            "output_things",
            vec![rectangle(1, 4.0, 4.0, 8.0, 8.0)],
        ))
        .build()
        .unwrap();
    let rules = rules_of(vec![with_property(rules, "DeleteTiles", Value::Bool(true))]);
    let mut map = target_map(&tiles, UVec2::new(3, 1))
        .tile_layer("set", "1 . 2")
        .layer(objects_layer(
            "things",
            vec![
                rectangle(7, 2.0, 2.0, 4.0, 4.0),
                rectangle(8, 20.0, 2.0, 4.0, 4.0),
            ],
        ))
        .build()
        .unwrap();
    let report = rules.apply(&mut map, None, 1);
    // Object 7 lay on (0, 0), where the input layer has a tile, and is removed; 8 on the empty
    // cell (1, 0) stays; the rule's rectangle is placed at (0, 0).
    assert_eq!(report.objects_removed, [ObjectId(7)]);
    let things: Vec<(ObjectId, DVec2)> = map
        .layers
        .iter()
        .find(|l| l.name == "things")
        .and_then(Layer::as_object_layer)
        .unwrap()
        .objects
        .iter()
        .map(|o| (o.id, o.pos))
        .collect();
    assert_eq!(
        things,
        [
            (ObjectId(8), DVec2::new(20.0, 2.0)),
            (report.objects_added[0], DVec2::new(4.0, 4.0))
        ]
    );
}

#[test]
fn delete_tiles_erases_the_input_cells_from_the_outputs_first() {
    let tiles = atlas();
    let rules = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .tile_layer("output_out", "5")
        .build()
        .unwrap();
    let rules = rules_of(vec![with_property(rules, "DeleteTiles", Value::Bool(true))]);
    let mut map = target_map(&tiles, UVec2::new(4, 1))
        .tile_layer("set", "1 2 1 .")
        .tile_layer("out", "9 9 9 9")
        .build()
        .unwrap();
    rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "out"), "5 . 5 9");
}

#[test]
fn locks_keep_outputs_and_delete_tiles_out_unless_rules_ignore_them() {
    let tiles = atlas();
    let rule = || {
        let map = rule_map(&tiles, UVec2::new(1, 1))
            .tile_layer("input_set", "1")
            .tile_layer("output_out", "5")
            .build()
            .unwrap();
        with_property(map, "DeleteTiles", Value::Bool(true))
    };
    let map = || {
        let mut out = tile_layer("out", "9 9 9");
        out.locked = true;
        target_map(&tiles, UVec2::new(3, 1))
            .tile_layer("set", "1 2 1")
            .layer(out)
            .build()
            .unwrap()
    };
    let mut locked = map();
    let report = rules_of(vec![rule()]).apply(&mut locked, None, 1);
    assert_eq!(
        grid_of(&locked, "out"),
        "9 9 9",
        "Tiled's DeleteTiles ignores locks"
    );
    assert_eq!(report.locked_outputs.len(), 1);
    assert_eq!(report.locked_outputs[0].name, "out");
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::LockedOutput)
    );

    let mut ignored = map();
    let rule = with_property(rule(), "IgnoreLock", Value::Bool(true));
    rules_of(vec![rule]).apply(&mut ignored, None, 1);
    assert_eq!(grid_of(&ignored, "out"), "5 . 5");
}

#[test]
fn a_locked_group_keeps_outputs_out_of_its_layers() {
    // Tiled checks the lock of a copy of the layer, which has no group, so it writes into a
    // locked group's layers; here the group's lock is honoured.
    let tiles = atlas();
    let rules = rules_of(vec![simple_rule(&tiles, "1", "2")]);
    let mut group = Layer::new(
        LayerId(0),
        "group",
        LayerKind::Group(vec![tile_layer("set", "1 1")]),
    );
    group.locked = true;
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .layer(group)
        .build()
        .unwrap();
    let report = rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "1 1");
    assert_eq!(report.locked_outputs[0].matches, 2);
}

#[test]
fn match_outside_map_lets_rules_reach_past_the_edge_where_cells_read_as_empty() {
    let tiles = atlas();
    let rule = || simple_rule(&tiles, "1 E", "5 .");
    let map = || {
        target_map(&tiles, UVec2::new(3, 1))
            .tile_layer("set", "1 . 1")
            .build()
            .unwrap()
    };
    let mut inside = map();
    rules_of(vec![rule()]).apply(&mut inside, None, 1);
    assert_eq!(grid_of(&inside, "set"), "5 . 1");
    let mut outside = map();
    let rule = with_property(rule(), "MatchOutsideMap", Value::Bool(true));
    rules_of(vec![rule]).apply(&mut outside, None, 1);
    assert_eq!(grid_of(&outside, "set"), "5 . 5");
}

#[test]
fn match_in_order_lets_rules_see_earlier_writes_and_their_own() {
    let tiles = atlas();
    // A rule that extends a row of 1s to the right, one cell per match.
    let spread = || simple_rule(&tiles, "1 E", ". 1");
    let map = || {
        target_map(&tiles, UVec2::new(5, 1))
            .tile_layer("set", "1 . . . .")
            .build()
            .unwrap()
    };
    let mut together = map();
    rules_of(vec![spread()]).apply(&mut together, None, 1);
    assert_eq!(grid_of(&together, "set"), "1 1 . . .");
    let mut in_order = map();
    let report = rules_of(vec![with_property(
        spread(),
        "MatchInOrder",
        Value::Bool(true),
    )])
    .apply(&mut in_order, None, 1);
    assert_eq!(grid_of(&in_order, "set"), "1 1 1 1 1");
    assert_eq!(report.matches, 4);

    // Two rules: the second sees what the first wrote only in order.
    let chain = || {
        rule_map(&tiles, UVec2::new(3, 1))
            .tile_layer("input_set", "1 . 2")
            .tile_layer("output_set", "2 . 3")
            .build()
            .unwrap()
    };
    let map = || {
        target_map(&tiles, UVec2::new(2, 1))
            .tile_layer("set", "1 2")
            .build()
            .unwrap()
    };
    let mut together = map();
    rules_of(vec![chain()]).apply(&mut together, None, 1);
    assert_eq!(grid_of(&together, "set"), "2 3");
    let mut in_order = map();
    rules_of(vec![with_property(
        chain(),
        "MatchInOrder",
        Value::Bool(true),
    )])
    .apply(&mut in_order, None, 1);
    assert_eq!(grid_of(&in_order, "set"), "3 3");
}

#[test]
fn rule_options_rectangles_disable_and_tune_the_rules_inside_them() {
    let tiles = atlas();
    let mut disabled = rectangle(1, 0.0, 0.0, 16.0, 16.0);
    disabled
        .properties
        .insert("Disabled".into(), Property::new(Value::Bool(true)));
    let rules = rule_map(&tiles, UVec2::new(3, 1))
        .tile_layer("input_set", "1 . 2")
        .tile_layer("output_set", "3 . 4")
        .layer(objects_layer("rule_options", vec![disabled]))
        .build()
        .unwrap();
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .tile_layer("set", "1 2")
        .build()
        .unwrap();
    rules_of(vec![rules]).apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "1 4");
}

#[test]
fn mod_x_and_offset_x_pick_positions_on_a_lattice_in_map_coordinates() {
    let tiles = atlas();
    let rule = || {
        let rule = simple_rule(&tiles, "1", "2");
        with_property(rule, "ModX", Value::Int(3))
    };
    let map = || {
        target_map(&tiles, UVec2::new(9, 1))
            .tile_layer("set", "1 1 1 1 1 1 1 1 1")
            .build()
            .unwrap()
    };
    let mut full = map();
    rules_of(vec![rule()]).apply(&mut full, None, 1);
    assert_eq!(grid_of(&full, "set"), "2 1 1 2 1 1 2 1 1");
    // A selection keeps the lattice where it is (Tiled's would start at the selection's left,
    // on columns 5 and 8 here).
    let mut selected = map();
    let selection = IRect {
        x: 4,
        y: 0,
        w: 5,
        h: 1,
    };
    rules_of(vec![rule()]).apply_to(&mut selected, &[selection], 1);
    assert_eq!(grid_of(&selected, "set"), "1 1 1 1 1 1 2 1 1");
    let mut offset = map();
    let rule = with_property(rule(), "OffsetX", Value::Int(1));
    rules_of(vec![rule]).apply(&mut offset, None, 1);
    assert_eq!(grid_of(&offset, "set"), "1 2 1 1 2 1 1 2 1");
}

#[test]
fn automapping_radius_grows_the_region_around_a_selection() {
    let tiles = atlas();
    let rule = || simple_rule(&tiles, "1", "2");
    let map = || {
        target_map(&tiles, UVec2::new(5, 1))
            .tile_layer("set", "1 1 1 1 1")
            .build()
            .unwrap()
    };
    let selection = [IRect {
        x: 2,
        y: 0,
        w: 1,
        h: 1,
    }];
    let mut plain = map();
    rules_of(vec![rule()]).apply_to(&mut plain, &selection, 1);
    assert_eq!(grid_of(&plain, "set"), "1 1 2 1 1");
    let mut grown = map();
    let rule = with_property(rule(), "AutomappingRadius", Value::Int(1));
    rules_of(vec![rule]).apply_to(&mut grown, &selection, 1);
    assert_eq!(grid_of(&grown, "set"), "1 2 2 2 1");
}

#[test]
fn output_layer_properties_are_merged_into_their_targets() {
    let tiles = atlas();
    let mut output = tile_layer("output_set", "2");
    output
        .properties
        .insert("made".into(), Property::new(Value::Bool(true)));
    let mut probability = tile_layer("outputA_deco", "3");
    probability
        .properties
        .insert("Probability".into(), Property::new(Value::Float(2.0)));
    probability
        .properties
        .insert("kind".into(), Property::new(Value::String("deco".into())));
    let rules = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .layer(output)
        .layer(probability)
        .build()
        .unwrap();
    let mut map = target_map(&tiles, UVec2::new(1, 1))
        .tile_layer("set", "1")
        .build()
        .unwrap();
    let report = rules_of(vec![rules]).apply(&mut map, None, 1);
    let set = map.layers.iter().find(|l| l.name == "set").unwrap();
    assert_eq!(set.properties["made"].value, Value::Bool(true));
    let deco = map.layers.iter().find(|l| l.name == "deco").unwrap();
    assert_eq!(deco.properties["kind"].value, Value::String("deco".into()));
    assert!(!deco.properties.contains_key("Probability"));
    assert_eq!(report.created_layers, [deco.id]);
}

#[test]
fn layers_the_map_lacks_are_created_on_top_in_rule_map_order_when_written() {
    let tiles = atlas();
    let first = rule_map(&tiles, UVec2::new(3, 1))
        .tile_layer("input_set", "1 . 1")
        .tile_layer("output_b", "2 . .")
        .tile_layer("output_a", "2 . .")
        .tile_layer("output_never", ". . 2")
        .build()
        .unwrap();
    // The second rule never matches, so "never" stays empty and is not created.
    let first = {
        let mut map = first;
        if let Some(LayerKind::Tile(input)) = map.layers.get_mut(0).map(|l| &mut l.kind) {
            input
                .set_cell(
                    IVec2::new(2, 0),
                    bt_core::Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(15)),
                )
                .unwrap();
        }
        map
    };
    let second = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .tile_layer("output_c", "3")
        .build()
        .unwrap();
    let mut map = target_map(&tiles, UVec2::new(1, 1))
        .tile_layer("set", "1")
        .build()
        .unwrap();
    let report = rules_of(vec![first, second]).apply(&mut map, None, 1);
    let names: Vec<&str> = map.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["set", "b", "a", "c"]);
    assert_eq!(report.created_layers.len(), 3);
}

#[test]
fn legacy_region_layers_bring_back_the_old_rules() {
    let tiles = atlas();
    // With a regions layer the empty cell beside the 1 is part of the input: it must hold none
    // of the rule's tiles and not be empty. And rules match in order by default.
    let rules = rule_map(&tiles, UVec2::new(2, 1))
        .tile_layer("regions", "1 1")
        .tile_layer("input_set", "1 .")
        .tile_layer("output_set", "2 .")
        .build()
        .unwrap();
    let rules = rules_of(vec![rules]);
    assert_eq!(rules.rule_maps()[0].rule_count(), 1);
    let mut map = target_map(&tiles, UVec2::new(6, 1))
        .tile_layer("set", "1 . 1 1 1 3")
        .build()
        .unwrap();
    rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "1 . 1 1 2 3");
}

#[test]
fn a_rule_map_that_cannot_be_used_says_why_and_the_others_still_run() {
    let tiles = atlas();
    let broken = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .tile_layer("output_set", "2")
        .tile_layer("walls", ".")
        .build()
        .unwrap();
    let odd = with_property(
        simple_rule(&tiles, "1", "3"),
        "ModX",
        Value::String("three".into()),
    );
    let rules = rules_of(vec![broken, odd]);
    let first = &rules.rule_maps()[0];
    assert!(!first.is_used());
    assert_eq!(first.problems()[0].severity(), Severity::Error);
    let second = &rules.rule_maps()[1];
    assert!(second.is_used());
    assert_eq!(second.problems()[0].kind, ProblemKind::UnreadableOption);
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .tile_layer("set", "1 1")
        .build()
        .unwrap();
    let report = rules.apply(&mut map, None, 1);
    // ModX kept its default of 1: every position.
    assert_eq!(grid_of(&map, "set"), "3 3");
    assert_eq!(report.rule_map_matches, [0, 2]);
}

#[test]
fn input_layers_the_map_lacks_read_as_empty() {
    let tiles = atlas();
    let wants_empty = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_missing", "E")
        .tile_layer("output_out", "2")
        .build()
        .unwrap();
    let wants_tile = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_missing", "1")
        .tile_layer("output_out", "3")
        .build()
        .unwrap();
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .tile_layer("out", ". .")
        .build()
        .unwrap();
    let rules = rules_of(vec![wants_empty, wants_tile]);
    assert_eq!(rules.missing_layers(&map).len(), 1);
    let report = rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "out"), "2 2");
    assert_eq!(report.rule_map_matches, [2, 0]);
}

#[test]
fn chained_rule_maps_grow_into_infinite_maps() {
    let tiles = atlas();
    // The first rule map writes a 2 right of each 1; the second turns 2s into 3s.
    let first = simple_rule(&tiles, "1 .", ". 2");
    let second = simple_rule(&tiles, "2", "3");
    let mut map = target_map(&tiles, UVec2::new(4, 4))
        .infinite()
        .tile_layer_at("set", IVec2::new(15, 0), "1")
        .build()
        .unwrap();
    let report = rules_of(vec![first, second]).apply(&mut map, None, 1);
    let set = tiles_of(&map);
    // (16, 0) is outside the chunk the map had: the region grew there for the second rule map.
    assert_eq!(set.cell(IVec2::new(16, 0)).tile().0, 3);
    assert_eq!(report.rule_map_matches, [1, 1]);
    assert_eq!(
        report.changed,
        Some(IRect {
            x: 16,
            y: 0,
            w: 1,
            h: 1
        })
    );
}

fn tiles_of(map: &Map) -> &bt_core::TileLayer {
    tiles(map, "set")
}

#[test]
fn unbound_outputs_can_go_to_temporary_layers_later_rule_maps_read() {
    let tiles = atlas();
    let first = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_ground", "1")
        .tile_layer("output_marks", "2")
        .build()
        .unwrap();
    let second = rule_map(&tiles, UVec2::new(1, 1))
        .tile_layer("input_marks", "2")
        .tile_layer("output_deco", "3")
        .build()
        .unwrap();
    let rules = rules_of(vec![first, second]);
    let map = || {
        target_map(&tiles, UVec2::new(2, 1))
            .tile_layer("ground", "1 .")
            .tile_layer("deco", ". .")
            .build()
            .unwrap()
    };
    // Tiled's way: the intermediate layer is created.
    let mut by_name = map();
    rules.apply(&mut by_name, None, 1);
    assert_eq!(grid_of(&by_name, "deco"), "3 .");
    assert!(by_name.layers.iter().any(|l| l.name == "marks"));
    // Bound names, the rest temporary: the intermediate layer is dropped after the run.
    let mut bound = map();
    let names = LayerNames::builder()
        .tiles("ground", LayerId(1))
        .tiles("deco", LayerId(2))
        .unbound(Unbound::Temporary)
        .build();
    let request = RunRequest::builder(1).layer_names(names).build();
    let report = rules.run(&mut bound, &request);
    assert_eq!(grid_of(&bound, "deco"), "3 .");
    assert!(!bound.layers.iter().any(|l| l.name == "marks"));
    assert!(report.created_layers.is_empty());
}

#[test]
fn names_can_be_bound_to_layers_of_other_names() {
    let tiles = atlas();
    let rules = rules_of(vec![simple_rule(&tiles, "1", "2")]);
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .tile_layer("plat1", "1 1")
        .build()
        .unwrap();
    let names = LayerNames::builder().tiles("set", LayerId(1)).build();
    let request = RunRequest::builder(1)
        .area(Area::Whole)
        .layer_names(names)
        .build();
    rules.run(&mut map, &request);
    assert_eq!(grid_of(&map, "plat1"), "2 2");
    assert_eq!(map.layers.len(), 1, "no layer named set is created");
}

#[test]
fn a_region_run_runs_on_the_whole_map_until_exact_region_runs_exist() {
    let tiles = atlas();
    let rules = rules_of(vec![simple_rule(&tiles, "1", "2")]);
    let mut map = target_map(&tiles, UVec2::new(3, 1))
        .tile_layer("set", "1 1 1")
        .build()
        .unwrap();
    let report = rules.apply(
        &mut map,
        Some(IRect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        }),
        1,
    );
    assert_eq!(grid_of(&map, "set"), "2 2 2");
    assert!(!report.incremental);
    assert_eq!(report.problems[0].kind, ProblemKind::FullRunFallback);
}

/// A one-rule rule map placing a rectangle (cell (1, 1)) and a point referring to it, in two
/// object layers of one output index, on a map of `orientation`.
fn object_rule(atlas: &Arc<bt_core::Tileset>, orientation: Orientation) -> Map {
    let mut point = MapObject {
        id: ObjectId(2),
        shape: bt_core::Shape::Point,
        pos: DVec2::new(40.0, 24.0),
        ..MapObject::default()
    };
    point
        .properties
        .insert("target".into(), Property::new(Value::Object(ObjectId(1))));
    rule_map(atlas, UVec2::new(3, 3))
        .orientation(orientation)
        .stagger(StaggerAxis::Y, StaggerIndex::Odd)
        .hex_side_length(8)
        .tile_layer("input_set", ". . .\n. 1 .\n. . .")
        .layer(objects_layer(
            "output_things",
            vec![rectangle(1, 16.0, 16.0, 16.0, 16.0)],
        ))
        .layer(objects_layer("output_marks", vec![point]))
        .build()
        .unwrap()
}

fn objects_of<'m>(map: &'m Map, name: &str) -> &'m [MapObject] {
    map.layers
        .iter()
        .find(|l| l.name == name)
        .and_then(Layer::as_object_layer)
        .map(|o| o.objects.as_slice())
        .unwrap_or_default()
}

#[test]
fn objects_are_placed_once_with_new_ids_and_their_references_follow() {
    let tiles = atlas();
    let rules = rules_of(vec![object_rule(&tiles, Orientation::Orthogonal)]);
    let mut map = target_map(&tiles, UVec2::new(4, 2))
        .tile_layer("set", ". . . .\n1 . . 1")
        .build()
        .unwrap();
    map.next_object_id = 10;
    let report = rules.apply(&mut map, None, 1);
    // Matches at (0, 1) and (3, 1); the rule's input is at (1, 1) of the rule map.
    let things = objects_of(&map, "things");
    let positions: Vec<DVec2> = things.iter().map(|o| o.pos).collect();
    assert_eq!(positions, [DVec2::new(0.0, 16.0), DVec2::new(48.0, 16.0)]);
    let marks = objects_of(&map, "marks");
    assert_eq!(
        marks.len(),
        2,
        "each object placed once (Tiled 1.11 placed some twice)"
    );
    assert_eq!(report.objects_added.len(), 4);
    // Each match's point refers to that match's rectangle.
    for (mark, thing) in marks.iter().zip(things) {
        assert_eq!(mark.properties["target"].value, Value::Object(thing.id));
    }
    assert!(things.iter().chain(marks).all(|o| o.id.0 >= 10));
    assert_eq!(map.next_object_id, 14);
}

#[test]
fn objects_move_by_the_geometry_of_staggered_and_hexagonal_maps() {
    let tiles = atlas();
    for orientation in [Orientation::Staggered, Orientation::Hexagonal] {
        let rules = rules_of(vec![object_rule(&tiles, orientation)]);
        let mut map = target_map(&tiles, UVec2::new(4, 4))
            .orientation(orientation)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd)
            .hex_side_length(8)
            .tile_layer("set", ". . . .\n. 1 . .\n. . . .\n. . 1 .")
            .build()
            .unwrap();
        rules.apply(&mut map, None, 1);
        let geometry = map.geometry();
        let things = objects_of(&map, "things");
        assert_eq!(things.len(), 2, "{orientation:?}");
        // The rule's output box starts at cell (1, 1); a match moves it by the match's
        // distance from the rule's input, measured on the map's grid.
        for (thing, at) in things.iter().zip([IVec2::new(1, 1), IVec2::new(2, 3)]) {
            let delta = geometry.tile_to_pixel(at.as_dvec2())
                - geometry.tile_to_pixel(DVec2::new(1.0, 1.0));
            assert_eq!(thing.pos, DVec2::new(16.0, 16.0) + delta, "{orientation:?}");
        }
    }
}

#[test]
fn a_tile_written_from_a_tileset_the_map_lacks_adds_it() {
    let tiles = atlas();
    let other = bt_test_data::fixtures::Atlas::builder()
        .name("other")
        .grid(2, 2)
        .build_embedded()
        .unwrap()
        .tileset;
    let rules = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
        .tileset(Arc::clone(&tiles))
        .tileset(Arc::clone(&other))
        .tile_layer("input_set", "1")
        .tile_layer("output_set", "3@1")
        .build()
        .unwrap();
    let mut map = target_map(&tiles, UVec2::new(2, 1))
        .tile_layer("set", "1 2")
        .build()
        .unwrap();
    let report = rules_of(vec![rules]).apply(&mut map, None, 1);
    assert_eq!(map.tilesets.len(), 2);
    assert!(Arc::ptr_eq(&map.tilesets[1].tileset, &other));
    assert_eq!(grid_of(&map, "set"), "3@1 2");
    assert_eq!(report.tilesets_added.len(), 1);
}

use bt_test_data::fixtures::MapBuilder;

#[test]
fn problems_found_compiling_are_typed_with_their_severity() {
    let tiles = atlas();
    let orphan = rule_map(&tiles, UVec2::new(3, 1))
        .tile_layer("input_set", "1 . .")
        .tile_layer("output_set", ". . 2")
        .build()
        .unwrap();
    let rules = rules_of(vec![orphan]);
    let kinds: Vec<(ProblemKind, Severity)> = rules.rule_maps()[0]
        .problems()
        .iter()
        .map(|p| (p.kind, p.severity()))
        .collect();
    assert_eq!(
        kinds,
        [
            (ProblemKind::OrphanInput, Severity::Note),
            (ProblemKind::OrphanOutput, Severity::Note),
            (ProblemKind::NoRules, Severity::Note)
        ]
    );
    assert_eq!(grid("1 .\n. 2"), "1 .\n. 2");
}
