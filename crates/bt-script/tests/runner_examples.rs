//! The runner on Tiled's examples (read-only, from `reference/tiled/examples`) and on the
//! Highlands pack when it is there: the sewers' rules beside the map run once and as AutoMap
//! layers started by a script, the desert is painted, a hexagonal map takes tiles and objects,
//! and the Highlands rules reach their outputs through `walls=ground`. Nothing is written.

#![cfg(test)]

mod common;

use std::path::Path;
use std::sync::Arc;

use bt_core::{Map, ObjectId};
use bt_script::{EditResult, Names, RunError, RunFailure, RunInput, parse_script, run_script};
use bt_tmx::Loader;
use common::*;
use glam::IVec2;

/// Reads a map through a loader the script then shares.
fn read(path: &Path) -> (Map, Loader) {
    let mut loader = Loader::default();
    let map = loader.load_map(path).unwrap().value;
    (map, loader)
}

/// Runs `script` on `map` at `path`.
fn edit(map: &Map, path: &Path, loader: &mut Loader, script: &str) -> Result<EditResult, RunError> {
    let script = parse_script(script).unwrap();
    let input = RunInput::builder(Arc::new(map.clone()), path).build();
    run_script(&script, &input, loader)
}

/// The number of matches an `automap` step noted.
fn matches_noted(result: &EditResult) -> usize {
    result
        .outcome
        .notes
        .iter()
        .find_map(|note| {
            let text = note.text.strip_prefix("automap: ")?;
            let count = text
                .strip_suffix(" rule matches")
                .or_else(|| text.strip_suffix(" rule match"))?;
            count.parse().ok()
        })
        .unwrap_or_else(|| panic!("no count of matches in {:?}", notes(result)))
}

#[test]
fn the_sewers_rules_beside_the_map_run_once() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let path = examples.join("sewer_automap/sewers.tmx");
    let (map, mut loader) = read(&path);
    let result = edit(&map, &path, &mut loader, "automap").unwrap();
    assert!(matches_noted(&result) > 0, "{:?}", notes(&result));
    assert!(
        notes(&result)
            .iter()
            .any(|n| n.contains("automap: new layers for the rules' outputs:")),
        "{:?}",
        notes(&result)
    );
    let names: Vec<&str> = result
        .outcome
        .changes
        .layers
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    assert!(names.contains(&"Ground"), "{names:?}");
    assert!(result.setup.is_none() && result.outcome.painted.is_none());
    // The rules read `set`: on a map without it they say so, with the near miss.
    let mut renamed = map.clone();
    renamed.layers[0].name = "Set".into();
    let missed = edit(&renamed, &path, &mut loader, "automap").unwrap();
    let texts = notes(&missed);
    assert!(
        texts
            .iter()
            .any(|n| n.contains("the rules read the layer set, which this map doesn't have")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|n| n.ends_with("the map has a layer Set, not set (names must match exactly)")),
        "{texts:?}"
    );
    // `set=Set` binds the rules' name to the map's layer.
    let bound = edit(&renamed, &path, &mut loader, "automap set=Set").unwrap();
    assert_eq!(matches_noted(&bound), matches_noted(&result));
}

