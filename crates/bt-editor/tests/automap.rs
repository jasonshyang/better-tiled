//! AutoMap in the editor on a temporary folder: the rules beside a map are read on a worker
//! once the map is current, Map ▸ AutoMap runs them as one step over the selection or the whole
//! map, a run asked for while they are read waits for them, changed rule files are read again
//! (a failed read keeps the rules there were), and a run that does nothing says why.

#![cfg(test)]

mod common;

use std::path::{Path, PathBuf};

use bt_core::{FileRef, LayerId, TilesetRef};
use bt_edit::{SelectionMode, TileRegion};
use bt_editor::{
    ActionId, DocumentId, Editor, Effect, Found, Intent, LiveEdit, Observation, Severity,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_tmx::WriteRequest;
use bt_utils::IRect;
use bt_utils::hash::fnv1a_64;
use common::{apply, editor_in};
use glam::{IVec2, UVec2};

/// A map or rule map of tile layers given as grids over the folder's `tiles.tsx`, written as
/// `name` in `folder`.
fn write_tiles_map(folder: &Path, name: &str, size: UVec2, layers: &[(&str, &str)]) -> PathBuf {
    let atlas = Atlas::builder().name("tiles").build_in(folder).unwrap();
    let tileset_path = folder.join("tiles.tsx");
    WriteRequest::for_tileset(&atlas.tileset, &tileset_path)
        .write()
        .unwrap();
    let mut builder = MapBuilder::new(UVec2::new(16, 16), size).tileset(atlas.tileset.clone());
    for (layer, cells) in layers {
        builder = builder.tile_layer(layer, cells);
    }
    let mut map = builder.build().unwrap();
    map.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", &tileset_path)),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    let path = folder.join(name);
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// A folder with a map of sand (tile 0) and an empty `top` layer, and the rule "sand gets a
/// flower (tile `flower`) on top" listed in a `rules.txt` beside it.
fn folder_with_rules(name: &str, flower: u32) -> (Scratch, PathBuf) {
    let scratch = Scratch::new(name).unwrap();
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("ground", "0 0 0\n0 0 0"), ("top", ". . .\n. . .")],
    );
    write_flower_rule(scratch.path(), flower);
    std::fs::write(scratch.join("rules.txt"), "rule.tmx\n").unwrap();
    (scratch, map)
}

fn write_flower_rule(folder: &Path, flower: u32) {
    write_tiles_map(
        folder,
        "rule.tmx",
        UVec2::new(1, 1),
        &[("input_ground", "0"), ("output_top", &flower.to_string())],
    );
}

/// The editor with the map open and current, after the tick that reads its rules.
fn open(scratch: &Scratch, map: &Path) -> (Editor, DocumentId) {
    let mut editor = editor_in(scratch);
    apply(&mut editor, Intent::Open(map.to_path_buf()));
    apply(&mut editor, Intent::Tick { now_ms: 0 });
    let doc = editor.current().unwrap();
    (editor, doc)
}

/// The `top` layer as a grid of tile ids.
fn top(editor: &Editor, doc: DocumentId) -> String {
    let map = editor.map_of(doc).unwrap();
    let layer = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    (0..2)
        .map(|y| {
            (0..3)
                .map(|x| {
                    let cell = layer.cell(IVec2::new(x, y));
                    match cell.slot() {
                        Some(_) => cell.tile().0.to_string(),
                        None => ".".to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn notices(effects: &[Effect]) -> Vec<(Severity, String)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notify(notice) => Some((notice.severity, notice.text.clone())),
            _ => None,
        })
        .collect()
}

/// What the window's poll finds: every watched file, hashed.
fn poll(editor: &mut Editor) -> Vec<Effect> {
    let observations = editor
        .watched()
        .into_iter()
        .map(|path| {
            let found = match std::fs::read(&path) {
                Ok(bytes) => Found::Present {
                    content_hash: fnv1a_64(&bytes),
                },
                Err(_) => Found::Missing,
            };
            Observation { path, found }
        })
        .collect();
    apply(editor, Intent::Observed(observations))
}

#[test]
fn automap_runs_the_rules_beside_the_map_as_one_step() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-run", 6);
    let (mut editor, doc) = open(&scratch, &map);
    let rules = editor.automap_rules(doc).unwrap();
    assert_eq!(rules.file(), Some(scratch.join("rules.txt").as_path()));
    assert_eq!(rules.rules().unwrap().rule_count(), 1);
    assert!(!rules.is_reading());

    let effects = apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        notices(&effects),
        [(Severity::Info, "AutoMap applied 6 matches".to_owned())]
    );
    assert_eq!(top(&editor, doc), "6 6 6\n6 6 6");
    let last = editor.automap_rules(doc).unwrap().last_run().unwrap();
    assert_eq!(last.report.matches, 6);
    assert_eq!(
        top_of_map(&last.before),
        "empty",
        "the map before the run is kept"
    );
    let history = editor.document(doc).unwrap().history().entries();
    assert_eq!(history.last().unwrap().label, "AutoMap");

    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(top(&editor, doc), ". . .\n. . .", "one step");
}

fn top_of_map(map: &bt_core::Map) -> &'static str {
    let layer = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    if layer.cell(IVec2::ZERO).slot().is_none() {
        "empty"
    } else {
        "painted"
    }
}

