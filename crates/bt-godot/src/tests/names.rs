//! Node identity (docs/design/12-godot-export.md): names and `unique_id`s from Tiled ids, the Godot 4.6 gate, the
//! rename report, and what a level scene that inherits an export keeps when the map changes.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_core::{
    CustomTypes, ImageLayer, Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Shape,
    TileLayer,
};
use glam::DVec2;
use serde_json::Value;

use super::super::report::{Element, Renamed};
use super::super::{names, read, version};
use super::support::*;
use super::*;

/// A project.godot for Godot `features` (none: no `config/features` line).
fn project_for(name: &str, features: Option<&str>) -> std::path::PathBuf {
    let dir = project(name);
    let features = features.map_or(String::new(), |v| {
        format!("config/features=PackedStringArray(\"{v}\", \"Forward Plus\")\n")
    });
    std::fs::write(
        dir.join("project.godot"),
        format!("config_version=5\n\n[application]\n\nconfig/name=\"bt-test\"\n{features}"),
    )
    .unwrap();
    dir
}

/// A small deterministic random source (xorshift64*), so that a failure repeats.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % n.max(1)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

#[test]
fn siblings_are_named_by_id_not_by_order() {
    let wanted = |list: &[(u32, &str)]| {
        list.iter()
            .map(|&(id, name)| (id, name.to_owned()))
            .collect::<Vec<_>>()
    };
    // The lowest id keeps a shared name, the others add their id, and again if that is taken.
    assert_eq!(
        names::siblings(&wanted(&[(7, "a"), (3, "a"), (5, "a_7")])),
        ["a_7_7", "a", "a_7"]
    );
    assert_eq!(
        names::siblings(&wanted(&[(2, "b"), (1, "b"), (4, "b")])),
        ["b_2", "b", "b_4"]
    );

    let o = |id: u32, name: &str, class: &str, shape: Shape| MapObject {
        id: bt_core::ObjectId(id),
        name: name.to_owned(),
        class: class.to_owned(),
        shape,
        ..MapObject::default()
    };
    let tile = MapObject {
        tile: Some(bt_core::Cell::new(
            bt_core::TilesetSlot::new(0).unwrap(),
            bt_core::TileId(0),
        )),
        ..o(9, "", "", Shape::Rectangle)
    };
    let objects = [
        o(197, "", "", Shape::Rectangle),
        o(195, "", "Chest", Shape::Rectangle),
        o(3, " a.b ", "", Shape::Point),
        o(4, "", " ", Shape::Ellipse),
        tile,
    ];
    assert_eq!(
        names::object_names(&objects.iter().collect::<Vec<_>>()),
        ["rectangle_197", "Chest_195", "a_b", "ellipse_4", "sprite_9"]
    );
    let layer = |id: u32, name: &str| {
        Layer::new(LayerId(id), name, LayerKind::Object(ObjectLayer::default()))
    };
    assert_eq!(
        names::layer_names(&[
            layer(4, ""),
            layer(2, "Tiles"),
            layer(5, "Tiles"),
            layer(1, "layer_4")
        ]),
        ["layer_4_4", "Tiles", "Tiles_5", "layer_4"]
    );
}

#[test]
fn unique_ids_come_from_tiled_ids_and_hashes() {
    let on = || {
        let mut ids = names::Names::default();
        ids.unique_ids = true;
        ids
    };
    let mut ids = on();
    assert_eq!(
        (ids.root(), ids.object(195), ids.layer(3)),
        (Some(1), Some((1 << 29) + 195), Some((1 << 30) + 3))
    );
    let shape = ids.helper(Some((1 << 29) + 195), "shape", 0);
    assert_eq!(
        on().helper(Some((1 << 29) + 195), "shape", 0),
        shape,
        "the same in every export"
    );
    // Ids from 2^28 on would reach the next range: hashed like helpers, from 3·2^29 up.
    let big = [ids.object(1 << 28), ids.layer(u32::MAX)].map(Option::unwrap);
    assert!(big.iter().all(|&id| id >= 3 << 29), "{big:?}");
    // Enough helpers for hashes to clash: salted again, every one is still different.
    let helpers: HashSet<i32> = (0..100_000)
        .map(|n| ids.helper(Some(1), "shape", n).unwrap())
        .collect();
    assert_eq!(helpers.len(), 100_000);
    assert!(
        helpers.iter().all(|&id| id >= 3 << 29)
            && !helpers.contains(&shape.unwrap())
            && !big.iter().any(|id| helpers.contains(id))
    );
    let mut off = names::Names::default();
    assert_eq!(
        (
            off.root(),
            off.object(1),
            off.layer(1),
            off.helper(Some(1), "shape", 0)
        ),
        (None, None, None, None),
        "no unique_ids before Godot 4.6"
    );
}

/// Property: random adds (always the highest id, as in Tiled), deletes and reorders. Siblings
/// never share a name; adds and reorders rename nobody; a delete can only shorten a name that
/// had ids added (a sibling with the same name gone), never take a plain name away.
#[test]
fn sibling_names_survive_adds_deletes_and_reorders() {
    let pool = [
        "door",
        "door",
        "lamp",
        "door_3",
        "lamp_7",
        "door_3_9",
        "rectangle_5",
        "x",
    ];
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let (mut siblings, mut next, mut before) = (
            Vec::<(u32, String)>::new(),
            1,
            HashMap::<u32, String>::new(),
        );
        for _ in 0..40 {
            let op = rng.below(3);
            match op {
                0 => {
                    let at = rng.below(siblings.len() + 1);
                    siblings.insert(at, (next, pool[rng.below(pool.len())].to_owned()));
                    next += 1;
                }
                1 if !siblings.is_empty() => drop(siblings.remove(rng.below(siblings.len()))),
                _ => rng.shuffle(&mut siblings),
            }
            let names = names::siblings(&siblings);
            assert_eq!(
                names.iter().collect::<HashSet<_>>().len(),
                names.len(),
                "seed {seed}: {names:?}"
            );
            for ((id, wanted), name) in siblings.iter().zip(&names) {
                let added = name
                    .strip_prefix(wanted.as_str())
                    .unwrap_or_else(|| panic!("seed {seed}: {name} for {wanted}"));
                assert!(
                    added.is_empty() || added.split(&format!("_{id}")).all(str::is_empty),
                    "seed {seed}: {name} for {id} {wanted}"
                );
                match before.get(id) {
                    Some(old) if op == 1 => assert!(
                        old.starts_with(name.as_str()) && (old == name || old != wanted),
                        "seed {seed}: {old} became {name}"
                    ),
                    Some(old) => assert_eq!(old, name, "seed {seed}"),
                    None => {}
                }
            }
            before = siblings.iter().map(|(id, _)| *id).zip(names).collect();
        }
    }
}

