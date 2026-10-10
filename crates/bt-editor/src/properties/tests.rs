//! Custom properties of several targets together: rows with mixed and inherited values, and
//! every change applied to copies of a map and a tileset.

use std::sync::Arc;

use bt_core::{
    Cell, ClassType, ClassUsage, CustomType, CustomTypes, EnumStorage, EnumType, FileRef, Layer,
    LayerId, LayerKind, Map, MapObject, ObjectId, ObjectTemplate, Properties, Property, TileData,
    TileId, Tileset, TilesetSlot, Value,
};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use glam::UVec2;

use super::lookup::object_mut;
use super::{
    Change, ListEdit, Op, Origin, PropertyError, PropertySet, Shown, Step, Target, TargetId,
    ValueType,
};

fn int(value: i64) -> Property {
    Property::new(Value::Int(value))
}

fn float(value: f64) -> Property {
    Property::new(Value::Float(value))
}

fn text(value: &str) -> Property {
    Property::new(Value::String(value.into()))
}

fn list(items: Vec<Property>) -> Property {
    Property::new(Value::List(items))
}

fn class_value(class: &str, members: &[(&str, Property)]) -> Property {
    Property::typed(Value::Class(properties(members)), class)
}

fn properties(list: &[(&str, Property)]) -> Properties {
    list.iter()
        .map(|(name, property)| ((*name).to_owned(), property.clone()))
        .collect()
}

/// A path from its steps: numbers are list items, the rest names.
fn path(steps: &[&str]) -> Vec<Step> {
    steps
        .iter()
        .map(|step| match step.parse::<usize>() {
            Ok(index) => Step::Item(index),
            Err(_) => Step::Member((*step).to_owned()),
        })
        .collect()
}

/// The project: enums Kind (names) and Layers (flags stored as ints); classes Stats and Loot for
/// property values, and Enemy for objects, tiles, layers and maps.
fn types() -> CustomTypes {
    let mut types = CustomTypes::new();
    types.add(CustomType::enumeration(
        "Kind",
        EnumType {
            values: vec!["Small".into(), "Large".into()],
            ..EnumType::default()
        },
    ));
    types.add(CustomType::enumeration(
        "Layers",
        EnumType {
            storage: EnumStorage::Int,
            values: vec!["A".into(), "B".into(), "C".into()],
            values_as_flags: true,
        },
    ));
    types.add(CustomType::class(
        "Stats",
        ClassType {
            members: properties(&[("hp", int(10)), ("speed", float(1.5))]),
            use_as: ClassUsage::PROPERTY,
            ..ClassType::default()
        },
    ));
    types.add(CustomType::class(
        "Loot",
        ClassType {
            members: properties(&[
                ("gold", int(0)),
                ("stats", class_value("Stats", &[("hp", int(5))])),
            ]),
            use_as: ClassUsage::PROPERTY,
            ..ClassType::default()
        },
    ));
    types.add(CustomType::class(
        "Enemy",
        ClassType {
            members: properties(&[
                ("hp", int(10)),
                (
                    "kind",
                    Property::typed(Value::String("Small".into()), "Kind"),
                ),
                ("tags", list(Vec::new())),
            ]),
            use_as: ClassUsage::OBJECT | ClassUsage::TILE | ClassUsage::LAYER | ClassUsage::MAP,
            ..ClassType::default()
        },
    ));
    types
}

/// The map, the template its object 3 is an instance of, and the project's types.
struct World {
    map: Map,
    template: ObjectTemplate,
    types: CustomTypes,
}

