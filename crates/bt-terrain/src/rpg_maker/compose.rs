//! Composing RPG Maker A2 autotiles into the tiles of a blob set, quarter by quarter, as RPG
//! Maker composes them for each neighbourhood when it draws a map.
//!
//! An A2 autotile is 2×3 tiles: the top-left one shows it in RPG Maker's palette, the top-right
//! one holds the four inner corners, and the 2×2 tiles below are the autotile as a small island:
//! outer corners at its corners, edges along its edges, the inside in its middle.

use bt_image::DecodedImage;
use glam::UVec2;

use crate::layouts::{BLOB_7X7, Slot};

/// The autotiles of a sheet composed into Blob 7×7 blocks.
pub(crate) struct Composed {
    /// The blocks, where the autotiles are in the sheet; see-through where an autotile is empty
    /// and in the blob's plain tiles, which are nothing over nothing.
    pub(crate) image: DecodedImage,
    /// The top-left tile (column, row) of each block whose autotile has something drawn, in
    /// reading order.
    pub(crate) blocks: Vec<UVec2>,
}

/// The lanes that decide each quarter of a tile (top-left, top-right, bottom-left,
/// bottom-right): the edge above or below it, the edge beside it, and the corner between them.
const QUARTER_LANES: [[usize; 3]; 4] = [[0, 6, 7], [0, 2, 1], [4, 6, 5], [4, 2, 3]];

/// Where a tile's quarter comes from in its autotile, in quarters (column, row) of the 2×3
/// tiles: for quarter `quarter` (0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right) whose
/// neighbour above or below is the same terrain (`vertical`), whose neighbour beside it is
/// (`horizontal`), and whose neighbour across its corner is (`corner`).
fn source(quarter: usize, vertical: bool, horizontal: bool, corner: bool) -> UVec2 {
    let right = quarter & 1 == 1;
    let bottom = quarter & 2 == 2;
    if vertical && horizontal && !corner {
        // An inner corner, from the top-right tile.
        return UVec2::new(2 + u32::from(right), u32::from(bottom));
    }
    // From the 2×2 island below, keeping to the same side of a tile: a side the neighbour shares
    // comes from the island's middle, the other from its edge.
    let x = match (horizontal, right) {
        (false, false) => 0,
        (false, true) => 3,
        (true, false) => 2,
        (true, true) => 1,
    };
    let y = match (vertical, bottom) {
        (false, false) => 2,
        (false, true) => 5,
        (true, false) => 4,
        (true, true) => 3,
    };
    UVec2::new(x, y)
}

/// Composes every autotile of `sheet` (tiles `tile` pixels, an even number) into the Blob 7×7
/// layout: block `(column, row)` of the result is autotile `(column, row)` of the sheet.
pub(crate) fn compose(sheet: &DecodedImage, tile: u32) -> Composed {
    let autotiles = super::autotiles(sheet.width, sheet.height, tile);
    let layout = BLOB_7X7.size();
    let size = autotiles * layout * tile;
    let mut image = DecodedImage {
        width: size.x,
        height: size.y,
        rgba: vec![0; (size.x as usize) * (size.y as usize) * 4],
    };
    let slots = BLOB_7X7.slots();
    let mut blocks = Vec::new();
    for row in 0..autotiles.y {
        for column in 0..autotiles.x {
            let autotile = UVec2::new(column * 2, row * 3) * tile;
            if is_empty(sheet, autotile, tile) {
                continue;
            }
            let block = UVec2::new(column, row) * layout;
            for (index, slot) in (0_u32..).zip(&slots) {
                // Plain tiles are nothing over nothing.
                let Some(slot) = slot.filter(|s| s.terrain != 0) else {
                    continue;
                };
                let cell = block + UVec2::new(index % layout.x, index / layout.x);
                compose_tile(sheet, autotile, tile, slot, &mut image, cell * tile);
            }
            blocks.push(block);
        }
    }
    Composed { image, blocks }
}

/// Whether the autotile whose top-left pixel is `autotile` has nothing drawn.
fn is_empty(sheet: &DecodedImage, autotile: UVec2, tile: u32) -> bool {
    (0..3 * tile).all(|y| {
        (0..2 * tile).all(|x| {
            sheet
                .pixel(autotile.x + x, autotile.y + y)
                .is_none_or(|[_, _, _, a]| a == 0)
        })
    })
}

