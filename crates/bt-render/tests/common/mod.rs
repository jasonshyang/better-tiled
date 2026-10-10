//! What bt-render's tests share: the GPU device, expected tile pixels, an independent CPU
//! compositor of Tiled's tile drawing, and picture comparisons.

#![allow(dead_code, reason = "each test file uses its own part")]

use std::collections::HashMap;
use std::path::PathBuf;

use bt_core::{Flip, ImageSource, LayerKind, Map, TileId, TilesetImage};
use bt_image::{ColorKey, DecodedImage, decode_bytes, decode_file};
use bt_render::{HeadlessDevice, Picture, Render};
use bt_test_data::fixtures::Atlas;
use bt_utils::DRect;
use glam::{DVec2, IVec2, UVec2};

/// The headless device, or `None` (the test returns) when there is no GPU and skipping is on.
pub fn device() -> Option<&'static HeadlessDevice> {
    match HeadlessDevice::acquire() {
        Ok(device) => Some(device),
        Err(error) => {
            bt_test_data::missing("a GPU adapter", &error.to_string());
            None
        }
    }
}

/// The colour a tile of `atlas` drawn with `flip` shows at `local` of its `size` quad.
pub fn tile_pixel(atlas: &Atlas, tile: u32, flip: Flip, local: UVec2, size: UVec2) -> [u8; 4] {
    let mut u = local.x;
    let mut v = local.y;
    if flip.contains(Flip::V) {
        v = size.y - 1 - v;
    }
    if flip.contains(Flip::H) {
        u = size.x - 1 - u;
    }
    if flip.contains(Flip::D) {
        std::mem::swap(&mut u, &mut v);
    }
    atlas.tile_pixel(TileId(tile), UVec2::new(u, v)).unwrap()
}

/// Checks the `size` quad of a picture at `top_left` (picture pixels) shows tile `tile` drawn
/// with `flip`.
pub fn assert_tile(
    picture: &Picture,
    top_left: IVec2,
    size: UVec2,
    atlas: &Atlas,
    tile: u32,
    flip: Flip,
    what: &str,
) {
    for y in 0..size.y {
        for x in 0..size.x {
            let at = top_left + IVec2::new(x as i32, y as i32);
            let expected = tile_pixel(atlas, tile, flip, UVec2::new(x, y), size);
            let actual = picture.pixel(at.x as u32, at.y as u32);
            assert_eq!(actual, Some(expected), "{what}: local ({x}, {y}) at {at}");
        }
    }
}

/// A picture of a map at scale 1.
pub fn picture(map: &Map) -> Option<Picture> {
    let device = device()?;
    Some(
        Render::of(std::sync::Arc::new(map.clone()))
            .run(device)
            .unwrap(),
    )
}

/// The picture pixel showing world point `world` in a picture of `map` at scale 1.
pub fn pixel_of(map: &Map, world: IVec2) -> IVec2 {
    world - map.bounding_rect().min()
}