#[test]
fn automap_on_a_selection_runs_only_where_it_is() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-selection", 6);
    let (mut editor, doc) = open(&scratch, &map);
    apply(
        &mut editor,
        Intent::SelectTiles {
            doc,
            region: TileRegion::from_rect(IRect {
                x: 1,
                y: 0,
                w: 1,
                h: 2,
            }),
            mode: SelectionMode::Replace,
        },
    );
    let effects = apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        notices(&effects),
        [(Severity::Info, "AutoMap applied 2 matches".to_owned())]
    );
    assert_eq!(top(&editor, doc), ". 6 .\n. 6 .");
}

#[test]
fn automap_asked_for_while_the_rules_are_read_runs_when_they_arrive() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-waits", 6);
    let mut editor = editor_in(&scratch);
    apply(&mut editor, Intent::Open(map));
    let doc = editor.current().unwrap();
    let effects = editor.apply(Intent::Run(ActionId::AutoMap));
    let [Effect::LoadRules(job)] = effects.as_slice() else {
        panic!("the rules are read first: {effects:?}");
    };
    assert_eq!(job.file(), scratch.join("rules.txt"));
    assert!(editor.automap_rules(doc).unwrap().is_reading());
    assert_eq!(
        top(&editor, doc),
        ". . .\n. . .",
        "nothing runs before the rules"
    );
    let effects = editor.apply(Intent::RulesLoaded(Box::new(job.run())));
    assert_eq!(
        notices(&effects),
        [(Severity::Info, "AutoMap applied 6 matches".to_owned())]
    );
    assert_eq!(top(&editor, doc), "6 6 6\n6 6 6");
}

#[test]
fn a_map_without_rules_is_told_where_to_put_them() {
    let scratch = Scratch::new("bt-editor-automap-no-rules").unwrap();
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(1, 1),
        &[("ground", "0")],
    );
    let (mut editor, doc) = open(&scratch, &map);
    assert_eq!(editor.automap_rules(doc).unwrap().file(), None);
    let effects = apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        notices(&effects),
        [(
            Severity::Warning,
            "AutoMap has no rules for a.tmx: put a rules.txt beside the map, or name a rules \
             file in the project's properties"
                .to_owned()
        )]
    );
}

#[test]
fn a_run_that_does_nothing_says_which_layers_the_map_lacks() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-missing-layer", 6);
    write_tiles_map(
        scratch.path(),
        "rule.tmx",
        UVec2::new(1, 1),
        &[("input_walls", "0"), ("output_top", "6")],
    );
    let (mut editor, _doc) = open(&scratch, &map);
    let effects = apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        notices(&effects),
        [(
            Severity::Warning,
            "AutoMap did nothing: the map has no layer walls that the rules read".to_owned()
        )]
    );
}

#[test]
fn rules_are_read_again_when_their_files_change() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-reread", 6);
    let (mut editor, doc) = open(&scratch, &map);
    let watched = editor.watched();
    for file in ["rules.txt", "rule.tmx", "tiles.tsx"] {
        assert!(watched.contains(&scratch.join(file)), "{file} is watched");
    }
    assert!(poll(&mut editor).is_empty(), "nothing changed");

    write_flower_rule(scratch.path(), 7);
    poll(&mut editor);
    apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(top(&editor, doc), "7 7 7\n7 7 7", "the changed rule ran");
}