/// What a scene says about each layer and object: its node's path, name and unique_id, and the
/// id of its helper (a layer's Parallax2D holder, an area's shape).
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    path: String,
    name: String,
    unique_id: Option<i32>,
    helper: Option<i32>,
}

fn identities(tscn: &str) -> (HashMap<u32, Seen>, HashMap<u32, Seen>, Vec<i32>) {
    let scene = read::parse(tscn).unwrap();
    let nodes: Vec<(read::Node, &read::Section)> = scene.nodes().collect();
    let by_path: HashMap<String, &read::Node> = nodes.iter().map(|(n, _)| (n.path(), n)).collect();
    let (mut layers, mut objects) = (HashMap::new(), HashMap::new());
    for (node, section) in &nodes {
        let id = |key| section.get(key).and_then(read::int).map(|id| id as u32);
        let parent = node.parent.as_deref().and_then(|p| by_path.get(p));
        if let Some(id) = id("metadata/layer_id") {
            let holder =
                parent.filter(|p| p.class.as_deref() == Some("Parallax2D") && p.name == node.name);
            layers.insert(
                id,
                Seen {
                    path: node.path(),
                    name: node.name.clone(),
                    unique_id: node.unique_id,
                    helper: holder.and_then(|h| h.unique_id),
                },
            );
        } else if let Some(id) = id("metadata/id") {
            let shape = by_path
                .get(&format!("{}/Shape", node.path()))
                .and_then(|s| s.unique_id);
            objects.insert(
                id,
                Seen {
                    path: node.path(),
                    name: node.name.clone(),
                    unique_id: node.unique_id,
                    helper: shape,
                },
            );
        }
    }
    (
        layers,
        objects,
        nodes.iter().filter_map(|(n, _)| n.unique_id).collect(),
    )
}

fn objects_of(layers: &mut [Layer]) -> Vec<&mut Vec<MapObject>> {
    let mut out = Vec::new();
    for layer in layers {
        match &mut layer.kind {
            LayerKind::Object(o) => out.push(Arc::make_mut(&mut o.objects)),
            LayerKind::Group(children) => out.extend(objects_of(children)),
            _ => {}
        }
    }
    out
}