#[test]
fn the_sewers_automap_layers_started_by_a_script_make_the_result_from_what_was_painted() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let path = examples.join("sewer_automap/sewers.tmx");
    let (map, mut loader) = read(&path);
    let started = edit(&map, &path, &mut loader, "automap layers").unwrap();
    let setup = started.setup.clone().unwrap();
    assert!(started.setup_changed);
    assert_eq!(setup.runs.len(), 1);
    assert_ne!(
        *started.shown, *started.painted,
        "the file gets what the rules make"
    );
    let painted = started.outcome.painted.as_ref().unwrap();
    assert!(
        painted.layers.iter().all(|l| l.new && l.cells.is_none()),
        "what was painted gained the output layers, empty"
    );
    assert!(
        started
            .outcome
            .changes
            .layers
            .iter()
            .any(|l| l.name == "Ground" && l.cells.is_some()),
        "the changes are the rules' result"
    );
    assert!(
        notes(&started)
            .iter()
            .any(|n| n.contains("automap layers: on (1 run"))
    );

    // The next edit paints what was painted; the result is what the rules make of it.
    let rules = Arc::new(bt_automap::files::load(&mut loader, &setup.rules, Some(&path)).unwrap());
    let script = parse_script("clear rect 0,0 31x31 layer set\nautomap").unwrap();
    let input = RunInput::builder(Arc::clone(&started.painted), &path)
        .automap_layers(
            setup.clone(),
            Arc::clone(&rules),
            Arc::clone(&started.shown),
        )
        .build();
    let cleared = run_script(&script, &input, &mut loader).unwrap();
    assert!(notes(&cleared).iter().any(|n| n.ends_with(
        "automap: the map has AutoMap layers, whose rules run after every edit anyway"
    )));
    let ground = cleared
        .shown
        .layer(layer_id(&cleared.shown, "Ground"))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    assert_eq!(ground.cells().count(), 0, "nothing painted, nothing made");
    assert!(
        cleared
            .outcome
            .painted
            .as_ref()
            .is_some_and(|p| !p.is_empty())
    );

    // An object the rules placed can't be edited.
    let placed: Vec<ObjectId> = object_ids(&started.shown)
        .into_iter()
        .filter(|id| started.painted.object(*id).is_none())
        .collect();
    if let Some(id) = placed.first() {
        let script = parse_script(&format!("delete #{}", id.0)).unwrap();
        let error = run_script(&script, &input, &mut loader).unwrap_err();
        assert!(
            matches!(*error.failure, RunFailure::RuleMade { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("automap layers off"), "{error}");
    }

    // Off: what the rules made stays, as painted tiles.
    let script = parse_script("automap layers off").unwrap();
    let off = run_script(&script, &input, &mut loader).unwrap();
    assert!(off.setup.is_none() && off.setup_changed);
    assert_eq!(*off.painted, *started.shown);
    assert!(
        off.outcome.changes.is_empty(),
        "the map shows what it showed"
    );
    assert!(off.outcome.changed);
}

#[test]
fn the_desert_is_painted_cell_for_cell() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let path = examples.join("desert.tmx");
    let (map, mut loader) = read(&path);
    let names = Names::of(&map);
    let covered_by = |map: &Map, at: IVec2, terrain: &str| {
        let cell = map.layers[0].as_tile_layer().unwrap().cell(at);
        names
            .terrain_covering(map, cell)
            .is_some_and(|t| t.name == terrain)
    };
    // Somewhere all sand, with room around.
    let corner = (0..34)
        .flat_map(|y| (0..34).map(move |x| IVec2::new(x, y)))
        .find(|corner| {
            (0..6)
                .all(|dy| (0..7).all(|dx| covered_by(&map, *corner + IVec2::new(dx, dy), "desert")))
        })
        .expect("the desert has open sand");
    let rect = corner + IVec2::ONE;
    let script = format!("paint cobblestone rect {},{} 4x3", rect.x, rect.y);
    let result = edit(&map, &path, &mut loader, &script).unwrap();
    let covered: Vec<IVec2> = (corner.y..corner.y + 6)
        .flat_map(|y| (corner.x..corner.x + 7).map(move |x| IVec2::new(x, y)))
        .filter(|at| covered_by(&result.shown, *at, "cobblestone"))
        .collect();
    let expected: Vec<IVec2> = (rect.y..rect.y + 3)
        .flat_map(|y| (rect.x..rect.x + 4).map(move |x| IVec2::new(x, y)))
        .collect();
    assert_eq!(covered, expected, "exactly the cells painted");
    assert!(result.outcome.unfilled.is_empty(), "{:?}", notes(&result));
}

#[test]
fn a_hexagonal_map_takes_tiles_and_objects_where_they_are_put() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    let path = examples.join("hexagonal-mini.tmx");
    let (map, mut loader) = read(&path);
    let result = edit(
        &map,
        &path,
        &mut loader,
        "tile hex-mini:10 rect 2,2 3x2\nlayer add things objects\nobject spawn at 3.5,3.5\nobject \
         exit at 7.5,8\nmove spawn by 0,2",
    )
    .unwrap();
    let ground = result.shown.layers[0].as_tile_layer().unwrap();
    for cell in [IVec2::new(2, 2), IVec2::new(4, 3)] {
        assert_eq!(ground.cell(cell).tile().0, 10);
    }
    let things = objects(&result.shown, "things");
    let cells: Vec<IVec2> = things
        .iter()
        .map(|o| standing_in(&result.shown, o))
        .collect();
    assert_eq!(cells, [IVec2::new(3, 5), IVec2::new(7, 7)]);
}

#[test]
fn the_highlands_rules_reach_their_outputs_through_bindings() {
    let Some(pack) = bt_test_data::test_data("assets/Epic RPG World - Highlands V1.4.1") else {
        return;
    };
    let path = pack.join("TiledMap Editor/Sample scene.tmx");
    let (mut map, mut loader) = read(&path);
    map.layers.clear();
    let names = Names::of(&map);
    let platform = names
        .terrains()
        .iter()
        .find(|t| t.name.starts_with("platformv2-3tiles"))
        .map(|t| t.name.clone())
        .unwrap_or_else(|| panic!("terrains: {:?}", names.terrains()));
    let painted = format!("layer add ground tiles\npaint {platform} rect 5,5 10x8 layer ground");
    let unbound = edit(
        &map,
        &path,
        &mut loader,
        &format!("{painted}\nautomap rules.txt"),
    )
    .unwrap();
    assert_eq!(matches_noted(&unbound), 0);
    assert!(
        notes(&unbound)
            .iter()
            .any(|n| n.contains("the rules read the layer walls, which this map doesn't have")),
        "{:?}",
        notes(&unbound)
    );
    let bound = edit(
        &map,
        &path,
        &mut loader,
        &format!("{painted}\nautomap rules.txt walls=ground"),
    )
    .unwrap();
    assert!(matches_noted(&bound) > 0, "{:?}", notes(&bound));
    assert_ne!(
        bound.shown.layers, unbound.shown.layers,
        "the rules wrote their tiles through the binding"
    );
}

/// Every object id of a map.
fn object_ids(map: &Map) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            ids.extend(objects.objects.iter().map(|o| o.id));
        }
    });
    ids
}
