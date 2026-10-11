//! What the commands' tests share: `bt` run offline as typed, Tiled's examples (read-only), a
//! small map written to a test's folder, and a map with AutoMap layers saved as `bt edit` saves
//! one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{MapObject, Shape, Tileset, WangSetType};
use bt_project::{EntryPlace, ProjectFile};
use bt_script::{RunInput, parse_script, run_script};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
use bt_tmx::{Loader, WriteRequest};
use glam::{DVec2, UVec2};

use crate::exit::Printed;
use crate::folders::Folders;
use crate::run::Run;

/// `bt` with `args` (without the program's name), offline, through the command table and the
/// dispatch.
pub(crate) fn bt(args: &[&str]) -> anyhow::Result<Printed> {
    crate::run::run(&Run::of_args(args, Folders::default()))
}

/// A path as an argument.
pub(crate) fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Tiled's examples, or `None` (the test returns) when they are missing and skipping is on.
pub(crate) fn examples() -> Option<PathBuf> {
    bt_test_data::test_data("reference/tiled/examples")
}

/// Makes the file at `path` read-only, as version control locks a file.
pub(crate) fn lock(path: &Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).unwrap();
}

/// A 10×6 map written as `pond.tmx` in `folder`, with the pictures of its tilesets beside it:
/// grass with a pond of water at 3,2 4x2 on `ground` (a corner set “Land” of the tileset
/// `ground`), tile 2 of `things` at 1,0 and 1,1 on `deco` above it, and a spawn point standing in
/// cell 8,5 on `design`.
pub(crate) fn pond(folder: &Path) -> PathBuf {
    let set = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Land")
        .terrains(&["Grass", "Water"])
        .every_combination();
    let ground = Atlas::builder().name("ground").build_in(folder).unwrap();
    let ground = Arc::new(set.added_to(Tileset::clone(&ground.tileset)));
    let things = Atlas::builder().name("things").build_in(folder).unwrap();
    let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0;
    let water = combination_tile(2, [2, 2, 2, 2]).unwrap().0;
    let rows: Vec<String> = (0..6)
        .map(|y| {
            (0..10)
                .map(|x| {
                    let pond = (3..7).contains(&x) && (2..4).contains(&y);
                    if pond { water } else { grass }.to_string()
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let spawn = MapObject {
        class: "spawn".into(),
        shape: Shape::Point,
        pos: DVec2::new(8.5, 5.5) * 16.0,
        ..MapObject::default()
    };
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(10, 6))
        .tileset(ground)
        .tileset(Arc::clone(&things.tileset))
        .tile_layer("ground", &rows.join("\n"))
        .tile_layer("deco", ". 2@1 . . . . . . . .\n. 2@1 . . . . . . . .")
        .object_layer("design", vec![spawn])
        .build()
        .unwrap();
    let path = folder.join("pond.tmx");
    WriteRequest::for_map(&map, &path).write().unwrap();
    path
}

/// The sewers of Tiled's examples (`sewer_automap`, with the picture they share) copied into
/// `scratch`, with AutoMap layers started on them by the edit step `automap layers` and saved as
/// `bt edit` saves them: the map file gets what the rules make, and `better-tiled.json` beside it
/// what was painted. `None` (the test returns) when the examples are missing and skipping is on.
pub(crate) fn sewers_with_automap_layers(scratch: &Scratch) -> Option<PathBuf> {
    let examples = examples()?;
    let folder = scratch
        .mirror(&examples.join("sewer_automap"), "examples/sewer_automap")
        .unwrap();
    std::fs::copy(
        examples.join("sewer_tileset.png"),
        scratch.join("examples/sewer_tileset.png"),
    )
    .unwrap();
    let path = folder.join("sewers.tmx");
    let mut loader = Loader::default();
    let map = loader.load_map(&path).unwrap().value;
    let script = parse_script("automap layers").unwrap();
    let input = RunInput::builder(Arc::new(map), &path).build();
    let result = run_script(&script, &input, &mut loader).unwrap();
    WriteRequest::for_map(&result.shown, &path)
        .loader(&loader)
        .write()
        .unwrap();
    let setup = result.setup.unwrap();
    let place = EntryPlace::for_map(&path, None, None).unwrap();
    let rules = bt_project::paths::key(&place.file, &setup.rules).unwrap();
    let record = bt_automap::layers::painted_record(&setup, rules, &result.layers.unwrap());
    let locks = scratch.join("locks");
    std::fs::create_dir_all(&locks).unwrap();
    ProjectFile::edit(&place.file, &locks, |document| {
        document.set_automap(&place.key, record);
    })
    .unwrap();
    Some(path)
}