/// Draws the blob tile whose terrain lanes are `slot`'s, composed from the autotile whose
/// top-left pixel is `autotile`, into `out` with its top-left pixel at `at`.
fn compose_tile(
    sheet: &DecodedImage,
    autotile: UVec2,
    tile: u32,
    slot: Slot,
    out: &mut DecodedImage,
    at: UVec2,
) {
    let half = tile / 2;
    for (quarter, &[vertical, horizontal, corner]) in QUARTER_LANES.iter().enumerate() {
        let from = source(
            quarter,
            slot.is_terrain(vertical),
            slot.is_terrain(horizontal),
            slot.is_terrain(corner),
        );
        let to = UVec2::new(quarter as u32 & 1, quarter as u32 >> 1) * half;
        copy_square(sheet, autotile + from * half, out, at + to, half);
    }
}

/// Copies the `size`×`size` pixels at `from` of `source` to `to` of `out`.
fn copy_square(source: &DecodedImage, from: UVec2, out: &mut DecodedImage, to: UVec2, size: u32) {
    for y in 0..size {
        for x in 0..size {
            let Some(pixel) = source.pixel(from.x + x, from.y + y) else {
                continue;
            };
            let (px, py) = (to.x + x, to.y + y);
            if px >= out.width || py >= out.height {
                continue;
            }
            let index = (py as usize * out.width as usize + px as usize) * 4;
            if let Some(target) = out.rgba.get_mut(index..index + 4) {
                target.copy_from_slice(&pixel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One autotile of 4-pixel tiles whose 24 quarters (2×2 pixels) each have a colour of their
    /// own, telling where they are: red is the quarter's column, green its row.
    fn quarters() -> DecodedImage {
        let mut rgba = Vec::new();
        for y in 0..12_u32 {
            for x in 0..8_u32 {
                rgba.extend([(x / 2) as u8 * 40 + 10, (y / 2) as u8 * 40 + 10, 200, 255]);
            }
        }
        DecodedImage {
            width: 8,
            height: 12,
            rgba,
        }
    }

    /// The quarter of the autotile a pixel's colour comes from.
    fn from(pixel: [u8; 4]) -> [u32; 2] {
        [
            u32::from((pixel[0] - 10) / 40),
            u32::from((pixel[1] - 10) / 40),
        ]
    }

    /// The quarters the blob tile with terrain lanes `terrain` is made of.
    fn tile_of(out: &DecodedImage, terrain: u8) -> [[u32; 2]; 4] {
        let index = BLOB_7X7
            .slots()
            .iter()
            .position(|s| s.is_some_and(|s| s.terrain == terrain))
            .unwrap() as u32;
        let (x, y) = ((index % 7) * 4, (index / 7) * 4);
        [
            from(out.pixel(x, y).unwrap()),
            from(out.pixel(x + 2, y).unwrap()),
            from(out.pixel(x, y + 2).unwrap()),
            from(out.pixel(x + 2, y + 2).unwrap()),
        ]
    }

    #[test]
    fn tiles_are_composed_from_the_quarters_their_neighbours_call_for() {
        let composed = compose(&quarters(), 4);
        let out = &composed.image;
        assert_eq!((out.width, out.height), (28, 28));
        assert_eq!(composed.blocks, [UVec2::ZERO]);
        // Inside: the island's middle.
        assert_eq!(tile_of(out, 0xFF), [[2, 4], [1, 4], [2, 3], [1, 3]]);
        // All four edges but no corner: the inner corners.
        assert_eq!(tile_of(out, 0b0101_0101), [[2, 0], [3, 0], [2, 1], [3, 1]]);
        // Only the right edge: the island's top and bottom edges on the right, outer corners on
        // the left.
        assert_eq!(tile_of(out, 0b0000_0100), [[0, 2], [1, 2], [0, 5], [1, 5]]);
        // Right, bottom-right and bottom: the island's top-left tile.
        assert_eq!(tile_of(out, 0b0001_1100), [[0, 2], [1, 2], [0, 3], [1, 3]]);
        // All four edges and the top-right corner: inner corners but at the top right.
        assert_eq!(tile_of(out, 0b0101_0111), [[2, 0], [1, 4], [2, 1], [3, 1]]);
        // The plain tiles of the layout stay see-through.
        assert_eq!(out.pixel(0, 0), Some([0, 0, 0, 0]));
    }

    #[test]
    fn empty_autotiles_make_no_block() {
        let mut sheet = DecodedImage {
            width: 16,
            height: 12,
            rgba: vec![0; 16 * 12 * 4],
        };
        let one = quarters();
        copy_square(&one, UVec2::ZERO, &mut sheet, UVec2::new(8, 0), 8);
        copy_square(&one, UVec2::new(0, 4), &mut sheet, UVec2::new(8, 4), 8);
        let composed = compose(&sheet, 4);
        assert_eq!((composed.image.width, composed.image.height), (56, 28));
        assert_eq!(
            composed.blocks,
            [UVec2::new(7, 0)],
            "only the second autotile"
        );
    }
}
