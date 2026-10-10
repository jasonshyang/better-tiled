//! A first picture: tiles of a drawn atlas land on the pixels Tiled puts them on.

#![cfg(test)]

use std::sync::Arc;

use bt_core::TileId;
use bt_render::{HeadlessDevice, Render};
use bt_test_data::fixtures::{Atlas, MARKER, MapBuilder};
use glam::UVec2;

#[test]
fn an_orthogonal_map_draws_its_tiles_where_they_are() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 2))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "0 1 2\n3 . 5h")
        .build()
        .unwrap();
    let picture = Render::of(Arc::new(map)).run(device).unwrap();
    assert_eq!((picture.width, picture.height), (48, 32));
    for (cell, tile) in [((0, 0), 0), ((1, 0), 1), ((2, 0), 2), ((0, 1), 3)] {
        let at = |x: u32, y: u32| picture.pixel(cell.0 * 16 + x, cell.1 * 16 + y).unwrap();
        assert_eq!(at(0, 0), MARKER, "tile {tile}'s marker");
        assert_eq!(at(10, 10), Atlas::tile_color(TileId(tile)), "tile {tile}");
    }
    assert_eq!(
        picture.pixel(16 + 8, 16 + 8),
        Some([0, 0, 0, 0]),
        "an empty cell"
    );
    // Mirrored: the marker is at the top-right.
    assert_eq!(picture.pixel(32 + 15, 16), Some(MARKER));
    assert_eq!(picture.pixel(32, 16), Some(Atlas::tile_color(TileId(5))));
}

fn device() -> Option<&'static HeadlessDevice> {
    match HeadlessDevice::acquire() {
        Ok(device) => Some(device),
        Err(error) => {
            bt_test_data::missing("a GPU adapter", &error.to_string());
            None
        }
    }
}
