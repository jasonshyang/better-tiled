//! A map image can itself contain a map image; the image store guards recursive references.

#![cfg(test)]

use std::sync::Arc;

use bt_core::{FileRef, ImageLayer, ImageRef, TileId};
use bt_image::{ImageInput, ImageRequest, ImageStore};
use bt_render::{HeadlessDevice, MapImages, Render};
use bt_test_data::{
    Scratch,
    fixtures::{Atlas, MapBuilder},
};
use glam::UVec2;

fn store() -> ImageStore {
    ImageStore::builder()
        .map_images(Arc::new(MapImages::new(|path| {
            let map = bt_tmx::Loader::default()
                .load_map(path)
                .map_err(|error| error.to_string())?
                .value;
            Ok(Render::of(Arc::new(map)))
        })))
        .build()
        .unwrap()
}

fn image_layer(path: &std::path::Path) -> ImageLayer {
    ImageLayer {
        image: Some(ImageRef::file(FileRef::new("", path), UVec2::splat(16))),
        ..ImageLayer::default()
    }
}

#[test]
fn nested_map_images_share_the_outer_gpu_without_deadlocking() {
    let scratch = Scratch::new("render-map-images").unwrap();
    let gpu = HeadlessDevice::acquire().unwrap();
    let atlas = Atlas::builder().build_in(scratch.path()).unwrap();
    let tiles = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
        .tileset(atlas.tileset)
        .tile_layer("ground", "2")
        .build()
        .unwrap();
    let tiles_path = scratch.join("tiles.tmx");
    bt_tmx::WriteRequest::for_map(&tiles, &tiles_path)
        .write()
        .unwrap();
    let middle = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
        .image_layer("nested", image_layer(&tiles_path))
        .build()
        .unwrap();
    let middle_path = scratch.join("middle.tmj");
    bt_tmx::WriteRequest::for_map(&middle, &middle_path)
        .write()
        .unwrap();
    let outer = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
        .image_layer("nested", image_layer(&middle_path))
        .build()
        .unwrap();
    let picture = Render::of(Arc::new(outer))
        .run_with_images(gpu, &mut store())
        .unwrap();
    assert_eq!(picture.pixel(10, 10), Some(Atlas::tile_color(TileId(2))));
    assert_eq!(picture.pixel(0, 0), Some(bt_test_data::fixtures::MARKER));
}

#[test]
fn recursive_map_images_fail_instead_of_caching_a_missing_image_marker() {
    let scratch = Scratch::new("render-map-image-loop").unwrap();
    let path = scratch.join("loop.tmx");
    let map = MapBuilder::new(UVec2::splat(16), UVec2::ONE)
        .image_layer("loop", image_layer(&path))
        .build()
        .unwrap();
    bt_tmx::WriteRequest::for_map(&map, &path).write().unwrap();
    let error = store()
        .decode_now(ImageRequest {
            input: ImageInput::File(path),
            transparent: None,
        })
        .unwrap_err();
    assert!(error.to_string().contains("drawn with itself"), "{error}");
}