/// A map whose tileset's tile 0 has an `art` file and 20 hp, with objects: 1, an enemy with 3
/// hp of its own; 2, a tile object of tile 0; 3, an instance of an enemy template with 30 hp and
/// loot, keeping the template's values and a speed of its own. Layer 1 holds the objects;
/// layer 2 is an enemy group with a `count` of “x”.
fn world() -> World {
    let atlas = Atlas::builder().build_embedded().unwrap();
    let mut tileset = (*atlas.tileset).clone();
    tileset.tiles.insert(
        TileId(0),
        TileData {
            properties: properties(&[
                ("art", Property::new(Value::File("a.png".into()))),
                ("hp", int(20)),
            ]),
            ..TileData::default()
        },
    );
    let objects = vec![
        MapObject {
            class: "Enemy".into(),
            properties: properties(&[("hp", int(3))]),
            ..MapObject::default()
        },
        MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            ..MapObject::default()
        },
        MapObject {
            class: "Enemy".into(),
            template: Some(FileRef::new("enemy.tx", "/enemy.tx")),
            properties: properties(&[
                ("hp", int(30)),
                ("loot", class_value("Loot", &[])),
                ("speed", float(2.0)),
            ]),
            ..MapObject::default()
        },
    ];
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
        .tileset(Arc::new(tileset))
        .object_layer("things", objects)
        .layer(Layer {
            class: "Enemy".into(),
            properties: properties(&[("count", text("x"))]),
            ..Layer::new(LayerId(0), "enemies", LayerKind::Group(Vec::new()))
        })
        .build()
        .unwrap();
    let template = ObjectTemplate {
        object: Some(MapObject {
            class: "Enemy".into(),
            properties: properties(&[("hp", int(30)), ("loot", class_value("Loot", &[]))]),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    World {
        map,
        template,
        types: types(),
    }
}

/// The properties of the objects `ids` of `map` (instances with the world's template).
fn objects<'a>(world: &'a World, map: &'a Map, ids: &[u32]) -> PropertySet<'a> {
    let targets = ids
        .iter()
        .map(|id| Target::object(map, ObjectId(*id), Some(&world.template), &world.types).unwrap())
        .collect();
    PropertySet::of(targets, &world.types)
}

/// The properties of the map and some of its layers.
fn map_and_layers<'a>(map: &'a Map, layers: &[u32], types: &'a CustomTypes) -> PropertySet<'a> {
    let mut targets = vec![Target::map(map, types)];
    targets.extend(
        layers
            .iter()
            .map(|id| Target::layer(map, LayerId(*id), types).unwrap()),
    );
    PropertySet::of(targets, types)
}

/// The row at `at`, among `rows` and their children.
fn row<'r>(rows: &'r [Shown], at: &[&str]) -> &'r Shown {
    let wanted = path(at);
    let mut level = rows;
    for depth in 1..=wanted.len() {
        let found = level
            .iter()
            .find(|row| row.path == wanted[..depth])
            .unwrap_or_else(|| panic!("no row {at:?}"));
        if depth == wanted.len() {
            return found;
        }
        level = &found.children;
    }
    panic!("no row {at:?}")
}

fn has_row(rows: &[Shown], name: &str) -> bool {
    rows.iter().any(|row| row.path == path(&[name]))
}

fn object(map: &Map, id: u32) -> &MapObject {
    map.object(ObjectId(id)).unwrap()
}

fn object_properties_mut(map: &mut Map, id: u32) -> &mut Properties {
    &mut object_mut(map, ObjectId(id)).unwrap().properties
}

fn layer_property(map: &Map, layer: u32, name: &str) -> Property {
    map.layer(LayerId(layer)).unwrap().properties[name].clone()
}

#[test]
fn rows_list_every_property_any_target_has_with_mixed_values() {
    let world = world();
    let rows = objects(&world, &world.map, &[1, 2, 3]).rows();
    let names: Vec<String> = rows.iter().map(|row| row.path[0].to_string()).collect();
    assert_eq!(names, ["art", "hp", "kind", "loot", "speed", "tags"]);
    let hp = row(&rows, &["hp"]);
    assert!(hp.mixed);
    assert_eq!(hp.value, int(3), "the first target's");
    assert_eq!(hp.holders, [0], "only object 1 sets hp itself");
    assert_eq!(
        hp.inherited_from,
        Some(Origin::Tile),
        "object 2 has it from its tile (object 3 from its template)"
    );
    assert!(hp.resettable, "object 1's own hp hides its class's");
    let art = row(&rows, &["art"]);
    assert!(art.mixed, "only the tile object has art");
    assert!(art.holders.is_empty());
    assert!(!art.resettable);
    assert!(row(&rows, &["kind"]).mixed, "object 2 has no class");
    let enemies = objects(&world, &world.map, &[1, 3]).rows();
    let kind = row(&enemies, &["kind"]);
    assert!(!kind.mixed, "both have it from the class Enemy");
    assert_eq!(kind.inherited_from, Some(Origin::Class("Enemy".into())));
    assert_eq!(
        kind.value,
        Property::typed(Value::String("Small".into()), "Kind")
    );
}

