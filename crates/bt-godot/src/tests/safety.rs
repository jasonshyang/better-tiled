//! Invalid input never silently changes the planned resources or replaces output files.
use bt_core::{Color, EmbeddedImage, ImageSource, Properties, Property, TilesetImage, Value};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::UVec2;
use std::sync::Arc;

#[test]
fn embedded_images_and_colour_keys_are_tracked_png_resources() {
    let scratch = Scratch::new("godot-embedded-image").unwrap();
    std::fs::write(scratch.join("project.godot"), "config_version=5\n").unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let mut tileset = (*atlas.tileset).clone();
    let TilesetImage::Atlas { image, .. } = &mut tileset.image else {
        panic!("atlas")
    };
    let file = image.path().unwrap();
    let bytes = std::fs::read(file).unwrap();
    let pixels = bt_image::decode_bytes(&bytes, "png", None).unwrap();
    image.transparent = Some(Color::rgba(
        pixels.rgba[0],
        pixels.rgba[1],
        pixels.rgba[2],
        255,
    ));
    image.source = ImageSource::Embedded(EmbeddedImage {
        format: "png".into(),
        bytes: bytes.into(),
    });
    let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .tileset(Arc::new(tileset))
        .tile_layer("ground", "0 1")
        .build()
        .unwrap();
    let path = scratch.join("map.tmx");
    let scene = scratch.join("levels/map.tscn");
    let report = crate::export_map(
        &map,
        &path,
        &bt_core::CustomTypes::default(),
        &scene,
        &crate::Options::default(),
    )
    .unwrap();
    assert_eq!(report.counts.images, 1);
    let copy = report
        .files
        .iter()
        .find(|f| f.state == crate::State::Copied)
        .unwrap();
    let pixels = bt_image::decode_file(&copy.path, None).unwrap();
    assert_eq!(pixels.rgba[3], 0);
    let repeated = crate::export_map(
        &map,
        &path,
        &bt_core::CustomTypes::default(),
        &scene,
        &crate::Options::default(),
    )
    .unwrap();
    assert!(repeated.up_to_date());
}
#[test]
fn malformed_registries_and_nonfinite_values_write_nothing() {
    let scratch = Scratch::new("godot-invalid-input").unwrap();
    std::fs::write(scratch.join("project.godot"), "config_version=5\n").unwrap();
    let path = scratch.join("map.tmx");
    let scene = scratch.join("levels/map.tscn");
    let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
        .tile_layer("ground", ". .")
        .build()
        .unwrap();
    map.properties = Properties::from([("bad".into(), Property::new(Value::Float(f64::NAN)))]);
    let failed = crate::export_map(
        &map,
        &path,
        &bt_core::CustomTypes::default(),
        &scene,
        &crate::Options::default(),
    )
    .unwrap_err();
    assert!(failed.to_string().contains("finite"));
    assert!(!scene.exists());
    map.properties.clear();
    std::fs::create_dir_all(scratch.join("levels/images")).unwrap();
    std::fs::write(scratch.join("levels/images/.sources.json"), "broken JSON").unwrap();
    let failed = crate::export_map(
        &map,
        &path,
        &bt_core::CustomTypes::default(),
        &scene,
        &crate::Options::default(),
    )
    .unwrap_err();
    assert!(failed.to_string().contains("registry"));
    assert!(!scene.exists());
}
