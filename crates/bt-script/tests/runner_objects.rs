//! The runner's sprites, groups, pieces and objects (`10-agents.md`, §8.4, §8.5): sprites stand
//! where they are put; `scatter` keeps its distance and fits groups; pieces are placed and
//! scattered by their foot; objects with their shapes and typed values; `set`, `like`, `last`,
//! `move` and `delete … on`.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{
    ClassType, ClassUsage, CustomType, CustomTypes, Flip, MapObject, ObjectId, Property, Shape,
    Value,
};
use bt_script::{RunFailure, RunInput, parse_script, run_script};
use common::*;
use glam::{DVec2, IVec2};

fn d(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

#[test]
fn sprites_stand_where_they_are_put() {
    let level = level("bt-script-run-place");
    let result = ran(&level, "place tree at 3.5,6\nplace rock in rect 1,1 2x1");
    let props = objects(&result.shown, "props");
    assert_eq!(props.len(), 3, "every sprite on the props layer");
    assert!(close(stands(&result.shown, &props[0]), d(3.5, 6.0)));
    assert_eq!(standing_in(&result.shown, &props[0]), IVec2::new(3, 5));
    assert!(close(stands(&result.shown, &props[1]), d(1.5, 2.0)));
    assert!(close(stands(&result.shown, &props[2]), d(2.5, 2.0)));
    // Tiled anchors a tile object by its bottom-left: the tree's trunk is in its middle, two
    // pixels up from the bottom of its image.
    assert_eq!(props[0].pos, d(3.5 * 16.0 - 8.0, 6.0 * 16.0 + 2.0));
    let text = bt_script::Outcome::report(&result.outcome).text();
    assert!(
        text.contains(
            "\n  props: +3 objects #1-3: #1 tree at 3.5,6; #2 rock at 1.5,2; #3 rock at 2.5,2\n"
        ),
        "{text}"
    );
}

#[test]
fn scatter_keeps_its_distance_and_says_when_it_ran_out_of_room() {
    let level = level("bt-script-run-scatter");
    let result = ran(
        &level,
        "place tree at 6.5,4\nscatter tree in all count 20 spacing 3",
    );
    let props = objects(&result.shown, "props");
    let feet: Vec<DVec2> = props.iter().map(|o| stands(&result.shown, o)).collect();
    assert!(feet.len() > 4 && feet.len() < 21, "{} placed", feet.len());
    for (i, a) in feet.iter().enumerate() {
        for b in &feet[i + 1..] {
            assert!(a.distance(*b) >= 3.0, "{a} and {b} are too close");
        }
    }
    let placed = feet.len() - 1;
    assert_eq!(
        notes(&result),
        [format!(
            "line 2: scatter placed {placed} of 20 (no room left 3 cells apart)"
        )]
    );
    // Density counts the shape's cells: 0.1 of 96 is 10.
    let dense = ran(&level, "scatter rock in all density 0.1 spacing 0");
    assert_eq!(objects(&dense.shown, "props").len(), 10);
}

#[test]
fn groups_stand_at_their_offsets_mirrored_when_flipped_and_scatter_fits_them() {
    let level = level("bt-script-run-groups");
    let result = ran(
        &level,
        "layer add shadows objects below props\ndefine pine = sprite tree layer props + sprite \
         shadow at 1,0.5 layer shadows\nplace pine at 3.5,4 name big type=pine\nplace pine at \
         7.5,4 flip\nscatter pine in rect 0,0 5x8 count 6 spacing 1.5",
    );
    let map = &result.shown;
    let trees = objects(map, "props");
    let shadows = objects(map, "shadows");
    assert_eq!(trees.len(), shadows.len());
    assert!(close(stands(map, &shadows[0]), d(4.5, 4.5)));
    assert_eq!(
        (trees[0].name.as_str(), trees[0].class.as_str()),
        ("big", "pine")
    );
    assert_eq!(
        (shadows[0].name.as_str(), shadows[0].class.as_str()),
        ("", "")
    );
    // Flipped: the shadow's offset is mirrored and the sprites are mirrored.
    assert!(close(stands(map, &trees[1]), d(7.5, 4.0)));
    assert!(close(stands(map, &shadows[1]), d(6.5, 4.5)));
    assert!(trees[1].tile.unwrap().flip().contains(Flip::H));
    // Scattered groups have every sprite in the shape.
    for shadow in &shadows[2..] {
        let cell = standing_in(map, shadow);
        assert!(cell.x < 5, "a shadow stands outside the shape at {cell}");
    }
    assert!(trees.len() > 2);
}

#[test]
fn like_last_and_set_give_objects_their_types_and_values() {
    let level = level("bt-script-run-like");
    let result = ran(
        &level,
        "place rock at 1.5,2 type=gem points=5\nplace rock at 3.5,2 5.5,2 like #1\nset last \
         points=7 shine=\nset #1 name=first points=",
    );
    let props = objects(&result.shown, "props");
    assert_eq!(props[0].name, "first");
    assert!(
        props[0].properties.is_empty(),
        "an empty value removes a property"
    );
    for rock in &props[1..] {
        assert_eq!(rock.class, "gem");
        assert_eq!(rock.properties["points"].value, Value::Int(7));
    }
    let error = run(&level, "set last points=1").unwrap_err();
    assert_eq!(
        error.failure.to_string(),
        "`last` stands for the objects the operation before made, and none has made any"
    );
}

#[test]
fn objects_move_by_their_feet_points_and_corners() {
    let level = level("bt-script-run-move");
    let result = ran(
        &level,
        "place tree at 3.5,6\nmove #1 to 5.5,7\nobject spawn at 2.5,2\nobject zone rect \
         1,1 2x2\nmove #2 #3 by 1,1",
    );
    let map = &result.shown;
    assert!(close(stands(map, &objects(map, "props")[0]), d(5.5, 7.0)));
    let design = objects(map, "design");
    assert_eq!(design[0].pos, d(3.5 * 16.0, 3.0 * 16.0));
    assert_eq!(
        design[1].pos,
        d(32.0, 32.0),
        "a shape by its top-left corner"
    );
    let moved = &result.outcome.changes.layers;
    assert!(moved.iter().any(|l| l.name == "design"));
    let error = run(
        &level,
        "object spawn at 1,1\nobject spawn at 2,2\nmove spawn to 3,3",
    )
    .unwrap_err();
    assert_eq!(
        error.failure.to_string(),
        "there are 2 spawn objects: name one by id (#1, #2)"
    );
}

#[test]
fn delete_takes_objects_by_name_place_or_ground() {
    let level = level("bt-script-run-delete");
    let result = ran(
        &level,
        "paint water rect 0,0 4x8\nplace tree at 1.5,3 6.5,3 9.5,6\nobject spawn at \
         10.5,1.5\ndelete objects on water\ndelete objects in rect 5,0 3x8 type tree\ndelete \
         objects on water",
    );
    let props = objects(&result.shown, "props");
    let left: Vec<u32> = props.iter().map(|o| o.id.0).collect();
    assert_eq!(
        left,
        [3],
        "the tree on water and the one in the rectangle went"
    );
    assert_eq!(objects(&result.shown, "design").len(), 1);
    assert_eq!(
        notes(&result),
        ["line 6: delete: no objects stand on water"]
    );
}

#[test]
fn pieces_are_placed_and_scattered_by_their_foot() {
    let level = level("bt-script-run-pieces");
    let result = ran(
        &level,
        "tile ground:31 rect 1,1 2x2 layer ground\nplace rock at 2.5,3\ndefine hut = copy . \
         1,1 2x2 objects\nplace hut at 8,6",
    );
    let map = &result.shown;
    let ground = map
        .layer(layer_id(map, "ground"))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .clone();
    // The foot is the middle of the bottom row, 1,2 in the map copied from: 8,6 here.
    for cell in [
        IVec2::new(8, 5),
        IVec2::new(9, 5),
        IVec2::new(8, 6),
        IVec2::new(9, 6),
    ] {
        assert_eq!(ground.cell(cell).tile().0, 31, "{cell}");
    }
    let rocks = objects(map, "props");
    assert_eq!(rocks.len(), 2);
    assert!(
        close(stands(map, &rocks[1]), d(9.5, 7.0)),
        "the rock came along"
    );

    let scattered = ran(
        &level,
        "tile ground:31 rect 1,1 2x2 layer ground\ndefine hut = copy . 1,1 2x2\nscatter hut in \
         all count 3 spacing 3",
    );
    let tiles = scattered
        .shown
        .layer(layer_id(&scattered.shown, "ground"))
        .unwrap()
        .as_tile_layer()
        .unwrap()
        .cells()
        .filter(|c| c.cell.tile().0 == 31)
        .count();
    assert_eq!(tiles, 16, "the piece itself and three more, all on the map");

    let mixed = run(
        &level,
        "define hut = copy . 1,1 2x2\nplace hut, tree at 1,1",
    )
    .unwrap_err();
    assert_eq!(
        mixed.failure.to_string(),
        "pieces (define … = copy) and sprites can't be mixed in one operation"
    );
    let outside = run(&level, "define hut = copy . 1,1 2x2 foot 5,5").unwrap_err();
    assert_eq!(
        outside.failure.to_string(),
        "the foot 5,5 is outside the piece 1,1 2x2"
    );
}

#[test]
fn a_sprite_is_stretched_to_a_box_on_orthogonal_maps() {
    let level = level("bt-script-run-rect");
    let result = ran(&level, "place rock rect 1,1 2x1");
    let rock = &objects(&result.shown, "props")[0];
    assert_eq!(rock.size, d(32.0, 16.0));
    assert_eq!(rock.pos, d(16.0, 32.0), "Tiled's bottom-left anchor");
    let group = run(
        &level,
        "define pair = sprite rock + sprite tree at 1,0\nplace pair rect 1,1 2x1",
    )
    .unwrap_err();
    assert!(group.failure.to_string().contains("stretches one sprite"));
}

#[test]
fn objects_take_their_shapes_in_tile_units_and_values_their_types() {
    let level = level("bt-script-run-objects");
    let result = ran(
        &level,
        "object spawn at 4.5,5 enemy=golem wave=3 speed=1.5 boss=true\nobject hazard rect 1,1 \
         3x2 kind=\"thin ice\"\nobject pool ellipse 6,4 2x1\nobject wall poly 0,0 2,0 2,2\nobject \
         road path 0,7 11,7 target=#1\nobject coin in rect 0,0 12x8 count 3 spacing 2",
    );
    let design = objects(&result.shown, "design");
    let spawn = &design[0];
    assert_eq!(spawn.shape, Shape::Point);
    assert_eq!(spawn.pos, d(72.0, 80.0));
    assert_eq!(
        spawn.properties["enemy"].value,
        Value::String("golem".into())
    );
    assert_eq!(spawn.properties["wave"].value, Value::Int(3));
    assert_eq!(spawn.properties["speed"].value, Value::Float(1.5));
    assert_eq!(spawn.properties["boss"].value, Value::Bool(true));
    assert_eq!(
        (design[1].pos, design[1].size),
        (d(16.0, 16.0), d(48.0, 32.0))
    );
    assert_eq!(
        design[1].properties["kind"].value,
        Value::String("thin ice".into())
    );
    assert_eq!(
        (design[2].pos, design[2].size),
        (d(64.0, 48.0), d(64.0, 32.0))
    );
    assert_eq!(
        design[3].shape,
        Shape::Polygon(vec![d(0.0, 0.0), d(32.0, 0.0), d(32.0, 32.0)])
    );
    assert_eq!(
        design[4].properties["target"].value,
        Value::Object(ObjectId(1))
    );
    let coins: Vec<&MapObject> = design.iter().filter(|o| o.class == "coin").collect();
    assert_eq!(coins.len(), 3);
    for (i, a) in coins.iter().enumerate() {
        for b in &coins[i + 1..] {
            assert!(a.pos.distance(b.pos) >= 32.0);
        }
    }
    // Sizes keep a shape's top-left; points have none.
    let resized = ran(
        &level,
        "object hazard rect 1,1 3x2\nset #1 size=1x1 rotation=45 visible=false name=pit",
    );
    let pit = &objects(&resized.shown, "design")[0];
    assert_eq!((pit.pos, pit.size), (d(16.0, 16.0), d(16.0, 16.0)));
    assert_eq!(
        (pit.rotation, pit.visible, pit.name.as_str()),
        (45.0, false, "pit")
    );
    let point = run(&level, "object spawn at 1,1\nset #1 size=2x2").unwrap_err();
    assert_eq!(
        point.failure.to_string(),
        "points, polygons and paths have no size (move their points instead)"
    );
}

#[test]
fn a_projects_class_gives_values_their_types() {
    let level = level("bt-script-run-types");
    let mut members = bt_core::Properties::new();
    members.insert("wave".into(), Property::new(Value::Float(0.0)));
    let mut types = CustomTypes::new();
    types.add(CustomType::class(
        "spawn",
        ClassType {
            members,
            color: bt_core::Color::WHITE,
            draw_fill: false,
            use_as: ClassUsage::OBJECT,
        },
    ));
    let mut loader = bt_tmx::Loader::builder()
        .custom_types(Arc::new(types))
        .build();
    let script = parse_script("object spawn at 1,1 wave=3").unwrap();
    let input = RunInput::builder(Arc::new(level.map.clone()), &level.path).build();
    let result = run_script(&script, &input, &mut loader).unwrap();
    let spawn = &objects(&result.shown, "design")[0];
    assert_eq!(spawn.properties["wave"].value, Value::Float(3.0));
}

#[test]
fn objects_on_locked_layers_and_unknown_objects_refuse() {
    let level = level("bt-script-run-object-locks");
    let error = run(
        &level,
        "object spawn at 1,1\nlayer lock design\nmove #1 by 1,0",
    )
    .unwrap_err();
    assert_eq!(error.line, 3);
    assert!(matches!(*error.failure, RunFailure::Locked { .. }));
    let unknown = run(&level, "delete #9").unwrap_err();
    assert_eq!(
        unknown.failure.to_string(),
        "the map has no object #9 (bt objects MAP lists them)"
    );
}