#[test]
fn a_failed_read_keeps_the_rules_there_were() {
    let (scratch, map) = folder_with_rules("bt-editor-automap-failed-read", 6);
    let (mut editor, doc) = open(&scratch, &map);
    std::fs::write(scratch.join("rules.txt"), "rule.tmx\nmissing.tmx\n").unwrap();
    let effects = poll(&mut editor);
    assert_eq!(
        notices(&effects),
        [(
            Severity::Error,
            "Cannot read the rules in rules.txt: the rules read before stay in use until it is \
             fixed"
                .to_owned()
        )]
    );
    let rules = editor.automap_rules(doc).unwrap();
    assert!(rules.error().unwrap().contains("missing.tmx"));
    assert_eq!(rules.rules().unwrap().rule_count(), 1);
    apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(top(&editor, doc), "6 6 6\n6 6 6", "the earlier rules ran");

    write_tiles_map(scratch.path(), "missing.tmx", UVec2::new(1, 1), &[]);
    poll(&mut editor);
    assert_eq!(editor.automap_rules(doc).unwrap().error(), None, "fixed");
}

#[test]
fn a_rules_txt_appearing_beside_the_map_is_read() {
    let scratch = Scratch::new("bt-editor-automap-appears").unwrap();
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("ground", "0 0 0\n0 0 0"), ("top", ". . .\n. . .")],
    );
    let (mut editor, doc) = open(&scratch, &map);
    assert!(poll(&mut editor).is_empty());
    assert_eq!(editor.automap_rules(doc).unwrap().file(), None);

    write_flower_rule(scratch.path(), 6);
    std::fs::write(scratch.join("rules.txt"), "rule.tmx\n").unwrap();
    poll(&mut editor);
    let rules = editor.automap_rules(doc).unwrap();
    assert_eq!(rules.file(), Some(scratch.join("rules.txt").as_path()));
    assert_eq!(rules.rules().unwrap().rule_count(), 1);
}

#[test]
fn rules_are_read_again_when_a_class_gives_a_rule_tile_another_match_type() {
    use std::sync::Arc;

    use bt_core::{
        ClassType, ClassUsage, CustomType, CustomTypes, Project, Properties, Property, TileData,
        TileId, Tileset, Value,
    };
    use bt_editor::TypeEdit;

    let scratch = Scratch::new("bt-editor-automap-classes").unwrap();
    // Projects are looked for below the home folder, which is the scratch folder here.
    let folder = scratch.join("game");
    std::fs::create_dir_all(&folder).unwrap();
    let mut members = Properties::new();
    members.insert(
        "MatchType".into(),
        Property::new(Value::String("Empty".into())),
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
    let project = Project {
        custom_types: types,
        ..Project::default()
    };
    WriteRequest::for_project(&project, &folder.join("game.tiled-project"))
        .write()
        .unwrap();
    let map = write_tiles_map(
        &folder,
        "a.tmx",
        UVec2::new(3, 2),
        &[("ground", "0 . 0\n0 0 0"), ("top", ". . .\n. . .")],
    );
    // A tileset of the rules' own whose tile 9 is a hole: Empty, by its class.
    let atlas = Atlas::builder().name("tiles").build_in(&folder).unwrap();
    let mut marks = Tileset::clone(&atlas.tileset);
    marks.name = "marks".into();
    marks.tiles.insert(
        TileId(9),
        TileData {
            class: "Hole".into(),
            ..TileData::default()
        },
    );
    WriteRequest::for_tileset(&marks, &folder.join("marks.tsx"))
        .write()
        .unwrap();
    let marks = Arc::new(marks);
    let mut rule = MapBuilder::new(UVec2::new(16, 16), UVec2::new(1, 1))
        .tileset(atlas.tileset.clone())
        .tileset(Arc::clone(&marks))
        .tile_layer("input_ground", "9@1")
        .tile_layer("output_top", "6")
        .build()
        .unwrap();
    rule.tilesets[0] = TilesetRef {
        file: Some(FileRef::new("tiles.tsx", folder.join("tiles.tsx"))),
        first_gid: Some(1),
        tileset: atlas.tileset.clone(),
    };
    rule.tilesets[1] = TilesetRef {
        file: Some(FileRef::new("marks.tsx", folder.join("marks.tsx"))),
        first_gid: Some(17),
        tileset: marks,
    };
    WriteRequest::for_map(&rule, &folder.join("rule.tmx"))
        .write()
        .unwrap();
    std::fs::write(folder.join("rules.txt"), "rule.tmx\n").unwrap();

    let (mut editor, doc) = open(&scratch, &map);
    assert!(editor.project_path().is_some(), "the project above the map");
    apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        top(&editor, doc),
        ". 6 .\n. . .",
        "the hole matched the empty cell"
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));

    let hole = editor.custom_types().types[0].id;
    apply(
        &mut editor,
        Intent::EditCustomTypes {
            doc,
            edit: TypeEdit::Rename {
                id: hole,
                name: "Gap".to_owned(),
            },
            merge: None,
        },
    );
    let effects = editor.apply(Intent::Tick { now_ms: 1 });
    let [Effect::LoadRules(job)] = effects.as_slice() else {
        panic!("the rules are read again: {effects:?}");
    };
    editor.apply(Intent::RulesLoaded(Box::new(job.run())));
    let effects = apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(
        notices(&effects),
        [(
            Severity::Warning,
            "AutoMap did nothing: no rule matched".to_owned()
        )],
        "tile 9 is an ordinary tile now"
    );
}