#[test]
fn values_inherited_from_a_template_and_a_tile_show_where_they_come_from() {
    let world = world();
    let rows = objects(&world, &world.map, &[3]).rows();
    let hp = row(&rows, &["hp"]);
    assert_eq!(hp.value, int(30));
    assert_eq!(hp.inherited_from, Some(Origin::Template));
    assert!(hp.holders.is_empty(), "the kept value is the template's");
    assert!(!hp.resettable);
    let speed = row(&rows, &["speed"]);
    assert_eq!(speed.holders, [0]);
    assert_eq!(speed.inherited_from, None);
    let rows = objects(&world, &world.map, &[2]).rows();
    assert_eq!(row(&rows, &["art"]).inherited_from, Some(Origin::Tile));
    assert_eq!(row(&rows, &["hp"]).value, int(20));
}

#[test]
fn class_values_expand_to_members_with_unset_ones_at_the_class_default() {
    let world = world();
    let rows = objects(&world, &world.map, &[3]).rows();
    let loot = row(&rows, &["loot"]);
    assert_eq!(
        loot.value,
        class_value(
            "Loot",
            &[
                ("gold", int(0)),
                (
                    "stats",
                    class_value("Stats", &[("hp", int(5)), ("speed", float(1.5))])
                )
            ]
        ),
        "every member filled in"
    );
    let gold = row(&rows, &["loot", "gold"]);
    assert!(gold.is_default);
    assert_eq!(gold.value, int(0));
    assert!(!gold.resettable);
    let stats_hp = row(&rows, &["loot", "stats", "hp"]);
    assert_eq!(stats_hp.value, int(5), "Loot's default for its stats");
    assert!(stats_hp.is_default);
    let speed = row(&rows, &["loot", "stats", "speed"]);
    assert_eq!(speed.value, float(1.5), "Stats's own default");
}

#[test]
fn lists_show_their_items_and_differ_where_lengths_do() {
    let world = world();
    let mut map = world.map.clone();
    map.properties.insert("zones".into(), list(vec![int(1)]));
    map.layer_mut(LayerId(2))
        .unwrap()
        .properties
        .insert("zones".into(), list(vec![int(1), text("b")]));
    let rows = map_and_layers(&map, &[2], &world.types).rows();
    let zones = row(&rows, &["zones"]);
    assert!(zones.mixed);
    assert_eq!(zones.children.len(), 2);
    let first = row(&rows, &["zones", "0"]);
    assert!(!first.mixed);
    assert_eq!(first.holders, [0, 1]);
    let second = row(&rows, &["zones", "1"]);
    assert!(second.mixed, "the map's list has no second item");
    assert_eq!(second.value, text("b"));
    assert_eq!(second.holders, [1]);
}

#[test]
fn reset_brings_back_the_inherited_value() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 3]);
    let mut reset = world.map.clone();
    assert_eq!(
        set.apply_to_map(&mut reset, &Change::reset(path(&["hp"]))),
        Ok(true)
    );
    assert!(
        !object(&reset, 1).properties.contains_key("hp"),
        "the class's 10 shows"
    );
    assert_eq!(object(&reset, 3).properties["hp"], int(30), "unchanged");
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &Change::reset(path(&["speed"]))),
        Ok(false),
        "a value with nothing inherited under it stays"
    );

    // An instance overriding its template gets the template's value back.
    let mut overridden = world.map.clone();
    let set_hp = Change::set(path(&["hp"]), int(40));
    assert_eq!(set.apply_to_map(&mut overridden, &set_hp), Ok(true));
    assert_eq!(object(&overridden, 3).properties["hp"], int(40));
    let set = objects(&world, &overridden, &[3]);
    assert!(row(&set.rows(), &["hp"]).resettable);
    let mut reset = overridden.clone();
    assert_eq!(
        set.apply_to_map(&mut reset, &Change::reset(path(&["hp"]))),
        Ok(true)
    );
    assert_eq!(object(&reset, 3).properties["hp"], int(30));
}

