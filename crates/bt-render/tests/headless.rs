//! Headless pictures: banded renders equal one-piece renders, a picture covers the map's
//! bounding rectangle, a scale of 2 doubles every texel, packing under memory pressure keeps
//! what a frame uses, images larger than a page are split, and animations say when they change.

#![cfg(test)]

mod common;

use std::sync::Arc;

use bt_core::{Frame, TileId};
use bt_image::ImageStore;
use bt_render::{Camera, FrameInput, MapRenderer, Render};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use common::{cpu_reference, device, differing_share};
use glam::{DVec2, IVec2, UVec2};

fn busy_map() -> (Atlas, bt_core::Map) {
    let atlas = Atlas::builder()
        .tile_size(16, 16)
        .grid(4, 4)
        .build_embedded()
        .unwrap();
    let grid: Vec<String> = (0..9)
        .map(|y| {
            (0..13)
                .map(|x| ((x * 3 + y * 5) % 16).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(13, 9))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", &grid.join("\n"))
        .build()
        .unwrap();
    (atlas, map)
}

#[test]
fn bands_stitch_into_the_whole_picture() {
    let Some(device) = device() else { return };
    let (_, map) = busy_map();
    let map = Arc::new(map);
    let whole = Render::of(Arc::clone(&map)).run(device).unwrap();
    let banded = Render::of(map).band_side(37).run(device).unwrap();
    assert_eq!(whole, banded);
}

#[test]
fn a_picture_covers_the_maps_bounding_rectangle() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder().build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
        .infinite()
        .tileset(atlas.tileset.clone())
        .tile_layer_at("far", IVec2::new(-20, 35), "1 2")
        .build()
        .unwrap();
    let render = Render::of(Arc::new(map.clone()));
    // The chunks the cells are in: (-2, 2) here, 16 cells a side.
    assert_eq!(render.origin(), DVec2::new(-32.0 * 16.0, 32.0 * 16.0));
    assert_eq!(render.size(), UVec2::splat(16 * 16));
    let picture = render.run(device).unwrap();
    let at = (IVec2::new(-20, 35) * 16 - IVec2::new(-32, 32) * 16).as_uvec2();
    assert_eq!(
        picture.pixel(at.x + 10, at.y + 10),
        Some(Atlas::tile_color(TileId(1)))
    );
}

#[test]
fn a_scale_of_two_doubles_every_texel() {
    let Some(device) = device() else { return };
    let (_, map) = busy_map();
    let map = Arc::new(map);
    let one = Render::of(Arc::clone(&map)).run(device).unwrap();
    let two = Render::of(map).scale(2.0).run(device).unwrap();
    assert_eq!((two.width, two.height), (one.width * 2, one.height * 2));
    for y in 0..one.height {
        for x in 0..one.width {
            let expected = one.pixel(x, y);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                assert_eq!(two.pixel(2 * x + dx, 2 * y + dy), expected, "({x}, {y})");
            }
        }
    }
}

