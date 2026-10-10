//! Rules read from files: finding them, filters and includes, rule maps read through the map's
//! loader, reloading, `MatchType` from the project's classes, symbolic links, and the files to
//! watch. Rule maps and maps are built as grids and written with `bt-tmx` into a folder of the
//! test's own.

#![cfg(test)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::{Error, ProblemKind, files};
use bt_core::{
    ClassType, ClassUsage, CustomType, CustomTypes, FileRef, Layer, LayerId, LayerKind, Map,
    MapObject, ObjectId, ObjectLayer, ObjectTemplate, Project, Properties, Property, TileData,
    TileId, Tileset, TilesetRef, Value,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::{Loader, WriteRequest};
use common::{grid_of, load_map};
use glam::{DVec2, UVec2};

/// A folder with an external tileset (`tiles.tsx` and its image) for rule maps and maps.
struct Folder {
    scratch: Scratch,
    tileset: Arc<Tileset>,
}

impl Folder {
    fn new(name: &str) -> Folder {
        let scratch = Scratch::new(name).unwrap();
        let atlas = Atlas::builder()
            .name("tiles")
            .build_in(scratch.path())
            .unwrap();
        let tileset = atlas.tileset;
        Folder::write_tileset(&scratch.join("tiles.tsx"), &tileset);
        Folder { scratch, tileset }
    }

    fn write_tileset(path: &Path, tileset: &Tileset) {
        WriteRequest::for_tileset(tileset, path).write().unwrap();
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.scratch.join(relative)
    }

    /// The external tileset, as a map refers to it.
    fn tiles(&self) -> TilesetRef {
        TilesetRef {
            file: Some(FileRef::new("tiles.tsx", self.path("tiles.tsx"))),
            first_gid: Some(1),
            tileset: Arc::clone(&self.tileset),
        }
    }

    /// A map of `size` using the external tileset, from tile layers given as grids.
    fn map(&self, size: UVec2, layers: &[(&str, &str)]) -> Map {
        let mut builder = MapBuilder::new(UVec2::splat(16), size);
        for (name, cells) in layers {
            builder = builder.tile_layer(name, cells);
        }
        let mut map = builder.build().unwrap();
        map.tilesets.push(self.tiles());
        map
    }

    fn write_map(&self, relative: &str, map: &Map) -> PathBuf {
        let path = self.path(relative);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        WriteRequest::for_map(map, &path).write().unwrap();
        path
    }

    fn write_text(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.path(relative);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    /// A rule map turning tile `from` into tile `to` on layer `set`.
    fn write_rule(&self, relative: &str, from: u32, to: u32) -> PathBuf {
        let rule = self.map(
            UVec2::new(1, 1),
            &[
                ("input_set", &from.to_string()),
                ("output_set", &to.to_string()),
            ],
        );
        self.write_map(relative, &rule)
    }
}

#[test]
fn rule_maps_are_read_through_the_maps_loader_and_share_its_tilesets() {
    let folder = Folder::new("bt-automap-files-shared");
    folder.write_rule("rules/one.tmx", 1, 2);
    folder.write_rule("rules/two.tmx", 2, 3);
    folder.write_text(
        "rules.txt",
        "# rules\nrules/one.tmx\n// and\nrules/two.tmx\n",
    );
    let map_path = folder.write_map("map.tmx", &folder.map(UVec2::new(2, 1), &[("set", "1 .")]));
    let mut loader = Loader::default();
    let mut map = load_map(&mut loader, &map_path);
    let found = files::find_rules(Some(&map_path), None, None).unwrap();
    assert_eq!(found, folder.path("rules.txt"));
    let rules = files::load(&mut loader, &found, Some(&map_path)).unwrap();
    assert_eq!(rules.rule_maps().len(), 2);
    assert!(rules.problems().is_empty());
    assert!(Arc::ptr_eq(
        &rules.tileset(0).unwrap().tileset,
        &map.tilesets[0].tileset
    ));
    assert_eq!(rules.tileset_files(), [folder.path("tiles.tsx")]);
    assert_eq!(rules.rules_file(), Some(found.as_path()));
    rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "3 .");
}

#[test]
fn filters_and_includes_choose_the_rule_maps_of_each_map() {
    let folder = Folder::new("bt-automap-files-filters");
    folder.write_rule("all.tmx", 1, 2);
    folder.write_rule("town.tmx", 2, 3);
    folder.write_rule("sub/cave.tmx", 2, 4);
    folder.write_text("sub/rules.txt", "[cave*]\ncave.tmx\n");
    folder.write_text(
        "rules.txt",
        "all.tmx\n[town*]\ntown.tmx\n[*]\nsub/rules.txt\n",
    );
    let names = |map: &str| -> Vec<String> {
        let mut loader = Loader::default();
        files::load(&mut loader, &folder.path("rules.txt"), Some(Path::new(map)))
            .unwrap()
            .rule_maps()
            .iter()
            .map(|r| r.path().file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names("/maps/Town1.tmx"), ["all.tmx", "town.tmx"]);
    assert_eq!(names("/maps/cave2.tmx"), ["all.tmx", "cave.tmx"]);
    assert_eq!(names("/maps/field.tmx"), ["all.tmx"]);
}

#[test]
fn a_missing_rule_map_stops_the_load_and_a_fixed_one_is_listed_once() {
    let folder = Folder::new("bt-automap-files-missing");
    folder.write_rule("one.tmx", 1, 2);
    folder.write_text("rules.txt", "one.tmx\ntwo.tmx\n");
    let mut loader = Loader::default();
    let rules_path = folder.path("rules.txt");
    match files::load(&mut loader, &rules_path, None) {
        Err(Error::MissingRuleMap { path, .. }) => assert_eq!(path, folder.path("two.tmx")),
        other => panic!("{other:?}"),
    }
    // Tiled would list one.tmx twice on the next try; here each load starts afresh.
    folder.write_rule("two.tmx", 2, 3);
    let rules = files::load(&mut loader, &rules_path, None).unwrap();
    assert_eq!(rules.rule_maps().len(), 2);
    assert!(matches!(
        files::load(&mut loader, &folder.path("none.txt"), None),
        Err(Error::RulesFile { .. })
    ));
}

#[test]
fn a_rule_map_that_cannot_be_read_is_left_out_or_keeps_its_previous_rules() {
    let folder = Folder::new("bt-automap-files-reload");
    folder.write_rule("one.tmx", 1, 2);
    folder.write_rule("two.tmx", 2, 3);
    let rules_path = folder.write_text("rules.txt", "one.tmx\ntwo.tmx\n");
    let mut loader = Loader::default();
    let first = files::load(&mut loader, &rules_path, None).unwrap();
    // Caught half-written.
    folder.write_text("two.tmx", "<map><layer");
    let reloaded = files::reload(&mut loader, &rules_path, None, &first).unwrap();
    assert_eq!(reloaded.rule_maps().len(), 2);
    let kept = &reloaded.rule_maps()[1];
    assert_eq!(kept.rule_count(), 1);
    assert_eq!(kept.problems()[0].kind, ProblemKind::UnreadableRuleMap);
    assert!(kept.problems()[0].text.contains("kept"));
    // Loaded afresh there is nothing to keep: the rule map is left out, and the set says so.
    let fresh = files::load(&mut loader, &rules_path, None).unwrap();
    assert_eq!(fresh.rule_maps().len(), 1);
    assert_eq!(fresh.problems()[0].kind, ProblemKind::UnreadableRuleMap);
    // Fixed, it is read again.
    folder.write_rule("two.tmx", 2, 4);
    let fixed = files::reload(&mut loader, &rules_path, None, &reloaded).unwrap();
    assert!(fixed.rule_maps()[1].problems().is_empty());
    let mut map = folder.map(UVec2::new(1, 1), &[("set", "2")]);
    fixed.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "4");
}

fn hole_types(match_type: &str) -> CustomTypes {
    let mut members = Properties::new();
    members.insert(
        "MatchType".into(),
        Property::new(Value::String(match_type.into())),
    );
    let mut types = CustomTypes::new();
    types.add(CustomType::class(
        "Hole",
        ClassType {
            members,
            use_as: ClassUsage::TILE,
            ..ClassType::default()
        },
    ));
    types
}

#[test]
fn a_tiles_class_gives_its_match_type_and_a_changed_class_asks_for_a_reload() {
    let folder = Folder::new("bt-automap-files-classes");
    // Tile 9 of a tileset of the rules' own is a "Hole": Empty by its class.
    let mut marks = Tileset::clone(&folder.tileset);
    marks.name = "marks".into();
    marks.tiles.insert(
        TileId(9),
        TileData {
            class: "Hole".into(),
            ..TileData::default()
        },
    );
    Folder::write_tileset(&folder.path("marks.tsx"), &marks);
    let marks_ref = TilesetRef {
        file: Some(FileRef::new("marks.tsx", folder.path("marks.tsx"))),
        first_gid: Some(17),
        tileset: Arc::new(marks),
    };
    let mut rule = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
        .tile_layer("input_set", "9@1")
        .tile_layer("output_set", "5")
        .build()
        .unwrap();
    rule.tilesets.push(folder.tiles());
    rule.tilesets.push(marks_ref);
    folder.write_map("rule.tmx", &rule);
    let project = Project {
        custom_types: hole_types("Empty"),
        ..Project::default()
    };
    let mut loader = Loader::builder()
        .custom_types(Arc::new(project.custom_types.clone()))
        .build();
    let rules = files::load(&mut loader, &folder.path("rule.tmx"), None).unwrap();
    let mut map = folder.map(UVec2::new(2, 1), &[("set", "1 .")]);
    rules.apply(&mut map, None, 1);
    assert_eq!(
        grid_of(&map, "set"),
        "1 5",
        "the hole matched the empty cell"
    );
    assert!(!rules.classes_changed(&hole_types("Empty")));
    assert!(rules.classes_changed(&hole_types("NonEmpty")));
    assert!(rules.classes_changed(&CustomTypes::new()));
}

#[cfg(unix)]
#[test]
fn rules_reached_through_symbolic_links_share_the_maps_tilesets() {
    let folder = Folder::new("bt-automap-files-links");
    folder.write_rule("real/rule.tmx", 1, 2);
    folder.write_text("real/rules.txt", "rule.tmx\n");
    std::os::unix::fs::symlink(folder.path("real"), folder.path("linked")).unwrap();
    let map_path = folder.write_map("map.tmx", &folder.map(UVec2::new(1, 1), &[("set", "1")]));
    let mut loader = Loader::default();
    let mut map = load_map(&mut loader, &map_path);
    let rules = files::load(
        &mut loader,
        &folder.path("linked/rules.txt"),
        Some(&map_path),
    )
    .unwrap();
    assert_eq!(rules.rule_maps()[0].path(), folder.path("linked/rule.tmx"));
    rules.apply(&mut map, None, 1);
    assert_eq!(grid_of(&map, "set"), "2");
}

#[test]
fn every_file_whose_change_reloads_the_rules_is_watched() {
    let folder = Folder::new("bt-automap-files-watched");
    folder.write_rule("all.tmx", 1, 2);
    folder.write_rule("town.tmx", 2, 3);
    folder.write_rule("sub/cave.tmx", 2, 4);
    folder.write_text("sub/rules.txt", "[cave*]\ncave.tmx\n");
    // A rule map placing an object made from a template.
    let template = ObjectTemplate {
        object: Some(MapObject {
            size: DVec2::splat(16.0),
            ..MapObject::default()
        }),
        ..ObjectTemplate::default()
    };
    WriteRequest::for_template(&template, &folder.path("thing.tx"))
        .write()
        .unwrap();
    let mut objects = folder.map(UVec2::new(1, 1), &[("input_set", "1")]);
    objects.layers.push(Layer::new(
        LayerId(9),
        "output_things",
        LayerKind::Object(ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(1),
                template: Some(FileRef::new("thing.tx", folder.path("thing.tx"))),
                size: DVec2::splat(16.0),
                ..MapObject::default()
            }]),
            ..ObjectLayer::default()
        }),
    ));
    objects.next_layer_id = 10;
    objects.next_object_id = 2;
    folder.write_map("objects.tmx", &objects);
    folder.write_text("broken.tmx", "<map><layer");
    let rules_path = folder.write_text(
        "rules.txt",
        "all.tmx\n[town*]\ntown.tmx\n[*]\nsub/rules.txt\nobjects.tmx\nbroken.tmx\n",
    );
    let mut loader = Loader::default();
    let rules = files::load(&mut loader, &rules_path, Some(Path::new("/maps/field.tmx"))).unwrap();
    // Town's rules do not apply to this map and broken.tmx cannot be read: both are watched.
    assert_eq!(rules.rule_maps().len(), 2);
    let mut expected: Vec<PathBuf> = [
        "rules.txt",
        "sub/rules.txt",
        "all.tmx",
        "town.tmx",
        "sub/cave.tmx",
        "objects.tmx",
        "broken.tmx",
        "tiles.tsx",
        "thing.tx",
    ]
    .iter()
    .map(|name| folder.path(name))
    .collect();
    expected.sort();
    assert_eq!(rules.watched_files(), expected);
}