#[test]
fn a_value_typed_over_several_targets_keeps_each_ones_type() {
    let world = world();
    let mut map = world.map.clone();
    map.properties.insert("count".into(), int(3));
    let set = map_and_layers(&map, &[2], &world.types);
    assert!(row(&set.rows(), &["count"]).mixed);

    let mut copy = map.clone();
    let seven = Change::set(path(&["count"]), text("7"));
    assert_eq!(set.apply_to_map(&mut copy, &seven), Ok(true));
    assert_eq!(copy.properties["count"], int(7));
    assert_eq!(layer_property(&copy, 2, "count"), text("7"));

    let mut copy = map.clone();
    let seven = Change::set(path(&["count"]), int(7));
    assert_eq!(set.apply_to_map(&mut copy, &seven), Ok(true));
    assert_eq!(copy.properties["count"], int(7));
    assert_eq!(layer_property(&copy, 2, "count"), text("7"));

    let mut copy = map.clone();
    let word = Change::set(path(&["count"]), text("seven"));
    assert_eq!(set.apply_to_map(&mut copy, &word), Ok(true));
    assert_eq!(
        copy.properties["count"],
        text("seven"),
        "seven is no int: the map's value becomes text"
    );
}

#[test]
fn setting_a_property_some_targets_lack_gives_it_to_them() {
    let world = world();
    let art = Property::new(Value::File("b.png".into()));
    let mut copy = world.map.clone();
    let set = objects(&world, &world.map, &[1, 2]);
    assert_eq!(
        set.apply_to_map(&mut copy, &Change::set(path(&["art"]), art.clone())),
        Ok(true)
    );
    assert_eq!(object(&copy, 1).properties["art"], art);
    assert_eq!(
        object(&copy, 2).properties["art"],
        art,
        "the tile object now overrides its tile's"
    );

    // A member set where one target lacks the class value gives it one with that member.
    let mut copy = world.map.clone();
    let set = objects(&world, &world.map, &[3, 1]);
    let gold = Change::set(path(&["loot", "gold"]), int(7));
    assert_eq!(set.apply_to_map(&mut copy, &gold), Ok(true));
    let expected = class_value("Loot", &[("gold", int(7))]);
    assert_eq!(object(&copy, 3).properties["loot"], expected);
    assert_eq!(object(&copy, 1).properties["loot"], expected);
}

#[test]
fn members_set_start_from_their_class_defaults_and_reset_unsets_them() {
    let world = world();
    let mut speedy = world.map.clone();
    let speed = Change::set(path(&["loot", "stats", "speed"]), text("4"));
    assert_eq!(
        objects(&world, &world.map, &[3]).apply_to_map(&mut speedy, &speed),
        Ok(true)
    );
    assert_eq!(
        object(&speedy, 3).properties["loot"],
        class_value(
            "Loot",
            &[(
                "stats",
                class_value("Stats", &[("hp", int(5)), ("speed", float(4.0))])
            )]
        ),
        "the stats start from Loot's default for them; the text keeps the float's type"
    );
    let set = objects(&world, &speedy, &[3]);
    let rows = set.rows();
    let shown = row(&rows, &["loot", "stats", "speed"]);
    assert!(!shown.is_default && shown.resettable);
    assert_eq!(shown.holders, [0]);

    let mut reset = speedy.clone();
    let unset = Change::reset(path(&["loot", "stats", "speed"]));
    assert_eq!(set.apply_to_map(&mut reset, &unset), Ok(true));
    assert_eq!(
        object(&reset, 3).properties["loot"],
        class_value(
            "Loot",
            &[("stats", class_value("Stats", &[("hp", int(5))]))]
        )
    );

    let set = objects(&world, &reset, &[3]);
    let mut emptied = reset.clone();
    let unset = Change::reset(path(&["loot", "stats", "hp"]));
    assert_eq!(set.apply_to_map(&mut emptied, &unset), Ok(true));
    assert_eq!(
        object(&emptied, 3).properties["loot"],
        class_value("Loot", &[]),
        "a class value inside another that has no member left is unset"
    );
    let unset = Change::reset(path(&["loot", "gold"]));
    assert_eq!(
        set.apply_to_map(&mut reset.clone(), &unset),
        Ok(false),
        "gold is not set"
    );
}

