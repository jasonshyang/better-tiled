//! Real maps exercise loading, deterministic scene generation and Godot's import pipeline.
use super::support::godot;
use crate::{Exporter, Options};
use bt_test_data::Scratch;
use std::path::Path;
use std::time::Instant;

const EXAMPLES: [&str; 14] = [
    "desert.tmx",
    "forest/forest.tmx",
    "orthogonal-outside.tmx",
    "perspective_walls.tmx",
    "rpg/island.tmx",
    "sewers.tmx",
    "sewer_automap/sewers.tmx",
    "sticker-knight/map/sandbox.tmx",
    "sticker-knight/map/sandbox2.tmx",
    "sticker-knight/ui/title.json",
    "isometric_grass_and_water.tmx",
    "isometric_staggered_grass_and_water.tmx",
    "hexagonal-mini.tmx",
    "test_hexagonal_tile_60x60x30.tmx",
];
#[test]
fn all_example_maps_export_deterministically_and_load_in_godot() {
    let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
        return;
    };
    real_maps(&examples, &EXAMPLES, "examples");
}
#[test]
fn both_highlands_maps_export_deterministically_and_load_in_godot() {
    let Some(pack) =
        bt_test_data::test_data("assets/Epic RPG World - Highlands V1.4.1/TiledMap Editor")
    else {
        return;
    };
    real_maps(
        &pack,
        &["Sample scene.tmx", "Highlands example map.tmx"],
        "highlands",
    );
}
fn real_maps(source: &Path, maps: &[&str], name: &str) {
    let Some(godot) = godot() else {
        return;
    };
    let scratch = Scratch::new(&format!("godot-real-{name}")).unwrap();
    for project in ["first", "second"] {
        std::fs::create_dir_all(scratch.join(project)).unwrap();
        std::fs::write(scratch.join(project).join("project.godot"),"config_version=5\n[application]\nconfig/name=\"bt tests\"\nconfig/features=PackedStringArray(\"4.7\",\"GL Compatibility\")\n").unwrap();
    }
    let mut loader = bt_tmx::Loader::builder().build();
    let mut exporter = Exporter::new();
    for (index, name) in maps.iter().enumerate() {
        let path = source.join(name);
        let loaded = loader.load_map(&path).unwrap();
        let relative = format!("levels/map{index}.tscn");
        let first = scratch.join("first").join(&relative);
        let second = scratch.join("second").join(&relative);
        let report = exporter
            .export_map(
                &loaded.value,
                &path,
                loader.custom_types(),
                &first,
                &Options::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        crate::export(&path, &mut loader, &second, &Options::default()).unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap(),
            "{name}: disk and in-memory exports agree"
        );
        let start = Instant::now();
        let again = exporter
            .export_map(
                &loaded.value,
                &path,
                loader.custom_types(),
                &first,
                &Options::default(),
            )
            .unwrap();
        assert!(again.up_to_date(), "{name}: {:?}", again.files);
        let repeat_time = start.elapsed();
        assert!(
            repeat_time.as_secs() < 60,
            "{name}: repeat export unexpectedly slow"
        );
        let verdict = crate::verify::verify(&godot, &first).unwrap();
        assert!(verdict.clean(), "{name}: {}", verdict.text());
        assert_eq!(
            verdict.cells, report.counts.cells,
            "{name}: Godot finds every exported cell"
        );
        eprintln!(
            "Godot {name}: {} cells; repeat {:?}",
            report.counts.cells, repeat_time
        );
    }
}