#[test]
fn an_include_cycle_is_a_note_naming_the_files() {
    let folder = Folder::new("bt-automap-files-cycle");
    folder.write_rule("one.tmx", 1, 2);
    folder.write_text("sub/rules.txt", "../rules.txt\n");
    let rules_path = folder.write_text("rules.txt", "one.tmx\nsub/rules.txt\n");
    let mut loader = Loader::default();
    let rules = files::load(&mut loader, &rules_path, None).unwrap();
    assert_eq!(rules.rule_maps().len(), 1);
    let problems = rules.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].kind, ProblemKind::IncludeCycle);
    assert_eq!(problems[0].severity(), bt_automap::Severity::Note);
    let text = &problems[0].text;
    assert!(
        text.contains(&folder.path("sub/rules.txt").display().to_string()),
        "{text}"
    );
}

#[test]
fn a_failed_load_names_the_files_to_watch_for_the_rules_to_load_again() {
    let folder = Folder::new("bt-automap-files-failed");
    folder.write_rule("one.tmx", 1, 2);
    let rules_path = folder.write_text("rules.txt", "one.tmx\nsub/rules.txt\ntwo.tmx\n");
    let mut loader = Loader::default();
    // A list that cannot be read stops the load; it is among the files to watch.
    assert!(matches!(
        files::load(&mut loader, &rules_path, None),
        Err(Error::RulesFile { .. })
    ));
    assert_eq!(
        files::listed_files(&rules_path),
        [
            folder.path("rules.txt"),
            folder.path("sub/rules.txt"),
            folder.path("one.tmx")
        ]
    );
    // So is a rule map that does not exist.
    folder.write_text("sub/rules.txt", "");
    assert!(matches!(
        files::load(&mut loader, &rules_path, None),
        Err(Error::MissingRuleMap { .. })
    ));
    assert!(files::listed_files(&rules_path).contains(&folder.path("two.tmx")));
}