#[test]
fn list_edits_apply_to_each_targets_list_by_index() {
    let world = world();
    let mut map = world.map.clone();
    map.properties
        .insert("zones".into(), list(vec![int(1), int(2), int(3)]));
    map.layer_mut(LayerId(2))
        .unwrap()
        .properties
        .insert("zones".into(), list(vec![int(9)]));
    let set = map_and_layers(&map, &[2, 1], &world.types);
    let edit = |edit: ListEdit| {
        let mut copy = map.clone();
        let changed = set.apply_to_map(&mut copy, &Change::list(path(&["zones"]), edit));
        assert_eq!(changed, Ok(true));
        copy
    };

    let pushed = edit(ListEdit::Push(text("new")));
    assert_eq!(
        pushed.properties["zones"],
        list(vec![int(1), int(2), int(3), text("new")])
    );
    assert_eq!(
        layer_property(&pushed, 2, "zones"),
        list(vec![int(9), text("new")])
    );
    assert_eq!(
        layer_property(&pushed, 1, "zones"),
        list(vec![text("new")]),
        "a layer without the list gets one"
    );

    let removed = edit(ListEdit::Remove(1));
    assert_eq!(removed.properties["zones"], list(vec![int(1), int(3)]));
    assert_eq!(
        layer_property(&removed, 2, "zones"),
        list(vec![int(9)]),
        "the layer's list has no second item"
    );

    let moved = edit(ListEdit::Move { from: 0, to: 2 });
    assert_eq!(
        moved.properties["zones"],
        list(vec![int(2), int(3), int(1)])
    );

    let duplicated = edit(ListEdit::Duplicate(0));
    assert_eq!(
        duplicated.properties["zones"],
        list(vec![int(1), int(1), int(2), int(3)])
    );
    assert_eq!(
        layer_property(&duplicated, 2, "zones"),
        list(vec![int(9), int(9)])
    );

    let mut item = map.clone();
    let set_item = Change::set(path(&["zones", "2"]), text("30"));
    assert_eq!(set.apply_to_map(&mut item, &set_item), Ok(true));
    assert_eq!(
        item.properties["zones"],
        list(vec![int(1), int(2), int(30)]),
        "an item keeps its type"
    );
    assert_eq!(
        layer_property(&item, 2, "zones"),
        list(vec![int(9)]),
        "a list without that item is left alone"
    );
    let out_of_range = Change::list(path(&["zones"]), ListEdit::Remove(5));
    assert_eq!(set.apply_to_map(&mut map.clone(), &out_of_range), Ok(false));
}

#[test]
fn add_gives_a_property_to_the_targets_without_it() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 2, 3]);
    let mut copy = world.map.clone();
    assert_eq!(
        set.apply_to_map(&mut copy, &Change::add(" score ", ValueType::Int)),
        Ok(true)
    );
    for id in [1, 2, 3] {
        assert_eq!(object(&copy, id).properties["score"], int(0));
    }
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &Change::add("hp", ValueType::Float)),
        Ok(false),
        "every target has hp, its own or inherited"
    );
    let mut copy = world.map.clone();
    let size = Change::add("size", ValueType::Custom("Kind".into()));
    assert_eq!(set.apply_to_map(&mut copy, &size), Ok(true));
    assert_eq!(
        object(&copy, 1).properties["size"],
        Property::typed(Value::String("Small".into()), "Kind")
    );
    let enemy = Change::add("x", ValueType::Custom("Enemy".into()));
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &enemy),
        Err(PropertyError::UnknownType("Enemy".into())),
        "Enemy is not a type of property values"
    );
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &Change::add("  ", ValueType::Int)),
        Err(PropertyError::EmptyName)
    );
}

#[test]
fn remove_takes_away_own_values_only() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 3]);
    let mut copy = world.map.clone();
    assert_eq!(
        set.apply_to_map(&mut copy, &Change::remove("speed")),
        Ok(true)
    );
    assert!(!object(&copy, 3).properties.contains_key("speed"));
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &Change::remove("kind")),
        Ok(false),
        "kind is inherited from the class: nothing to remove"
    );
    let mut copy = world.map.clone();
    assert_eq!(set.apply_to_map(&mut copy, &Change::remove("hp")), Ok(true));
    assert!(!object(&copy, 1).properties.contains_key("hp"));
    assert_eq!(
        object(&copy, 3).properties["hp"],
        int(30),
        "the instance keeps its template's value"
    );
}