#[test]
fn packing_under_pressure_keeps_what_the_frame_draws() {
    let Some(device) = device() else { return };
    // Pages of 128 texels and room for one: each atlas below takes most of a page, and the
    // tall one is larger than a page and split by tile rows.
    let mut renderer = MapRenderer::builder(
        device.device(),
        device.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
    )
    .page_side(128)
    .budget(128 * 128 * 5 * 4 / 3)
    .build();
    let mut store = ImageStore::builder().build().unwrap();
    let maps: Vec<bt_core::Map> = [(4, 4), (4, 4), (2, 12)]
        .into_iter()
        .enumerate()
        .map(|(i, (columns, rows))| {
            let atlas = Atlas::builder()
                .name(&format!("atlas{i}"))
                .tile_size(24, 24)
                .grid(columns, rows)
                .build_embedded()
                .unwrap();
            let count = columns * rows;
            let grid: Vec<String> = (0..4)
                .map(|y| {
                    (0..6)
                        .map(|x| ((x + y * 6) % count).to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            MapBuilder::new(UVec2::new(24, 24), UVec2::new(6, 4))
                .tileset(atlas.tileset.clone())
                .tile_layer("ground", &grid.join("\n"))
                .build()
                .unwrap()
        })
        .collect();
    // Twice round, so pages packed for one map are evicted for the next and packed again.
    for round in 0..2 {
        for (i, map) in maps.iter().enumerate() {
            let picture = Render::of(Arc::new(map.clone()))
                .run_with(device.device(), device.queue(), &mut renderer, &mut store)
                .unwrap();
            let share = differing_share(&picture, &cpu_reference(map), 2);
            assert!(
                share == 0.0,
                "round {round}, map {i}: {:.2} % differ",
                share * 100.0
            );
        }
    }
}

#[test]
fn animations_show_their_frame_and_say_when_the_next_is_due() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder()
        .animation(
            TileId(0),
            vec![
                Frame {
                    tile: TileId(5),
                    duration_ms: 100,
                },
                Frame {
                    tile: TileId(6),
                    duration_ms: 50,
                },
            ],
        )
        .build_embedded()
        .unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(2, 1))
        .tileset(atlas.tileset.clone())
        .tile_layer("water", "0 1")
        .build()
        .unwrap();
    let map = Arc::new(map);
    let at = |time_ms| {
        Render::of(Arc::clone(&map))
            .time_ms(time_ms)
            .run(device)
            .unwrap()
            .pixel(8, 8)
    };
    assert_eq!(at(0), Some(Atlas::tile_color(TileId(5))));
    assert_eq!(at(120), Some(Atlas::tile_color(TileId(6))));
    assert_eq!(at(150), Some(Atlas::tile_color(TileId(5))));

    let mut renderer = MapRenderer::builder(
        device.device(),
        device.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
    )
    .build();
    let mut store = ImageStore::builder().build().unwrap();
    let camera = Camera {
        origin: DVec2::ZERO,
        zoom: 1.0,
    };
    let frame =
        |time_ms| FrameInput::new(Arc::clone(&map), camera, UVec2::new(32, 16)).time_ms(time_ms);
    let stats = renderer.prepare(&mut store, &[frame(120)]).unwrap();
    assert!(stats.animating);
    assert_eq!(stats.next_frame_ms, Some(150));
    let still = MapBuilder::new(UVec2::new(16, 16), UVec2::new(2, 1))
        .tileset(Atlas::builder().build_embedded().unwrap().tileset)
        .tile_layer("ground", "0 1h")
        .build()
        .unwrap();
    let still = FrameInput::new(Arc::new(still), camera, UVec2::new(32, 16));
    let stats = renderer.prepare(&mut store, &[still]).unwrap();
    assert!(!stats.animating && stats.next_frame_ms.is_none());
}

#[test]
fn far_out_tile_layers_are_drawn_as_impostors_that_match_the_tiles() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder()
        .tile_size(16, 16)
        .grid(4, 4)
        .build_embedded()
        .unwrap();
    let grid: Vec<String> = (0..64)
        .map(|y| {
            (0..64)
                .map(|x| ((x / 3 + y / 5 + x * y) % 16).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(64, 64))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", &grid.join("\n"))
        .build()
        .unwrap();
    let map = Arc::new(map);
    // 16-pixel cells at a quarter: 4 pixels on screen, drawn through impostors at that scale.
    let far = Render::of(Arc::clone(&map))
        .scale(0.25)
        .run(device)
        .unwrap();
    let near = Render::of(Arc::clone(&map)).run(device).unwrap();
    // The tiles at full size, averaged 4×4 (premultiplied), are what the impostor should show.
    let mut expected = vec![0u8; (far.width * far.height * 4) as usize];
    for y in 0..far.height {
        for x in 0..far.width {
            let mut sum = [0u32; 4];
            for dy in 0..4 {
                for dx in 0..4 {
                    let p = near.pixel(4 * x + dx, 4 * y + dy).unwrap();
                    let a = u32::from(p[3]);
                    for c in 0..3 {
                        sum[c] += u32::from(p[c]) * a / 255;
                    }
                    sum[3] += a;
                }
            }
            let a = sum[3] / 16;
            let at = ((y * far.width + x) * 4) as usize;
            for c in 0..3 {
                expected[at + c] = ((sum[c] / 16) * 255)
                    .checked_div(a)
                    .map_or(0, |v| v.min(255) as u8);
            }
            expected[at + 3] = a as u8;
        }
    }
    let expected = bt_render::Picture {
        width: far.width,
        height: far.height,
        rgba: expected,
    };
    let share = differing_share(&far, &expected, 24);
    assert!(share < 0.02, "{:.2} % of pixels differ", share * 100.0);

    let mut renderer = MapRenderer::builder(
        device.device(),
        device.queue(),
        wgpu::TextureFormat::Rgba8Unorm,
    )
    .build();
    let mut store = ImageStore::builder().build().unwrap();
    let camera = Camera {
        origin: DVec2::ZERO,
        zoom: 0.25,
    };
    let frame = FrameInput::new(Arc::clone(&map), camera, UVec2::new(256, 256));
    renderer
        .prepare(&mut store, std::slice::from_ref(&frame))
        .unwrap();
    let stats = renderer.prepare(&mut store, &[frame]).unwrap();
    // The map is 1024 world pixels a side: four squares of 128 texels at a quarter.
    assert_eq!(stats.impostors, 4, "one per square");
    assert_eq!(stats.instances, 4);
    assert_eq!(stats.pending_impostors, 0);
}

#[test]
fn far_out_squares_stack_oversized_tiles_across_their_edges_as_tiled_does() {
    let Some(device) = device() else { return };
    let ground = Atlas::builder().name("ground").build_embedded().unwrap();
    let tall = Atlas::builder()
        .name("tall")
        .tile_size(16, 32)
        .grid(4, 2)
        .build_embedded()
        .unwrap();
    // At a quarter, a square is 512 world pixels: 32 cells. A tall tile of the lower layer in
    // the first row of the second square reaches up into the first square, where a tile of the
    // upper layer must stay on top of it.
    let grid = |row: usize, cell: &str| -> String {
        (0..40)
            .map(|y| {
                if y == row {
                    format!(". {cell} . .")
                } else {
                    ". . . .".to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 40))
        .tileset(ground.tileset.clone())
        .tileset(tall.tileset.clone())
        .tile_layer("low", &grid(32, "5@1"))
        .tile_layer("high", &grid(31, "1"))
        .build()
        .unwrap();
    let picture = Render::of(Arc::new(map)).scale(0.25).run(device).unwrap();
    let high = Atlas::tile_color(bt_core::TileId(1));
    let low = Atlas::tile_color(bt_core::TileId(5));
    // Row 31 is pixels 124..128, row 32 is 128..132; column 1 is pixels 4..8.
    assert_eq!(
        picture.pixel(6, 126),
        Some(high),
        "the upper layer stays on top"
    );
    assert_eq!(picture.pixel(6, 130), Some(low), "the tall tile's own cell");
}