#[test]
fn a_rule_tileset_copying_the_maps_through_another_file_is_reported() {
    let folder = Folder::new("bt-automap-files-copies");
    // copy.tsx: the same image and grid as tiles.tsx, another file.
    Folder::write_tileset(&folder.path("copy.tsx"), &folder.tileset);
    let mut rule = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
        .tile_layer("input_set", "1")
        .tile_layer("output_set", "2")
        .build()
        .unwrap();
    rule.tilesets.push(TilesetRef {
        file: Some(FileRef::new("copy.tsx", folder.path("copy.tsx"))),
        first_gid: Some(1),
        tileset: Arc::clone(&folder.tileset),
    });
    folder.write_map("rule.tmx", &rule);
    let map_path = folder.write_map("map.tmx", &folder.map(UVec2::new(1, 1), &[("set", "1")]));
    let mut loader = Loader::default();
    let mut map = load_map(&mut loader, &map_path);
    let rules = files::load(&mut loader, &folder.path("rule.tmx"), Some(&map_path)).unwrap();
    let copies = rules.tileset_copies(&map);
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].rules_file, Some(folder.path("copy.tsx")));
    assert_eq!(copies[0].map_file, Some(folder.path("tiles.tsx")));
    // As in Tiled, the rule does not match the map's tiles.
    let report = rules.apply(&mut map, None, 1);
    assert_eq!(report.matches, 0);
}