#[test]
fn rename_moves_own_values_and_refuses_to_replace_a_property() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 3]);
    let mut copy = world.map.clone();
    assert_eq!(
        set.apply_to_map(&mut copy, &Change::rename("hp", "health")),
        Ok(true)
    );
    assert_eq!(object(&copy, 1).properties["health"], int(3));
    assert!(!object(&copy, 1).properties.contains_key("hp"));
    assert!(
        !object(&copy, 3).properties.contains_key("health"),
        "object 3's hp is its template's"
    );
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &Change::rename("speed", "speed")),
        Ok(false)
    );

    let mut both = world.map.clone();
    object_properties_mut(&mut both, 3).insert("hp".into(), int(31));
    object_properties_mut(&mut both, 3).insert("health".into(), int(1));
    let set = objects(&world, &both, &[3]);
    assert_eq!(
        set.apply_to_map(&mut both.clone(), &Change::rename("hp", "health")),
        Err(PropertyError::NameTaken("health".into()))
    );
    assert_eq!(
        set.apply_to_map(&mut both.clone(), &Change::rename("hp", " ")),
        Err(PropertyError::EmptyName)
    );
    let mut renamed = both.clone();
    assert_eq!(
        set.apply_to_map(&mut renamed, &Change::rename("hp", "life")),
        Ok(true)
    );
    assert_eq!(object(&renamed, 3).properties["life"], int(31));
    assert_eq!(
        object(&renamed, 3).properties["hp"],
        int(30),
        "the template's hp shows again"
    );
}

#[test]
fn convert_changes_own_values_that_convert() {
    let world = world();
    let mut map = world.map.clone();
    map.properties.insert("count".into(), text("12"));
    let set = map_and_layers(&map, &[2], &world.types);
    let mut copy = map.clone();
    assert_eq!(
        set.apply_to_map(&mut copy, &Change::convert("count", ValueType::Int)),
        Ok(true)
    );
    assert_eq!(copy.properties["count"], int(12));
    assert_eq!(layer_property(&copy, 2, "count"), text("x"), "x is no int");

    let mut flags = map.clone();
    flags.properties.insert("mask".into(), text("A,C"));
    let set = map_and_layers(&flags, &[], &world.types);
    let mut converted = flags.clone();
    let to_layers = Change::convert("mask", ValueType::Custom("Layers".into()));
    assert_eq!(set.apply_to_map(&mut converted, &to_layers), Ok(true));
    assert_eq!(
        converted.properties["mask"],
        Property::typed(Value::Int(5), "Layers"),
        "names to flags, in the enum's storage form"
    );
    let set = map_and_layers(&converted, &[], &world.types);
    let mut back = converted.clone();
    let to_text = Change::convert("mask", ValueType::String);
    assert_eq!(set.apply_to_map(&mut back, &to_text), Ok(true));
    assert_eq!(back.properties["mask"], text("A,C"));
}

#[test]
fn enum_values_are_stored_in_their_enums_form() {
    let world = world();
    let mut map = world.map.clone();
    map.properties.insert(
        "size".into(),
        Property::typed(Value::String("Small".into()), "Kind"),
    );
    map.properties
        .insert("mask".into(), Property::typed(Value::Int(1), "Layers"));
    let set = map_and_layers(&map, &[], &world.types);

    let mut copy = map.clone();
    let by_index = Change::set(path(&["size"]), Property::typed(Value::Int(1), "Kind"));
    assert_eq!(set.apply_to_map(&mut copy, &by_index), Ok(true));
    assert_eq!(
        copy.properties["size"],
        Property::typed(Value::String("Large".into()), "Kind"),
        "Kind is stored by name"
    );

    let mut copy = map.clone();
    let by_names = Change::set(
        path(&["mask"]),
        Property::typed(Value::String("B,C".into()), "Layers"),
    );
    assert_eq!(set.apply_to_map(&mut copy, &by_names), Ok(true));
    assert_eq!(
        copy.properties["mask"],
        Property::typed(Value::Int(6), "Layers"),
        "Layers is stored as flags"
    );

    let mut copy = map.clone();
    let typed = Change::set(path(&["size"]), text("Large"));
    assert_eq!(set.apply_to_map(&mut copy, &typed), Ok(true));
    assert_eq!(
        copy.properties["size"],
        Property::typed(Value::String("Large".into()), "Kind"),
        "a name typed over an enum value keeps the enum"
    );
}

