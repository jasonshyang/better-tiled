//! Pictures drawn for the terrain labelling helpers: tiles whose corners and edges show
//! terrains, lane by lane, and RPG Maker A2 autotiles. An atlas cuts such a picture with
//! [`super::AtlasBuilder::picture`].

use bt_core::WangSetType;
use glam::UVec2;
use image::{Rgba, RgbaImage};

/// A see-through picture `width`×`height` pixels.
pub fn blank_picture(width: u32, height: u32) -> RgbaImage {
    RgbaImage::new(width, height)
}

/// The lane of a set of `kind` that pixel `x`, `y` of a tile `size` pixels square lies in, in
/// Tiled's lane order: a corner set's quarters, an edge set's triangles between the diagonals,
/// a mixed set's 3×3 grid, whose middle is 8.
pub fn lane_of_pixel(kind: WangSetType, x: u32, y: u32, size: u32) -> usize {
    let u = (f64::from(x) + 0.5) / f64::from(size.max(1));
    let v = (f64::from(y) + 0.5) / f64::from(size.max(1));
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
        WangSetType::Mixed => {
            let third = |t: f64| (t * 3.0).floor().clamp(0.0, 2.0) as u8;
            match (third(u), third(v)) {
                (0, 0) => 7,
                (1, 0) => 0,
                (2, 0) => 1,
                (2, 1) => 2,
                (2, 2) => 3,
                (1, 2) => 4,
                (0, 2) => 5,
                (0, 1) => 6,
                _ => 8,
            }
        }
    }
}

/// Paints the tile whose top-left pixel is `at`, `size` pixels square: each pixel in the colour
/// of its lane ([`lane_of_pixel`]), `colors[lane]`, and a mixed set's middle in `colors[8]`;
/// `None` leaves a pixel as it is. Pixels outside the picture are left out.
pub fn paint_lanes(
    picture: &mut RgbaImage,
    at: UVec2,
    size: u32,
    kind: WangSetType,
    colors: [Option<[u8; 4]>; 9],
) {
    for y in 0..size {
        for x in 0..size {
            let lane = lane_of_pixel(kind, x, y, size);
            if let Some(color) = colors.get(lane).copied().flatten() {
                put(picture, at.x + x, at.y + y, color);
            }
        }
    }
}

/// Paints an RPG Maker A2 autotile in `color` whose top-left tile is tile `tile` (column and
/// row) of a picture of `size`-pixel tiles, as RPG Maker lays one out in 2×3 tiles: the palette
/// tile, the inner corners beside it (the terrain but for a notch a third of a tile wide at each
/// corner), and below them a 2×2-tile island whose outer band, a third of a tile wide, is
/// see-through.
pub fn paint_a2_autotile(picture: &mut RgbaImage, tile: UVec2, size: u32, color: [u8; 4]) {
    let origin = tile * size;
    let band = size / 3;
    for y in 0..size {
        for x in 0..size {
            put(picture, origin.x + x, origin.y + y, color);
            let inner = |v: u32| (band..size - band).contains(&v);
            if inner(x) || inner(y) {
                put(picture, origin.x + size + x, origin.y + y, color);
            }
        }
    }
    let island = band..2 * size - band;
    for y in 0..2 * size {
        for x in 0..2 * size {
            if island.contains(&x) && island.contains(&y) {
                put(picture, origin.x + x, origin.y + size + y, color);
            }
        }
    }
}

/// Sets a pixel, when it is inside the picture.
fn put(picture: &mut RgbaImage, x: u32, y: u32, color: [u8; 4]) {
    if x < picture.width() && y < picture.height() {
        picture.put_pixel(x, y, Rgba(color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRASS: [u8; 4] = [60, 170, 60, 255];
    const DIRT: [u8; 4] = [150, 100, 50, 255];

    #[test]
    fn lanes_are_painted_where_tileds_tileset_view_finds_them() {
        let mut picture = blank_picture(24, 12);
        let mut colors = [None; 9];
        colors[7] = Some(GRASS);
        colors[3] = Some(DIRT);
        paint_lanes(&mut picture, UVec2::ZERO, 12, WangSetType::Corner, colors);
        assert_eq!(picture.get_pixel(1, 1).0, GRASS, "the top-left corner");
        assert_eq!(picture.get_pixel(11, 11).0, DIRT, "the bottom-right corner");
        assert_eq!(picture.get_pixel(11, 0).0, [0; 4], "the top-right is left");
        let mut middle = [None; 9];
        middle[8] = Some(GRASS);
        paint_lanes(
            &mut picture,
            UVec2::new(12, 0),
            12,
            WangSetType::Mixed,
            middle,
        );
        assert_eq!(picture.get_pixel(18, 6).0, GRASS);
        assert_eq!(picture.get_pixel(13, 1).0, [0; 4]);
        assert_eq!(lane_of_pixel(WangSetType::Edge, 6, 0, 12), 0);
        assert_eq!(lane_of_pixel(WangSetType::Edge, 11, 6, 12), 2);
    }

    #[test]
    fn an_autotile_is_its_palette_tile_inner_corners_and_island() {
        let mut picture = blank_picture(24, 36);
        paint_a2_autotile(&mut picture, UVec2::ZERO, 12, GRASS);
        assert_eq!(picture.get_pixel(0, 0).0, GRASS, "the palette tile");
        assert_eq!(picture.get_pixel(12, 0).0, [0; 4], "a notch");
        assert_eq!(picture.get_pixel(18, 0).0, GRASS, "between the notches");
        assert_eq!(picture.get_pixel(0, 12).0, [0; 4], "the island's band");
        assert_eq!(picture.get_pixel(12, 24).0, GRASS, "the island");
    }
}