/// Tiled's tile layers composited on the CPU at scale 1, cell by cell in Tiled's order
/// (`MapGeometry::for_each_tile`), each image placed by Tiled's `CellRenderer` rules: anchored
/// at its bottom-left, moved by the tile offset, mirrored and transposed by its flips, its
/// layer's opacity applied to each tile. Layers must be untinted, and hexagonal cells unturned.
pub fn cpu_reference(map: &Map) -> Picture {
    let geometry = map.geometry();
    let bounds = map.bounding_rect();
    let width = bounds.w as u32;
    let height = bounds.h as u32;
    let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];
    let mut images: HashMap<String, DecodedImage> = HashMap::new();
    let exposed = DRect::from_min_size(bounds.min().as_dvec2(), bounds.size().as_dvec2())
        .expanded(DVec2::splat(512.0));
    map.for_each_layer(|layer, _| {
        let LayerKind::Tile(tiles) = &layer.kind else {
            return;
        };
        let effective = map.effective_layer(layer.id).unwrap();
        if !effective.visible {
            return;
        }
        assert!(
            effective.tint.is_none(),
            "the reference draws untinted layers"
        );
        geometry.for_each_tile(exposed, |pos, anchor| {
            let cell = tiles.cell(pos - effective.offset.as_ivec2());
            let Some(slot) = cell.slot() else { return };
            let tileset = &map.tilesets[slot.index()].tileset;
            let Some(tile_image) = tileset.tile_image(cell.tile()) else {
                return;
            };
            let flip = cell.flip();
            assert!(
                map.orientation != bt_core::Orientation::Hexagonal
                    || !flip.intersects(Flip::D | Flip::HEX120),
                "the reference does not turn hexagonal cells"
            );
            let key = match &tile_image.image.source {
                ImageSource::File(file) => file.path.display().to_string(),
                ImageSource::Embedded(embedded) => {
                    format!("embedded {:p}", embedded.bytes.as_ptr())
                }
            };
            let image = images.entry(key).or_insert_with(|| {
                let transparent = tile_image.image.transparent.map(|c| ColorKey {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                });
                match &tile_image.image.source {
                    ImageSource::File(file) => decode_file(&file.path, transparent).unwrap(),
                    ImageSource::Embedded(embedded) => {
                        decode_bytes(&embedded.bytes, &embedded.format, transparent).unwrap()
                    }
                }
            });
            let rect = tile_image.rect;
            let swap = flip.contains(Flip::D);
            let quad = if swap {
                IVec2::new(rect.h, rect.w)
            } else {
                IVec2::new(rect.w, rect.h)
            };
            let anchor = if map.orientation == bt_core::Orientation::Isometric {
                DVec2::new(anchor.x.floor(), (anchor.y + 0.5).floor())
            } else {
                anchor
            };
            let left =
                anchor.x as i32 + tileset.tile_offset.x + effective.offset.x as i32 - bounds.x;
            let top = anchor.y as i32 - quad.y + tileset.tile_offset.y + effective.offset.y as i32
                - bounds.y;
            for j in 0..quad.y {
                for i in 0..quad.x {
                    let x = left + i;
                    let y = top + j;
                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                        continue;
                    }
                    let mut u = i;
                    let mut v = j;
                    if flip.contains(Flip::V) {
                        v = quad.y - 1 - v;
                    }
                    if flip.contains(Flip::H) {
                        u = quad.x - 1 - u;
                    }
                    if swap {
                        std::mem::swap(&mut u, &mut v);
                    }
                    let Some(texel) = image.pixel((rect.x + u) as u32, (rect.y + v) as u32) else {
                        continue;
                    };
                    let alpha = f32::from(texel[3]) / 255.0 * effective.opacity as f32;
                    let target = &mut canvas[(y as u32 * width + x as u32) as usize];
                    for c in 0..3 {
                        let source = f32::from(texel[c]) / 255.0 * alpha;
                        target[c] = source + target[c] * (1.0 - alpha);
                    }
                    target[3] = alpha + target[3] * (1.0 - alpha);
                }
            }
        });
    });
    let rgba = canvas
        .iter()
        .flat_map(|p| {
            if p[3] <= 0.0 {
                [0; 4]
            } else {
                let un = |c: f32| ((c / p[3]).clamp(0.0, 1.0) * 255.0).round() as u8;
                [un(p[0]), un(p[1]), un(p[2]), (p[3] * 255.0).round() as u8]
            }
        })
        .collect();
    Picture {
        width,
        height,
        rgba,
    }
}

/// The share of pixels that differ by more than `tolerance` in a premultiplied channel (what
/// shows; a nearly transparent pixel's colour is not kept by 8-bit premultiplied targets).
pub fn differing_share(actual: &Picture, expected: &Picture, tolerance: u8) -> f64 {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    let premultiplied = |p: &[u8]| -> [i32; 4] {
        let a = i32::from(p[3]);
        [
            i32::from(p[0]) * a / 255,
            i32::from(p[1]) * a / 255,
            i32::from(p[2]) * a / 255,
            a,
        ]
    };
    let differing = actual
        .rgba
        .chunks_exact(4)
        .zip(expected.rgba.chunks_exact(4))
        .filter(|(a, e)| {
            let a = premultiplied(a);
            let e = premultiplied(e);
            (0..4).any(|c| a[c].abs_diff(e[c]) > u32::from(tolerance))
        })
        .count();
    differing as f64 / (expected.width * expected.height).max(1) as f64
}

/// Writes a picture into `BT_GOLDEN_DIFF_DIR` when it is set, to look at; the folder is made
/// when it is not there yet (CI names one inside `target/`).
pub fn keep(picture: &Picture, name: &str) {
    if let Some(dir) = std::env::var_os("BT_GOLDEN_DIFF_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::save_buffer(
            dir.join(format!("{name}.png")),
            &picture.rgba,
            picture.width,
            picture.height,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    }
}

/// The tile count of an atlas, to pick tiles from.
pub fn tile_count(atlas: &Atlas) -> u32 {
    match &atlas.tileset.image {
        TilesetImage::Atlas { tile_count, .. } => *tile_count,
        TilesetImage::Collection => 0,
    }
}