#[test]
fn paste_sets_the_copied_properties_on_every_target() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 2]);
    let pasted = properties(&[("hp", int(9)), ("note", text("hi"))]);
    let change = Change::paste(pasted);
    assert_eq!(change.label(), "Paste Properties");
    let mut copy = world.map.clone();
    assert_eq!(set.apply_to_map(&mut copy, &change), Ok(true));
    for id in [1, 2] {
        assert_eq!(object(&copy, id).properties["hp"], int(9));
        assert_eq!(object(&copy, id).properties["note"], text("hi"));
    }
    let set = objects(&world, &copy, &[1, 2]);
    assert_eq!(
        set.apply_to_map(&mut copy.clone(), &change),
        Ok(false),
        "pasted already"
    );
}

#[test]
fn tiles_and_the_tileset_change_in_a_tileset() {
    let world = world();
    let types = &world.types;
    let tileset: Tileset = (*Atlas::builder().build_embedded().unwrap().tileset).clone();
    let set = PropertySet::of(
        vec![
            Target::tile(&tileset, TileId(3), types).unwrap(),
            Target::tile(&tileset, TileId(5), types).unwrap(),
            Target::tileset(&tileset, types),
        ],
        types,
    );
    let mut copy = tileset.clone();
    let solid = Change::add("solid", ValueType::Bool);
    assert_eq!(set.apply_to_tileset(&mut copy, &solid), Ok(true));
    let tile = copy.tile(TileId(3)).unwrap();
    assert_eq!(
        tile.properties["solid"],
        Property::new(Value::Bool(false)),
        "an atlas tile without data gets some"
    );
    assert_eq!(tile.probability, 1.0);
    assert!(copy.properties.contains_key("solid"));
    assert_eq!(
        set.apply_to_tileset(&mut tileset.clone(), &Change::remove("solid")),
        Ok(false)
    );
    assert_eq!(
        set.apply_to_map(&mut world.map.clone(), &solid),
        Err(PropertyError::NotInMap(TargetId::Tile(TileId(3))))
    );
    let map_set = map_and_layers(&world.map, &[], types);
    assert_eq!(
        map_set.apply_to_tileset(&mut tileset.clone(), &solid),
        Err(PropertyError::NotInTileset(TargetId::Map))
    );
}

#[test]
fn classes_count_only_for_what_they_are_used_as() {
    let world = world();
    let mut map = world.map.clone();
    map.class = "Enemy".into();
    assert!(
        has_row(&map_and_layers(&map, &[], &world.types).rows(), "kind"),
        "Enemy is a class for maps"
    );
    map.class = "Stats".into();
    assert!(
        !has_row(&map_and_layers(&map, &[], &world.types).rows(), "speed"),
        "Stats is only a class for property values"
    );
}

#[test]
fn changes_that_do_not_suit_their_path_are_refused() {
    let world = world();
    let set = objects(&world, &world.map, &[1]);
    let refused = |change: Change| {
        set.apply_to_map(&mut world.map.clone(), &change)
            .unwrap_err()
    };
    assert_eq!(
        refused(Change {
            path: path(&["loot", "gold"]),
            op: Op::Remove,
        }),
        PropertyError::PathMismatch {
            operation: "Remove",
            path: "loot.gold".into()
        }
    );
    assert_eq!(
        refused(Change::reset(path(&["tags", "0"]))),
        PropertyError::PathMismatch {
            operation: "Reset",
            path: "tags[0]".into()
        }
    );
    assert_eq!(
        refused(Change {
            path: path(&["hp"]),
            op: Op::Paste(Properties::new()),
        }),
        PropertyError::PathMismatch {
            operation: "Paste",
            path: "hp".into()
        }
    );
    assert_eq!(
        refused(Change::set(Vec::new(), int(1))),
        PropertyError::PathMismatch {
            operation: "Set",
            path: String::new()
        }
    );
    assert_eq!(
        Change::set(path(&["loot", "gold"]), int(1)).label(),
        "Set Property 'loot.gold'"
    );
    assert_eq!(Change::remove("hp").label(), "Remove Property 'hp'");
}

#[test]
fn a_target_given_twice_counts_once() {
    let world = world();
    let set = objects(&world, &world.map, &[1, 1]);
    assert_eq!(set.targets().len(), 1);
    let mut copy = world.map.clone();
    let push = Change::list(path(&["tags"]), ListEdit::Push(int(1)));
    assert_eq!(set.apply_to_map(&mut copy, &push), Ok(true));
    assert_eq!(
        object(&copy, 1).properties["tags"],
        list(vec![int(1)]),
        "the class's empty list, pushed to once"
    );
}
