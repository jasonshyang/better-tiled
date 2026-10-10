//! Tileset pictures drawn pixel by pixel for the labelling helpers: autotile layouts in two
//! colours (or one over nothing), and atlases to cut them with.

use std::sync::Arc;

use bt_core::{Tileset, WangSetType};
use bt_image::DecodedImage;
use bt_terrain::layouts::Layout;
use bt_test_data::fixtures::Atlas;
use glam::UVec2;

/// An opaque colour.
pub fn rgb(r: u8, g: u8, b: u8) -> [u8; 4] {
    [r, g, b, 255]
}

/// A see-through picture of `columns`×`rows` tiles of `size` pixels.
pub fn blank(columns: u32, rows: u32, size: u32) -> DecodedImage {
    DecodedImage {
        width: columns * size,
        height: rows * size,
        rgba: vec![0; (columns * size * rows * size * 4) as usize],
    }
}

/// The tileset that cuts a picture of `columns`×`rows` tiles of `size` pixels, without spacing.
pub fn atlas_for(columns: u32, rows: u32, size: u32) -> Arc<Tileset> {
    Atlas::builder()
        .name("drawn")
        .tile_size(size, size)
        .grid(columns, rows)
        .build_embedded()
        .unwrap()
        .tileset
}

/// Sets the pixel at `x`, `y` of the picture.
pub fn put(picture: &mut DecodedImage, x: u32, y: u32, color: [u8; 4]) {
    let i = ((y * picture.width + x) * 4) as usize;
    picture.rgba[i..i + 4].copy_from_slice(&color);
}

/// The pixel at `x`, `y` of the picture.
pub fn get(picture: &DecodedImage, x: u32, y: u32) -> [u8; 4] {
    picture.pixel(x, y).unwrap()
}

/// Fills tile `column`, `row` (of `size` pixels) with `color` where `inside(x, y)` holds for
/// the pixel's place in the tile.
pub fn fill_tile(
    picture: &mut DecodedImage,
    column: u32,
    row: u32,
    size: u32,
    color: [u8; 4],
    inside: impl Fn(u32, u32) -> bool,
) {
    for y in 0..size {
        for x in 0..size {
            if inside(x, y) {
                put(picture, column * size + x, row * size + y, color);
            }
        }
    }
}

/// The lane of a set of `kind` whose region pixel `x`, `y` of a tile of `size` is in (8 for a
/// mixed set's middle): corner sets by quarter, edge sets by the triangles between the
/// diagonals, mixed sets by a 3×3 grid.
pub fn lane_at(kind: WangSetType, x: u32, y: u32, size: u32) -> usize {
    let u = (x as f32 + 0.5) / size as f32;
    let v = (y as f32 + 0.5) / size as f32;
    match kind {
        WangSetType::Corner => match (u < 0.5, v < 0.5) {
            (true, true) => 7,
            (false, true) => 1,
            (false, false) => 3,
            (true, false) => 5,
        },
        WangSetType::Edge => {
            let (dx, dy) = (u - 0.5, v - 0.5);
            if dy.abs() > dx.abs() {
                if dy < 0.0 { 0 } else { 4 }
            } else if dx > 0.0 {
                2
            } else {
                6
            }
        }
        WangSetType::Mixed => match ((u * 3.0) as u32, (v * 3.0) as u32) {
            (0, 0) => 7,
            (1, 0) => 0,
            (2, 0) => 1,
            (2, 1) => 2,
            (2, 2) => 3,
            (1, 2) => 4,
            (0, 2) => 5,
            (0, 1) => 6,
            _ => 8,
        },
    }
}

/// Draws `layout` with its top-left tile at `at`: in each tile, the region of each lane takes
/// `terrain` where the layout puts the terrain and `under` elsewhere (`None` leaves it
/// see-through); a mixed set's middle takes the terrain where the slot's middle is the terrain.
pub fn draw_layout(
    picture: &mut DecodedImage,
    layout: &Layout,
    at: UVec2,
    size: u32,
    terrain: [u8; 4],
    under: Option<[u8; 4]>,
) {
    let width = layout.size().x;
    for (i, slot) in layout.slots().iter().enumerate() {
        let Some(slot) = slot else { continue };
        let (column, row) = (at.x + i as u32 % width, at.y + i as u32 / width);
        for y in 0..size {
            for x in 0..size {
                let lane = lane_at(layout.kind, x, y, size);
                let on = if lane == 8 {
                    slot.middle
                } else {
                    slot.terrain & (1 << lane) != 0
                };
                if let Some(color) = if on { Some(terrain) } else { under } {
                    put(picture, column * size + x, row * size + y, color);
                }
            }
        }
    }
}