/// The `top` layer of what the canvas shows.
fn shown_top(editor: &Editor, doc: DocumentId) -> String {
    let map = editor.shown_map(doc).unwrap();
    let layer = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    (0..2)
        .map(|y| {
            (0..3)
                .map(|x| match layer.cell(IVec2::new(x, y)).slot() {
                    Some(_) => layer.cell(IVec2::new(x, y)).tile().0.to_string(),
                    None => ".".to_owned(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Paints tile `tile` of the map's tileset at `at` on layer `layer`, as one step.
fn paint(editor: &mut Editor, doc: DocumentId, layer: u32, at: IVec2, tile: u32) {
    let asset = editor.document(doc).unwrap().asset();
    editor
        .transact(doc, "Paint", |tx| {
            let map = tx.map_mut(asset)?;
            let tiles = map
                .layer_mut(LayerId(layer))
                .and_then(|l| l.as_tile_layer_mut())
                .unwrap();
            let cell =
                bt_core::Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(tile));
            tiles.set_cell(at, cell).unwrap();
            Ok::<_, bt_editor::EditError>(())
        })
        .unwrap();
    // The window ticks the editor every frame; that is when the result is made again.
    apply(editor, Intent::Tick { now_ms: 2 });
}

fn is_dirty(editor: &Editor, doc: DocumentId) -> bool {
    let asset = editor.document(doc).unwrap().asset();
    editor.store().get(asset).unwrap().is_dirty()
}

#[test]
fn live_automap_shows_the_rules_result_and_keeps_what_was_painted() {
    let (scratch, map) = folder_with_rules("bt-editor-live", 6);
    let (mut editor, doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    assert_eq!(editor.checked(ActionId::LiveAutoMap), Some(true));
    let setup = editor.document(doc).unwrap().automap().unwrap();
    assert_eq!(setup.rules, scratch.join("rules.txt"));
    assert_eq!(setup.runs[0].tile_layer("ground"), Some(LayerId(1)));
    assert_eq!(setup.runs[0].tile_layer("top"), Some(LayerId(2)));
    assert_eq!(
        shown_top(&editor, doc),
        "6 6 6\n6 6 6",
        "the result is shown"
    );
    assert_eq!(top(&editor, doc), ". . .\n. . .", "what was painted stays");
    assert!(
        is_dirty(&editor, doc),
        "the map's file will hold the result"
    );

    // Painting changes what was painted; the result follows.
    paint(&mut editor, doc, 1, IVec2::new(1, 0), 1);
    assert_eq!(shown_top(&editor, doc), "6 . 6\n6 6 6");
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(
        shown_top(&editor, doc),
        "6 6 6\n6 6 6",
        "undo, and the result follows"
    );

    apply(&mut editor, Intent::Run(ActionId::ShowPainted));
    assert_eq!(
        shown_top(&editor, doc),
        ". . .\n. . .",
        "what was painted, shown"
    );
    apply(&mut editor, Intent::Run(ActionId::ShowPainted));

    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    assert!(editor.document(doc).unwrap().automap().is_none());
    assert_eq!(
        top(&editor, doc),
        "6 6 6\n6 6 6",
        "stopping keeps the result as painted"
    );
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert!(
        editor.document(doc).unwrap().automap().is_some(),
        "undo starts them again"
    );
    assert_eq!(top(&editor, doc), ". . .\n. . .");
    let labels: Vec<String> = editor
        .document(doc)
        .unwrap()
        .history()
        .entries()
        .into_iter()
        .filter(|e| e.done)
        .map(|e| e.label)
        .collect();
    assert_eq!(labels, ["Start AutoMap Layers"]);
}

#[test]
fn starting_live_automap_makes_the_output_layers_the_map_lacks() {
    let scratch = Scratch::new("bt-editor-live-outputs").unwrap();
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("ground", "0 0 0\n0 0 0")],
    );
    write_tiles_map(
        scratch.path(),
        "rule.tmx",
        UVec2::new(1, 1),
        &[("input_ground", "0"), ("output_Over", "6")],
    );
    std::fs::write(scratch.join("rules.txt"), "rule.tmx\n").unwrap();
    let (mut editor, doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    let painted = editor.map_of(doc).unwrap();
    let over = painted.layers.last().unwrap();
    assert_eq!(over.name, "Over", "made on top");
    let setup = editor.document(doc).unwrap().automap().unwrap();
    assert_eq!(setup.runs[0].tile_layer("Over"), Some(over.id));
    let shown = editor.shown_map(doc).unwrap();
    let made = shown.layer(over.id).unwrap().as_tile_layer().unwrap();
    assert_eq!(made.cells().count(), 6);
}

#[test]
fn live_automap_binds_the_names_the_rules_use_to_other_layers() {
    let scratch = Scratch::new("bt-editor-live-bind").unwrap();
    // As in the Highlands pack: the rules read and write `walls`, the map paints on `plat1`.
    let map = write_tiles_map(
        scratch.path(),
        "a.tmx",
        UVec2::new(3, 2),
        &[("walls", ". . .\n. . ."), ("plat1", "0 . 0\n. . .")],
    );
    write_tiles_map(
        scratch.path(),
        "rule.tmx",
        UVec2::new(1, 1),
        &[("input_walls", "0"), ("output_walls", "6")],
    );
    std::fs::write(scratch.join("rules.txt"), "rule.tmx\n").unwrap();
    let (mut editor, doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    apply(
        &mut editor,
        Intent::EditLive {
            doc,
            edit: LiveEdit::Bind {
                run: 0,
                name: "walls".to_owned(),
                objects: false,
                layer: Some(LayerId(2)),
            },
        },
    );
    let shown = editor.shown_map(doc).unwrap();
    let plat1 = shown.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    assert_eq!(
        plat1.cell(IVec2::new(0, 0)).tile().0,
        6,
        "plat1 is read and written"
    );
    let walls = shown.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
    assert_eq!(walls.cells().count(), 0);

    let seed = editor.document(doc).unwrap().automap().unwrap().seed;
    apply(
        &mut editor,
        Intent::EditLive {
            doc,
            edit: LiveEdit::Reroll,
        },
    );
    assert_ne!(editor.document(doc).unwrap().automap().unwrap().seed, seed);
    apply(&mut editor, Intent::Run(ActionId::Undo));
    assert_eq!(editor.document(doc).unwrap().automap().unwrap().seed, seed);
}

#[test]
fn live_automap_started_right_after_automap_keeps_the_map_before_that_run() {
    let (scratch, map) = folder_with_rules("bt-editor-live-after-run", 6);
    let (mut editor, doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::AutoMap));
    assert_eq!(top(&editor, doc), "6 6 6\n6 6 6");
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    assert_eq!(
        top(&editor, doc),
        ". . .\n. . .",
        "what was painted is the map before"
    );
    assert_eq!(shown_top(&editor, doc), "6 6 6\n6 6 6");
}

#[test]
fn a_map_with_live_automap_saves_the_result_and_opens_with_what_was_painted() {
    let (scratch, map) = folder_with_rules("bt-editor-live-save", 6);
    let (mut editor, doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    paint(&mut editor, doc, 2, IVec2::new(2, 1), 3);
    let effects = apply(&mut editor, Intent::Run(ActionId::Save));
    assert_eq!(
        notices(&effects),
        [(Severity::Info, "Saved a.tmx".to_owned())],
        "{effects:?}"
    );
    assert!(!is_dirty(&editor, doc));
    let record = std::fs::read_to_string(scratch.join("better-tiled.json")).unwrap();
    assert!(record.contains("\"rules\": \"rules.txt\""), "{record}");

    // The file holds the rules' result, as Tiled would show it.
    let mut loader = bt_tmx::Loader::default();
    let saved = loader.load_map(&map).unwrap().value;
    let top_saved = saved.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
    assert_eq!(top_saved.cells().count(), 6);
    assert_eq!(
        top_saved.cell(IVec2::new(2, 1)).tile().0,
        6,
        "the rules wrote over it"
    );

    // Opened again: what was painted comes back, with the setup, and nothing is unsaved.
    let mut again = editor_in(&scratch);
    apply(&mut again, Intent::Open(map.clone()));
    apply(&mut again, Intent::Tick { now_ms: 0 });
    let doc = again.current().unwrap();
    assert!(again.document(doc).unwrap().automap().is_some());
    assert_eq!(top(&again, doc), ". . .\n. . 3", "what was painted");
    assert_eq!(shown_top(&again, doc), "6 6 6\n6 6 6");
    assert!(
        !is_dirty(&again, doc),
        "the rules still make what the file holds"
    );

    // Stopped and saved: the record goes.
    apply(&mut again, Intent::Run(ActionId::LiveAutoMap));
    apply(&mut again, Intent::Run(ActionId::Save));
    let record = std::fs::read_to_string(scratch.join("better-tiled.json")).unwrap();
    assert!(!record.contains("rules.txt"), "{record}");
}

#[test]
fn a_map_whose_rules_changed_since_it_was_saved_opens_unsaved() {
    let (scratch, map) = folder_with_rules("bt-editor-live-rules-changed", 6);
    let (mut editor, _doc) = open(&scratch, &map);
    apply(&mut editor, Intent::Run(ActionId::LiveAutoMap));
    apply(&mut editor, Intent::Run(ActionId::Save));
    write_flower_rule(scratch.path(), 7);

    let mut again = editor_in(&scratch);
    apply(&mut again, Intent::Open(map));
    let effects = apply(&mut again, Intent::Tick { now_ms: 0 });
    let doc = again.current().unwrap();
    assert_eq!(shown_top(&again, doc), "7 7 7\n7 7 7");
    assert!(is_dirty(&again, doc));
    assert_eq!(
        notices(&effects),
        [(
            Severity::Info,
            "The AutoMap rules now make something other than what a.tmx holds: saving keeps what \
             they make now"
                .to_owned()
        )]
    );
}

#[test]
fn live_automap_comes_back_with_recovered_work() {
    use std::time::SystemTime;

    use bt_editor::{Choice, orphans};

    let (scratch, map) = folder_with_rules("bt-editor-live-recovery", 6);
    let mut crashed = editor_in(&scratch);
    apply(&mut crashed, Intent::Open(map));
    apply(&mut crashed, Intent::Tick { now_ms: 0 });
    let doc = crashed.current().unwrap();
    apply(&mut crashed, Intent::Run(ActionId::LiveAutoMap));
    paint(&mut crashed, doc, 2, IVec2::new(2, 1), 3);
    let effects = apply(&mut crashed, Intent::Tick { now_ms: 10_000 });
    let [Effect::WriteSnapshot(snapshot)] = effects.as_slice() else {
        panic!("one snapshot: {effects:?}");
    };
    snapshot.write().unwrap();
    drop(crashed);

    let found = orphans(&scratch.join("recovery"), 2000, SystemTime::now());
    assert_eq!(found.len(), 1);
    std::fs::create_dir_all(scratch.join("locks")).unwrap();
    let mut editor = Editor::builder()
        .recovery_folder(scratch.join("recovery"))
        .pid(2000)
        .home(Some(scratch.path().to_path_buf()))
        .lock_folder(scratch.join("locks"))
        .build()
        .unwrap();
    let effects = apply(&mut editor, Intent::Recoverable(found));
    let question = common::ask(&effects).clone();
    apply(
        &mut editor,
        Intent::Decided {
            decision: question.id,
            choice: Choice::Recover,
        },
    );
    apply(&mut editor, Intent::Tick { now_ms: 0 });
    let doc = editor.current().unwrap();
    let setup = editor.document(doc).unwrap().automap().unwrap();
    assert_eq!(setup.rules, scratch.join("rules.txt"));
    assert_eq!(top(&editor, doc), ". . .\n. . 3", "what was painted");
    assert_eq!(shown_top(&editor, doc), "6 6 6\n6 6 6");
    assert!(editor.has_unsaved());
}