/// Property, on whole scenes: random edits of a map (objects added, deleted, reordered and moved
/// to another layer; layers added, deleted and reordered, at the top and inside a group). Every
/// surviving layer and object keeps its node's name, its unique_id and its helper's, unique_ids
/// never repeat, and the rename report names exactly the objects that moved.
#[test]
fn map_edits_never_rename_or_renumber_what_survives() {
    let dir = project_for("identity", Some("4.7"));
    let target = dir.join("levels/edited.tscn");
    for seed in 1..=4u64 {
        let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03));
        let (mut next_layer, mut next_object) = (1u32, 1u32);
        let mut object = |rng: &mut Rng| {
            let id = next_object;
            next_object += 1;
            let (name, class) = match rng.below(4) {
                0 => (format!("thing{id}"), ""),
                1 => (String::new(), "Chest"),
                _ => (String::new(), ""),
            };
            let shape = [
                Shape::Point,
                Shape::Ellipse,
                Shape::Rectangle,
                Shape::Rectangle,
            ][rng.below(4)]
            .clone();
            MapObject {
                id: bt_core::ObjectId(id),
                name,
                class: class.to_owned(),
                shape,
                size: DVec2::new(16.0, 8.0),
                ..MapObject::default()
            }
        };
        let mut layer = |rng: &mut Rng, kind: LayerKind| {
            let id = next_layer;
            next_layer += 1;
            let mut layer = Layer::new(
                LayerId(id),
                if rng.below(3) == 0 {
                    String::new()
                } else {
                    format!("layer{id}")
                },
                kind,
            );
            if rng.below(3) == 0 {
                layer.parallax = DVec2::new(0.5, 1.0);
            }
            layer
        };
        let object_layer = |rng: &mut Rng, object: &mut dyn FnMut(&mut Rng) -> MapObject| {
            let list: Vec<MapObject> = (0..3).map(|_| object(rng)).collect();
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(list),
                ..ObjectLayer::default()
            })
        };
        let mut map = Map {
            width: 4,
            height: 4,
            tile_width: 16,
            tile_height: 16,
            ..Map::default()
        };
        map.layers.push(layer(
            &mut rng,
            LayerKind::Tile(TileLayer::finite(glam::UVec2::new(4, 4))),
        ));
        let kind = object_layer(&mut rng, &mut object);
        map.layers.push(layer(&mut rng, kind));
        let mut children = Vec::new();
        for _ in 0..2 {
            let kind = object_layer(&mut rng, &mut object);
            children.push(layer(&mut rng, kind));
        }
        let mut group = layer(&mut rng, LayerKind::Group(Vec::new()));
        group.parallax = DVec2::ONE;
        group.kind = LayerKind::Group(children);
        map.layers.push(group);
        map.layers
            .push(layer(&mut rng, LayerKind::Image(ImageLayer::default())));

        let mut before: Option<(HashMap<u32, Seen>, HashMap<u32, Seen>)> = None;
        let mut moved = None;
        for step in 0..20 {
            let report = export_map(
                &map,
                &dir.join("maps/edited.tmx"),
                &CustomTypes::default(),
                &target,
                &Options::default(),
            )
            .unwrap();
            let (layers, objects, ids) = identities(&std::fs::read_to_string(&target).unwrap());
            assert_eq!(
                ids.iter().collect::<HashSet<_>>().len(),
                ids.len(),
                "seed {seed} step {step}: a unique_id repeats"
            );
            for (id, seen) in &layers {
                assert_eq!(seen.unique_id, Some((1 << 30) + *id as i32));
            }
            for (id, seen) in &objects {
                assert_eq!(seen.unique_id, Some((1 << 29) + *id as i32));
            }
            if let Some((layers_before, objects_before)) = &before {
                for (id, seen) in &layers {
                    if let Some(old) = layers_before.get(id) {
                        assert_eq!(old, seen, "seed {seed} step {step}: layer {id}");
                    }
                }
                for (id, seen) in &objects {
                    if let Some(old) = objects_before.get(id) {
                        assert_eq!(
                            (&old.name, old.unique_id, old.helper),
                            (&seen.name, seen.unique_id, seen.helper),
                            "seed {seed} step {step}: object {id}"
                        );
                        assert!(
                            old.path == seen.path || moved == Some(*id),
                            "seed {seed} step {step}: object {id} moved from {} to {}",
                            old.path,
                            seen.path
                        );
                    }
                }
                let renamed: Vec<Element> = report.renamed.iter().map(|r| r.element).collect();
                let expected: Vec<Element> = moved
                    .filter(|id| objects_before[id].path != objects[id].path)
                    .map(Element::Object)
                    .into_iter()
                    .collect();
                assert_eq!(renamed, expected, "seed {seed} step {step}");
            }
            before = Some((layers, objects));

            // The next edit.
            moved = None;
            let mut lists = objects_of(&mut map.layers);
            let from = rng.below(lists.len());
            match rng.below(6) {
                0 => {
                    let at = rng.below(lists[from].len() + 1);
                    let o = object(&mut rng);
                    lists[from].insert(at, o);
                }
                1 if !lists[from].is_empty() => {
                    let at = rng.below(lists[from].len());
                    lists[from].remove(at);
                }
                2 if !lists[from].is_empty() => {
                    let at = rng.below(lists[from].len());
                    let o = lists[from].remove(at);
                    moved = Some(o.id.0);
                    let to = rng.below(lists.len());
                    let at = rng.below(lists[to].len() + 1);
                    lists[to].insert(at, o);
                }
                3 => rng.shuffle(lists[from].as_mut_slice()),
                4 => {
                    let kind = object_layer(&mut rng, &mut object);
                    let new = layer(&mut rng, kind);
                    let list = if rng.below(2) == 0 {
                        &mut map.layers
                    } else {
                        group_children(&mut map.layers)
                    };
                    let at = rng.below(list.len() + 1);
                    list.insert(at, new);
                }
                _ => {
                    let list = if rng.below(2) == 0 {
                        &mut map.layers
                    } else {
                        group_children(&mut map.layers)
                    };
                    if rng.below(2) == 0 && list.len() > 1 {
                        let at = rng.below(list.len());
                        if !matches!(list[at].kind, LayerKind::Group(_)) {
                            list.remove(at);
                        }
                    } else {
                        rng.shuffle(list);
                    }
                }
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn group_children(layers: &mut [Layer]) -> &mut Vec<Layer> {
    layers
        .iter_mut()
        .find_map(|l| {
            if let LayerKind::Group(children) = &mut l.kind {
                Some(children)
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn unique_ids_need_a_project_for_godot_4_6_or_later() {
    let features = |text: &str| version::features(text);
    assert_eq!(
        features(
            "config_version=5\n\n[application]\n\nconfig/features=PackedStringArray(\"4.7\", \"Forward Plus\")\n"
        ),
        Some((4, 7))
    );
    assert_eq!(
        features("[application]\nconfig/features=PackedStringArray(\"C#\", \"4.10\")\n"),
        Some((4, 10))
    );
    assert_eq!(
        features("[application]\nconfig/name=\"x\"\n"),
        None,
        "no config/features"
    );
    assert_eq!(
        features("[rendering]\nconfig/features=PackedStringArray(\"4.7\")\n"),
        None,
        "another section"
    );
    assert_eq!(
        features("config_version=4\n\n[application]\n\nconfig/name=\"godot 3\"\n"),
        None
    );

    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="3" nextobjectid="2">
 <objectgroup id="2" name="things">
  <object id="1" name="door" x="0" y="0" width="16" height="16"/>
 </objectgroup>
</map>
"#;
    for (features, ids) in [
        (None, false),
        (Some("4.5"), false),
        (Some("4.6"), true),
        (Some("4.7"), true),
    ] {
        let dir = project_for(&format!("gate-{}", features.unwrap_or("none")), features);
        std::fs::write(dir.join("maps/gate.tmx"), map).unwrap();
        let target = dir.join("gate.tscn");
        export(
            &dir.join("maps/gate.tmx"),
            &mut bt_tmx::Loader::builder().build(),
            &target,
            &Options::default(),
        )
        .unwrap();
        let tscn = std::fs::read_to_string(&target).unwrap();
        let scene = read::parse(&tscn).unwrap();
        let found: Vec<(String, Option<i32>)> = scene
            .nodes()
            .map(|(n, _)| (n.path(), n.unique_id))
            .collect();
        let expected = [
            (".", 1),
            ("things", (1 << 30) + 2),
            ("things/door", (1 << 29) + 1),
        ];
        if ids {
            let shape = found.last().and_then(|(_, id)| *id).unwrap();
            assert!(shape >= 3 << 29, "the shape's id is hashed: {shape}");
            let expected: Vec<(String, Option<i32>)> = expected
                .iter()
                .map(|&(p, id)| (p.to_owned(), Some(id)))
                .chain([("things/door/Shape".to_owned(), Some(shape))])
                .collect();
            assert_eq!(found, expected, "{features:?}");
        } else {
            assert!(
                found.iter().all(|(_, id)| id.is_none()),
                "{features:?}: {found:?}"
            );
        }
        assert!(
            tscn.contains("metadata/layer_id = 2\n"),
            "layers carry their id whatever the version"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn the_first_export_reports_the_counter_names_it_replaces() {
    let dir = project("renamed");
    let map = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="2" nextobjectid="198">
 <objectgroup id="1" name="objects">
  <object id="195" x="0" y="0" width="16" height="16"/>
  <object id="196" name="door" x="0" y="0"><point/></object>
  <object id="197" x="16" y="0" width="16" height="16"/>
 </objectgroup>
</map>
"#;
    std::fs::write(dir.join("maps/rooms.tmx"), map).unwrap();
    // What the export before this one wrote: kinds numbered in the order they came.
    let old = r#"[gd_scene format=3]

[node name="rooms" type="Node2D"]
texture_filter = 1

[node name="objects" type="Node2D" parent="."]

[node name="rectangle" type="Area2D" parent="objects"]
position = Vector2(0, 0)
metadata/id = 195

[node name="Shape" type="CollisionShape2D" parent="objects/rectangle"]
position = Vector2(8, 8)

[node name="door" type="Marker2D" parent="objects"]
position = Vector2(0, 0)
metadata/id = 196

[node name="rectangle2" type="Area2D" parent="objects"]
position = Vector2(16, 0)
metadata/id = 197
"#;
    let target = dir.join("rooms.tscn");
    std::fs::write(&target, old).unwrap();
    let paths = |renamed: &[Renamed]| {
        renamed
            .iter()
            .map(|r| (r.element, r.from.clone(), r.to.clone()))
            .collect::<Vec<_>>()
    };
    // Today's exports have no header: the first export after upgrading asks once (Replace).
    let refused = export(
        &dir.join("maps/rooms.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    );
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    let replace = Options {
        replace: true,
        ..Options::default()
    };
    let report = export(
        &dir.join("maps/rooms.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &replace,
    )
    .unwrap();
    let object = |id, from: &str, to: &str| (Element::Object(id), from.to_owned(), to.to_owned());
    assert_eq!(
        paths(&report.renamed),
        [
            object(195, "objects/rectangle", "objects/rectangle_195"),
            object(197, "objects/rectangle2", "objects/rectangle_197")
        ]
    );

    let again = export(
        &dir.join("maps/rooms.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert!(again.renamed.is_empty(), "{:?}", again.renamed);

    // A renamed layer: the layer and everything under it.
    std::fs::write(
        dir.join("maps/rooms.tmx"),
        map.replace(r#"name="objects""#, r#"name="things""#),
    )
    .unwrap();
    let report = export(
        &dir.join("maps/rooms.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(
        paths(&report.renamed),
        [
            (Element::Layer(1), "objects".to_owned(), "things".to_owned()),
            object(195, "objects/rectangle_195", "things/rectangle_195"),
            object(196, "objects/door", "things/door"),
            object(197, "objects/rectangle_197", "things/rectangle_197"),
        ]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn maps_whose_layers_have_no_ids_are_warned_about() {
    let tmx = |layer_id: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.0" orientation="orthogonal" renderorder="right-down" width="2" height="2" tilewidth="16" tileheight="16">
 <tileset firstgid="1" name="t" tilewidth="16" tileheight="16" tilecount="1" columns="1">
  <image source="tiles.png" width="16" height="16"/>
  <tile id="0"><objectgroup draworder="index"><object id="1" x="0" y="0" width="16" height="16"/></objectgroup></tile>
 </tileset>
 <tileset firstgid="2" source="tiles.tsx"/>
 <layer{layer_id} name="ground" width="2" height="2"><data encoding="csv">1,0,0,0</data></layer>
 <objectgroup id="9" name="things"/>
</map>
"#
        )
    };
    assert!(names::layer_ids_unsaved(&tmx("")));
    assert!(names::layer_ids_unsaved(&tmx(r#" id="0""#)));
    assert!(
        !names::layer_ids_unsaved(&tmx(r#" id="1""#)),
        "a tile's collision object group has no id, and isn't a layer"
    );
    assert!(names::layer_ids_unsaved(
        r#"{"layers": [{"id": 1, "type": "group", "layers": [{"type": "tilelayer"}]}]}"#
    ));
    assert!(!names::layer_ids_unsaved(
        r#"{"layers": [{"id": 1, "type": "group", "layers": [{"id": 2, "type": "tilelayer"}]}]}"#
    ));

    let dir = project("unsaved-ids");
    std::fs::write(dir.join("maps/old.tmx"), tmx("")).unwrap();
    let report = export(
        &dir.join("maps/old.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &dir.join("old.tscn"),
        &Options::default(),
    )
    .unwrap();
    let codes: Vec<&str> = report.notes.iter().map(|n| n.code.as_str()).collect();
    assert!(
        codes.contains(&"godot/layer-ids-unsaved"),
        "{:?}",
        report.warnings()
    );
    let level = export(
        &dir.join("maps/level.tmx"),
        &mut loader(&dir),
        &dir.join("level.tscn"),
        &Options::default(),
    )
    .unwrap();
    assert!(
        level
            .notes
            .iter()
            .all(|n| n.code.as_str() != "godot/layer-ids-unsaved")
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Runs Godot headless in `project`; (stdout, stderr). Killed after two minutes, since a script
/// error under `--script` leaves Godot running.
fn headless(godot: &Path, project: &Path, args: &[&str]) -> (String, String) {
    let mut child = std::process::Command::new(godot)
        .arg("--headless")
        .arg("--path")
        .arg(project)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pipe = |mut from: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            from.read_to_string(&mut text).ok();
            text
        })
    };
    let (stdout, stderr) = (
        pipe(Box::new(child.stdout.take().unwrap())),
        pipe(Box::new(child.stderr.take().unwrap())),
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("Godot didn't finish {args:?}:\n{}", stdout.join().unwrap());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    (stdout.join().unwrap(), stderr.join().unwrap())
}

/// The JSON a script prints between markers, and its stderr.
fn script(godot: &Path, project: &Path, text: &str, args: &[&str]) -> (Value, String) {
    std::fs::write(project.join("check.gd"), text).unwrap();
    let (stdout, stderr) = headless(
        godot,
        project,
        &[&["--script", "res://check.gd", "--"][..], args].concat(),
    );
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| panic!("no result from Godot:\n{stdout}\n{stderr}"));
    (serde_json::from_str(json).unwrap(), stderr)
}

/// Loads a scene as Godot's editor saves one: instantiated for editing and packed again.
const PACK: &str = r#"extends SceneTree

func _init() -> void:
	var args := OS.get_cmdline_user_args()
	var result := {}
	var file = load(args[0])
	if file:
		var scene: Node = file.instantiate(PackedScene.GEN_EDIT_STATE_MAIN)
		var packed := PackedScene.new()
		result["packed"] = packed.pack(scene)
		result["saved"] = ResourceSaver.save(packed, args[1])
		scene.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

/// Every node of a scene by path: its class, `id`, `layer_id` and `note` metadata, position and
/// (for shapes) whether it is disabled.
const NODES: &str = r#"extends SceneTree

func _init() -> void:
	var result := {}
	var file = load(OS.get_cmdline_user_args()[0])
	if file:
		var room: Node = file.instantiate()
		var nodes := {}
		for node: Node in [room] + room.find_children("*", "", true, false):
			var d := {"class": node.get_class()}
			for key in ["id", "layer_id", "note"]:
				if node.has_meta(key):
					d[key] = node.get_meta(key)
			if node is Node2D:
				d["position"] = [(node as Node2D).position.x, (node as Node2D).position.y]
			if node is CollisionShape2D:
				d["disabled"] = (node as CollisionShape2D).disabled
			nodes[String(room.get_path_to(node))] = d
		result["nodes"] = nodes
		room.free()
	print("BT-JSON-BEGIN", JSON.stringify(result), "BT-JSON-END")
	quit()
"#;

const BASE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="5" nextobjectid="6">
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
0,0,0,0,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <objectgroup id="2" name="Objects">
  <object id="1" name="Door" x="32" y="32" width="32" height="32"/>
  <object id="2" x="64" y="0" width="16" height="16"/>
  <object id="3" name="Lamp" x="10" y="10" width="8" height="8"/>
  <object id="4" name="Gone" x="20" y="20"><point/></object>
 </objectgroup>
 <objectgroup id="3" name="Other">
  <object id="5" name="Sign" x="0" y="64"><point/></object>
 </objectgroup>
 <imagelayer id="4" name="sky" parallaxx="0.5"/>
</map>
"#;

/// The map edited: layer Objects renamed Things and sky renamed clouds, an object added before
/// the others, object 2 moved to Other and Gone deleted.
const EDITED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="5" nextobjectid="7">
 <layer id="1" name="ground" width="4" height="4">
  <data encoding="csv">
0,0,0,0,
0,0,0,0,
0,0,0,0,
0,0,0,0
</data>
 </layer>
 <objectgroup id="2" name="Things">
  <object id="6" name="Crate" x="0" y="0"><point/></object>
  <object id="1" name="Door" x="32" y="32" width="32" height="32"/>
  <object id="3" name="Lamp" x="10" y="10" width="8" height="8"/>
 </objectgroup>
 <objectgroup id="3" name="Other">
  <object id="2" x="64" y="0" width="16" height="16"/>
  <object id="5" name="Sign" x="0" y="64"><point/></object>
 </objectgroup>
 <imagelayer id="4" name="clouds" parallaxx="0.5"/>
</map>
"#;

/// A level scene inheriting the export, as a person writes it: overrides on an object, its
/// shape, the shape of another, an object that will move, one that will be deleted and a layer
/// inside its Parallax2D, and a node added under a layer.
const LEVEL: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://levels/base.tscn" id="1_base"]

[node name="Level" instance=ExtResource("1_base")]

[node name="Door" parent="Objects"]
position = Vector2(100, 100)
metadata/note = "door"

[node name="Shape" parent="Objects/Door"]
disabled = true

[node name="Shape" parent="Objects/Lamp"]
disabled = true

[node name="rectangle_2" parent="Objects"]
metadata/note = "moved"

[node name="Gone" parent="Objects"]
metadata/note = "deleted"

[node name="sky" parent="sky"]
metadata/note = "sky"

[node name="Player" type="Node2D" parent="Objects"]
position = Vector2(5, 5)
metadata/note = "player"
"#;

/// Note 16 §3.6's table, in Godot: a level scene saved by Godot after the export had unique_ids
/// keeps its overrides and added nodes through renames, moves and additions, and drops the one
/// on a deleted object; a hand-written level scene, never saved by Godot, keeps them only where
/// the paths are unchanged.
///
/// One case Godot 4.7.2 gets only half right: an override on a node whose parent is overridden
/// too (Door/Shape here) is saved with its parent's path, not ids. After the layer is renamed,
/// Godot finds it again by its own unique_id (so helpers need theirs) but renames it
/// `Objects_Door#Shape`; without the id it is lost.
#[test]
fn level_scenes_saved_by_godot_keep_their_overrides_through_map_edits() {
    let Some(godot) = godot() else { return };
    let dir = project_for("contract", Some("4.7"));
    std::fs::create_dir_all(dir.join("levels")).unwrap();
    std::fs::write(dir.join("maps/base.tmx"), BASE).unwrap();
    let base = dir.join("levels/base.tscn");
    export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &base,
        &Options::default(),
    )
    .unwrap();
    std::fs::write(dir.join("levels/level_text.tscn"), LEVEL).unwrap();
    headless(&godot, &dir, &["--import"]);
    let (result, _) = script(
        &godot,
        &dir,
        PACK,
        &["res://levels/level_text.tscn", "res://levels/level.tscn"],
    );
    assert_eq!(result, serde_json::json!({"packed": 0, "saved": 0}));
    let saved = std::fs::read_to_string(dir.join("levels/level.tscn")).unwrap();
    let level = read::parse(&saved).unwrap();
    // A node whose parent is in the base, not in this file, is found again by its parent's id.
    let paths: HashSet<String> = level.nodes().map(|(n, _)| n.path()).collect();
    for (node, _) in level.nodes().filter(|(n, _)| {
        n.parent
            .as_ref()
            .is_some_and(|p| p != "." && !paths.contains(p))
    }) {
        assert!(
            !node.parent_id_path.is_empty(),
            "{} has no parent_id_path:\n{saved}",
            node.path()
        );
    }

    std::fs::write(dir.join("maps/base.tmx"), EDITED).unwrap();
    let report = export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &base,
        &Options::default(),
    )
    .unwrap();
    let renamed: Vec<(Element, &str, &str)> = report
        .renamed
        .iter()
        .map(|r| (r.element, r.from.as_str(), r.to.as_str()))
        .collect();
    assert_eq!(
        renamed,
        [
            (Element::Layer(2), "Objects", "Things"),
            (Element::Layer(4), "sky/sky", "clouds/clouds"),
            (Element::Object(1), "Objects/Door", "Things/Door"),
            (
                Element::Object(2),
                "Objects/rectangle_2",
                "Other/rectangle_2"
            ),
            (Element::Object(3), "Objects/Lamp", "Things/Lamp"),
        ]
    );
    headless(&godot, &dir, &["--import"]);

    let (result, _) = script(&godot, &dir, NODES, &["res://levels/level.tscn"]);
    let nodes = result["nodes"].as_object().unwrap();
    let find = |key: &str, id: i64| {
        nodes
            .iter()
            .find(|(_, d)| d[key] == id)
            .unwrap_or_else(|| panic!("no {key} {id}: {nodes:?}"))
    };
    let (path, door) = find("id", 1);
    assert_eq!(
        (path.as_str(), &door["note"], &door["position"]),
        (
            "Things/Door",
            &serde_json::json!("door"),
            &serde_json::json!([100.0, 100.0])
        ),
        "the override follows a renamed layer"
    );
    assert_eq!(
        nodes["Things/Lamp/Shape"]["disabled"], true,
        "so does one on a helper, by its parent's id"
    );
    let door_shapes: Vec<&Value> = nodes
        .iter()
        .filter(|(p, d)| p.starts_with("Things/Door/") && d["class"] == "CollisionShape2D")
        .map(|(_, d)| d)
        .collect();
    assert!(
        door_shapes.len() == 1 && door_shapes[0]["disabled"] == true,
        "found by its own id, though renamed: {nodes:?}"
    );
    let (path, moved) = find("id", 2);
    assert_eq!(
        (path.as_str(), &moved["note"]),
        ("Other/rectangle_2", &serde_json::json!("moved")),
        "the override follows an object to another layer"
    );
    assert!(
        nodes
            .values()
            .all(|d| d["id"] != 4 && d["note"] != "deleted"),
        "the deleted object's override is dropped"
    );
    let (path, sky) = find("layer_id", 4);
    assert_eq!(
        (path.as_str(), &sky["note"]),
        ("clouds/clouds", &serde_json::json!("sky")),
        "the override follows a layer in a renamed Parallax2D"
    );
    assert_eq!(
        nodes["Things/Player"]["note"], "player",
        "an added node follows its renamed layer"
    );
    let (path, added) = find("id", 6);
    assert!(
        path == "Things/Crate" && added.get("note").is_none(),
        "{path}: {added}"
    );
    assert!(
        nodes["Things/Lamp"].get("note").is_none(),
        "no override moved to another object"
    );

    // The level scene as written by hand: found by paths alone.
    let (result, _) = script(&godot, &dir, NODES, &["res://levels/level_text.tscn"]);
    let nodes = result["nodes"].as_object().unwrap();
    let lost = nodes["Things/Door"].get("note").is_none()
        && nodes["Other/rectangle_2"].get("note").is_none()
        && nodes["Things/Lamp/Shape"]["disabled"] == false;
    assert!(lost, "overrides by path are lost: {nodes:?}");
    assert_eq!(
        nodes["Objects#Player"]["note"], "player",
        "the added node moves to the root: {nodes:?}"
    );
    assert_eq!(nodes["clouds/clouds"]["layer_id"], 4);
    std::fs::remove_dir_all(&dir).ok();
}

/// An export whose helpers (a shape under its area, a layer's Parallax2D holder) have no
/// unique_id loads without a word from Godot: ids are needed only where names can change.
#[test]
fn scenes_whose_helpers_have_no_unique_ids_load_cleanly() {
    let Some(godot) = godot() else { return };
    let dir = project_for("partial-ids", Some("4.7"));
    std::fs::write(dir.join("maps/base.tmx"), BASE).unwrap();
    let target = dir.join("levels/partial.tscn");
    export(
        &dir.join("maps/base.tmx"),
        &mut bt_tmx::Loader::builder().build(),
        &target,
        &Options::default(),
    )
    .unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let helpers: Vec<&str> = tscn
        .lines()
        .filter(|l| {
            l.starts_with("[node ")
                && (l.contains("type=\"CollisionShape2D\"") || l.contains("type=\"Parallax2D\""))
        })
        .collect();
    assert_eq!(helpers.len(), 4, "three shapes and a Parallax2D: {tscn}");
    let mut partial = tscn.clone();
    for line in helpers {
        let (start, rest) = line.split_once(" unique_id=").unwrap();
        let end = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        partial = partial.replace(line, &format!("{start}{end}"));
    }
    assert!(partial.matches("unique_id=").count() == tscn.matches("unique_id=").count() - 4);
    std::fs::write(&target, partial).unwrap();
    headless(&godot, &dir, &["--import"]);
    let (result, stderr) = script(&godot, &dir, NODES, &["res://levels/partial.tscn"]);
    assert!(stderr.trim().is_empty(), "{stderr}");
    assert_eq!(
        result["nodes"]["Objects/Door/Shape"]["class"],
        "CollisionShape2D"
    );
    assert_eq!(result["nodes"]["sky/sky"]["layer_id"], 4);
    std::fs::remove_dir_all(&dir).ok();
}

/// Every node's metadata by path, as Godot loads the scene, and what the scripted ones' `état`
/// variable holds.
const META: &str = r#"extends SceneTree

func _init() -> void:
	var room: Node = (load(OS.get_cmdline_user_args()[0]) as PackedScene).instantiate()
	var nodes := {}
	for node: Node in [room] + room.find_children("*", "", true, false):
		var meta := {}
		for key in node.get_meta_list():
			meta[key] = node.get_meta(key)
		if node.get_script():
			meta["@état"] = node.get("état")
		nodes[String(room.get_path_to(node))] = meta
	room.free()
	print("BT-JSON-BEGIN", JSON.stringify({"nodes": nodes}), "BT-JSON-END")
	quit()
"#;

/// Properties named like the export's own metadata: the map's marker, a layer's and an
/// object's ids, and classes; one of them twice (`id` and `id_`).
const RESERVED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="3">
 <properties>
  <property name="bt_source" value="elsewhere.tmx"/>
 </properties>
 <objectgroup id="1" name="Things" class="Shelf">
  <properties>
   <property name="class" value="shelf prop"/>
   <property name="id" type="int" value="78"/>
   <property name="layer_id" type="int" value="77"/>
  </properties>
  <object id="1" name="Door" x="0" y="0">
   <properties><property name="id" type="int" value="42"/><property name="id_" value="taken"/></properties>
   <point/>
  </object>
  <object id="2" name="Chest" type="Box" x="10" y="10">
   <properties><property name="class" value="mine"/><property name="layer_id" type="int" value="5"/></properties>
   <point/>
  </object>
 </objectgroup>
</map>
"#;

/// A Tiled property named like the export's own metadata (`metadata/id`, `layer_id`, `class`,
/// `bt_source`) gets another key, with a warning, so that each node has each of those once,
/// holding the export's value: the rename report finds every node by its Tiled id, and
/// `export --all` the map by the marker. Scenes exported before, with the property's line after
/// the export's, are read by the first.
#[test]
fn properties_named_like_the_exports_own_metadata_get_another_key() {
    let dir = project_for("reserved", Some("4.7"));
    let (map, target) = (
        dir.join("maps/reserved.tmx"),
        dir.join("levels/reserved.tscn"),
    );
    std::fs::write(&map, RESERVED).unwrap();
    let report = export(&map, &mut loader(&dir), &target, &Options::default()).unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let scene = read::parse(&tscn).unwrap();
    let mut seen = HashMap::new();
    for (node, section) in scene.nodes() {
        let meta: Vec<(String, &str)> = section
            .props
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("metadata/")?.to_owned(), *v)))
            .collect();
        let keys: HashSet<&str> = meta.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys.len(),
            meta.len(),
            "a key twice on {}: {meta:?}",
            node.path()
        );
        seen.insert(node.path(), meta);
    }
    let meta = |path: &str| {
        seen[path]
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        meta("."),
        [
            "bt_source = \"../maps/reserved.tmx\"",
            "bt_source_ = \"elsewhere.tmx\""
        ]
    );
    assert_eq!(
        meta("Things"),
        [
            "layer_id = 1",
            "class = \"Shelf\"",
            "class_ = \"shelf prop\"",
            "id_ = 78",
            "layer_id_ = 77"
        ]
    );
    assert_eq!(
        meta("Things/Door"),
        ["id = 1", "id_2 = 42", "id_ = \"taken\""]
    );
    assert_eq!(
        meta("Things/Chest"),
        [
            "id = 2",
            "class = \"Box\"",
            "class_ = \"mine\"",
            "layer_id_ = 5"
        ]
    );
    let notes: Vec<(&str, Severity, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.severity, n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [(
            "godot/reserved-name",
            Severity::Warning,
            "metadata/id, layer_id, class and bt_source are the export's own (a Tiled id, the class, the map the scene is made from), so properties of those names go under another key: bt_source is metadata/bt_source_ on the map; class is metadata/class_ on layer 1 and object 2; id is metadata/id_ on layer 1; id is metadata/id_2 on object 1; layer_id is metadata/layer_id_ on layer 1 and object 2"
        )]
    );
    assert_eq!(
        report.notes[0].elements,
        [Element::Layer(1), Element::Object(2), Element::Object(1)]
    );

    // The map found by its marker, and every node by its id.
    let all = export_all(
        &dir,
        &mut |_| Ok(loader(&dir)),
        &Options {
            check: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(
        all.scenes.len() == 1 && all.scenes[0].outcome.as_ref().is_ok_and(Report::up_to_date),
        "{:?}",
        all.scenes
    );
    let edited = RESERVED
        .replace("\"Things\"", "\"Stuff\"")
        .replace("\"Door\"", "\"Portal\"")
        .replace("\"Chest\"", "\"Coffer\"");
    std::fs::write(&map, edited).unwrap();
    let report = export(&map, &mut loader(&dir), &target, &Options::default()).unwrap();
    let all_three = [
        (Element::Layer(1), "Things".to_owned(), "Stuff".to_owned()),
        (
            Element::Object(1),
            "Things/Door".to_owned(),
            "Stuff/Portal".to_owned(),
        ),
        (
            Element::Object(2),
            "Things/Chest".to_owned(),
            "Stuff/Coffer".to_owned(),
        ),
    ];
    assert_eq!(
        report
            .renamed
            .iter()
            .map(|r| (r.element, r.from.clone(), r.to.clone()))
            .collect::<Vec<_>>(),
        all_three
    );

    // As an export before this wrote them: each property's line after the export's own.
    let old = "[gd_scene format=3]\n\n[node name=\"reserved\" type=\"Node2D\"]\nmetadata/bt_source = \"../maps/reserved.tmx\"\nmetadata/bt_source = \"elsewhere.tmx\"\n\n[node name=\"Things\" type=\"Node2D\" parent=\".\"]\nmetadata/layer_id = 1\nmetadata/id = 78\nmetadata/layer_id = 77\n\n[node name=\"Door\" type=\"Marker2D\" parent=\"Things\"]\nmetadata/id = 1\nmetadata/id = 42\n\n[node name=\"Chest\" type=\"Marker2D\" parent=\"Things\"]\nmetadata/id = 2\nmetadata/layer_id = 5\n";
    let mut names = names::Names::default();
    for (element, path) in [
        (Element::Layer(1), "Stuff"),
        (Element::Object(1), "Stuff/Portal"),
        (Element::Object(2), "Stuff/Coffer"),
    ] {
        names.placed(element, path);
    }
    let from_old: Vec<(Element, String, String)> = names
        .renamed(old)
        .into_iter()
        .map(|r| (r.element, r.from, r.to))
        .collect();
    assert_eq!(from_old, all_three);
    // And it still names the map it was made from, for the contract and --all.
    let old_scene = dir.join("old.tscn");
    std::fs::write(&old_scene, old).unwrap();
    assert_eq!(
        super::super::contract::marker_in(&old_scene).as_deref(),
        Some("../maps/reserved.tmx")
    );
    std::fs::remove_file(&old_scene).unwrap();

    // Godot takes each key, with the export's value.
    let Some(godot) = godot() else { return };
    headless(&godot, &dir, &["--import"]);
    let (result, stderr) = script(&godot, &dir, META, &["res://levels/reserved.tscn"]);
    assert!(stderr.trim().is_empty(), "{stderr}");
    assert_eq!(
        result["nodes"]["."],
        serde_json::json!({"bt_source": "../maps/reserved.tmx", "bt_source_": "elsewhere.tmx"})
    );
    assert_eq!(
        result["nodes"]["Stuff"],
        serde_json::json!({"layer_id": 1, "class": "Shelf", "class_": "shelf prop", "id_": 78, "layer_id_": 77})
    );
    assert_eq!(
        result["nodes"]["Stuff/Portal"],
        serde_json::json!({"id": 1, "id_": "taken", "id_2": 42})
    );
    assert_eq!(
        result["nodes"]["Stuff/Coffer"],
        serde_json::json!({"id": 2, "class": "Box", "class_": "mine", "layer_id_": 5})
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Properties with names beyond ASCII, with a space and with a leading digit, two coming to
/// the key another property has, and a scene variable named beyond ASCII.
const KEYS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="32" tileheight="32" infinite="0" nextlayerid="2" nextobjectid="3">
 <properties>
  <property name="état" value="carte"/>
 </properties>
 <objectgroup id="1" name="Things">
  <properties>
   <property name="grosse" type="int" value="4"/>
   <property name="größe" type="int" value="3"/>
  </properties>
  <object id="1" name="Sign" x="0" y="0">
   <properties>
    <property name="2nd" value="b"/>
    <property name="max hp" type="int" value="10"/>
    <property name="max_hp" type="int" value="11"/>
    <property name="Œuvre" value="x"/>
    <property name="体力" type="int" value="7"/>
   </properties>
   <point/>
  </object>
  <object id="2" name="Chest" x="10" y="10">
   <properties><property name="resPath" value="res://scenes/chest.tscn"/><property name="état" type="int" value="5"/></properties>
   <point/>
  </object>
 </objectgroup>
</map>
"#;

/// Metadata keys are ASCII identifiers, the only ones Godot's `set_meta` takes ("Invalid
/// metadata identifier" otherwise, and the value is lost): accents dropped, anything else `_`,
/// two properties coming to one key kept apart, and the renames noted. A script variable keeps
/// its name, quoted as Godot quotes it.
#[test]
fn metadata_keys_are_ones_godot_takes() {
    let dir = project_for("metadata-keys", Some("4.7"));
    std::fs::create_dir_all(dir.join("scenes")).unwrap();
    std::fs::write(
        dir.join("scenes/chest.gd"),
        "extends Node2D\n\n@export var état: int = 0\n",
    )
    .unwrap();
    std::fs::write(dir.join("scenes/chest.tscn"), "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://scenes/chest.gd\" id=\"1\"]\n\n[node name=\"Root\" type=\"Node2D\"]\nscript = ExtResource(\"1\")\n").unwrap();
    let (map, target) = (dir.join("maps/keys.tmx"), dir.join("levels/keys.tscn"));
    std::fs::write(&map, KEYS).unwrap();
    let report = export(&map, &mut loader(&dir), &target, &Options::default()).unwrap();
    let tscn = std::fs::read_to_string(&target).unwrap();
    let s = sections(&tscn);
    let meta = |name: &str, parent: &str| {
        node(&s, name, parent)
            .iter()
            .filter(|l| l.starts_with("metadata/") || l.starts_with('"'))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        meta("Things", "."),
        [
            "metadata/layer_id = 1",
            "metadata/grosse = 4",
            "metadata/grosse_ = 3"
        ]
    );
    assert_eq!(
        meta("Sign", "Things"),
        [
            "metadata/id = 1",
            "metadata/_2nd = \"b\"",
            "metadata/max_hp_ = 10",
            "metadata/max_hp = 11",
            "metadata/OEuvre = \"x\"",
            "metadata/__ = 7"
        ]
    );
    assert_eq!(
        meta("Chest", "Things"),
        ["\"état\" = 5", "metadata/id = 2", "metadata/etat = 5"]
    );
    let notes: Vec<(&str, Severity, &str)> = report
        .notes
        .iter()
        .map(|n| (n.code.as_str(), n.severity, n.message.as_str()))
        .collect();
    assert_eq!(
        notes,
        [(
            "godot/metadata-key",
            Severity::Info,
            "Godot's metadata keys are ASCII letters, digits and _, not starting with a digit, so these properties go under another key: 2nd is metadata/_2nd on object 1; größe is metadata/grosse_ on layer 1; max hp is metadata/max_hp_ on object 1; état is metadata/etat on the map and object 2; Œuvre is metadata/OEuvre on object 1; 体力 is metadata/__ on object 1"
        )]
    );
    assert_eq!(
        report.notes[0].elements,
        [Element::Object(1), Element::Layer(1), Element::Object(2)]
    );

    let Some(godot) = godot() else { return };
    headless(&godot, &dir, &["--import"]);
    let (result, stderr) = script(&godot, &dir, META, &["res://levels/keys.tscn"]);
    assert!(stderr.trim().is_empty(), "{stderr}");
    assert_eq!(
        result["nodes"]["."],
        serde_json::json!({"bt_source": "../maps/keys.tmx", "etat": "carte"})
    );
    assert_eq!(
        result["nodes"]["Things"],
        serde_json::json!({"layer_id": 1, "grosse": 4, "grosse_": 3})
    );
    assert_eq!(
        result["nodes"]["Things/Sign"],
        serde_json::json!({"id": 1, "_2nd": "b", "max_hp_": 10, "max_hp": 11, "OEuvre": "x", "__": 7})
    );
    assert_eq!(
        result["nodes"]["Things/Chest"],
        serde_json::json!({"id": 2, "etat": 5, "@état": 5})
    );
    std::fs::remove_dir_all(&dir).ok();
}
