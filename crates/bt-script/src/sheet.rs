//! The catalog's pictures (`bt catalog --sheet`, `10-agents.md`, §10.4), drawn on the CPU so
//! they need no GPU: the listed terrains and sprites as numbered thumbnails, or every tile of
//! tilesets with its id under it, laid out as in the tileset, the tiles the map uses outlined.

use std::collections::HashSet;

use bt_core::{Cell, Map, TileId, TilesetSlot};
use bt_image::DecodedImage;
use glam::{IVec2, UVec2};

use crate::catalog_tiles::TilesetPart;
use crate::error::Error;
use crate::symbols::TileKey;
use crate::tile_pictures::{TilePictures, TilePixels};

/// The side of a thumbnail.
const THUMB: u32 = 96;

/// Thumbnails across a row.
const COLUMNS: u32 = 10;

/// Pixels between things.
const GAP: u32 = 6;

/// The height of a label under a picture.
const LABEL: u32 = 14;

/// The largest sheet drawn, in pixels.
const MOST_PIXELS: u64 = 64_000_000;

/// Behind everything.
const BACKGROUND: [u8; 4] = [60, 60, 66, 255];

/// The outline of the tiles the map uses.
const USED: [u8; 4] = [255, 214, 0, 255];

/// The colour of numbers.
const INK: [u8; 4] = [255, 255, 255, 255];

/// A numbered item of a list, drawn by one of its tiles.
pub(crate) struct SheetItem {
    /// Its number in the list.
    pub(crate) number: usize,
    /// The tile that shows it.
    pub(crate) tile: Cell,
}

/// RGBA pixels drawn on.
struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Result<Canvas, Error> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MOST_PIXELS {
            return Err(Error::SheetTooLarge {
                size: UVec2::new(width, height),
            });
        }
        let count = usize::try_from(pixels).unwrap_or(usize::MAX);
        let mut rgba = Vec::with_capacity(count.saturating_mul(4));
        for _ in 0..count {
            rgba.extend_from_slice(&BACKGROUND);
        }
        Ok(Canvas {
            width,
            height,
            rgba,
        })
    }

    /// `color` over the pixel at `x`, `y`, by its alpha.
    fn blend(&mut self, x: i64, y: i64, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let index = usize::try_from((y * i64::from(self.width) + x) * 4).unwrap_or(usize::MAX);
        let Some(pixel) = self.rgba.get_mut(index..index + 3) else {
            return;
        };
        let alpha = u32::from(color[3]);
        for (dst, src) in pixel.iter_mut().zip(color) {
            let mixed = (u32::from(src) * alpha + u32::from(*dst) * (255 - alpha) + 127) / 255;
            *dst = u8::try_from(mixed).unwrap_or(u8::MAX);
        }
    }

    /// A checker behind a picture's place, so see-through pixels show.
    fn checker(&mut self, at: IVec2, size: UVec2) {
        for y in 0..size.y {
            for x in 0..size.x {
                let shade = if (x / 6 + y / 6) % 2 == 0 { 92 } else { 104 };
                self.blend(
                    i64::from(at.x) + i64::from(x),
                    i64::from(at.y) + i64::from(y),
                    [shade, shade, shade + 4, 255],
                );
            }
        }
    }

    /// A tile's pixels drawn at `at`, `scale` times their size.
    fn tile(&mut self, pixels: &TilePixels, at: IVec2, scale: f64) {
        let size = (pixels.rect.size().as_dvec2() * scale).round();
        let (w, h) = (size.x as i64, size.y as i64);
        for y in 0..h {
            for x in 0..w {
                let source_x = ((x as f64 + 0.5) / scale).floor() as i32;
                let source_y = ((y as f64 + 0.5) / scale).floor() as i32;
                if let Some(color) = pixels.pixel(source_x, source_y) {
                    self.blend(i64::from(at.x) + x, i64::from(at.y) + y, color);
                }
            }
        }
    }

    /// A rectangle's outline, two pixels wide.
    fn outline(&mut self, at: IVec2, size: UVec2, color: [u8; 4]) {
        let (x0, y0) = (i64::from(at.x), i64::from(at.y));
        let (w, h) = (i64::from(size.x), i64::from(size.y));
        for d in 0..2 {
            for x in x0..x0 + w {
                self.blend(x, y0 + d, color);
                self.blend(x, y0 + h - 1 - d, color);
            }
            for y in y0..y0 + h {
                self.blend(x0 + d, y, color);
                self.blend(x0 + w - 1 - d, y, color);
            }
        }
    }

    /// Digits at `at`, two pixels a dot.
    fn number(&mut self, at: IVec2, text: &str) {
        for (index, digit) in text.chars().enumerate() {
            let Some(rows) = glyph(digit) else {
                continue;
            };
            let left = i64::from(at.x) + i64::try_from(index).unwrap_or(0) * 8;
            for (row, bits) in (0i64..).zip(rows) {
                for column in 0..3i64 {
                    if bits >> (2 - column) & 1 == 1 {
                        for dy in 0..2 {
                            for dx in 0..2 {
                                self.blend(
                                    left + column * 2 + dx,
                                    i64::from(at.y) + row * 2 + dy,
                                    INK,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn into_image(self) -> DecodedImage {
        DecodedImage {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
        }
    }
}

/// The width of a number's digits.
fn number_width(text: &str) -> u32 {
    u32::try_from(text.chars().count()).unwrap_or(0) * 8
}

/// A digit in three by five dots, a row a number.
fn glyph(digit: char) -> Option<[u8; 5]> {
    Some(match digit {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => return None,
    })
}

/// The items as thumbnails, ten a row, each with its number under it.
pub(crate) fn draw_items(
    map: &Map,
    items: &[SheetItem],
    pictures: &mut TilePictures,
) -> Result<DecodedImage, Error> {
    if items.is_empty() {
        return Err(Error::NothingToDraw);
    }
    let count = u32::try_from(items.len()).unwrap_or(u32::MAX);
    let rows = count.div_ceil(COLUMNS);
    let width = COLUMNS.min(count) * (THUMB + GAP) + GAP;
    let height = rows * (THUMB + LABEL + GAP) + GAP;
    let mut canvas = Canvas::new(width, height)?;
    for (index, item) in (0u32..).zip(items) {
        let at = IVec2::new(
            i32::try_from(GAP + (index % COLUMNS) * (THUMB + GAP)).unwrap_or(0),
            i32::try_from(GAP + (index / COLUMNS) * (THUMB + LABEL + GAP)).unwrap_or(0),
        );
        canvas.checker(at, UVec2::splat(THUMB));
        if let Some(pixels) = tile_pixels(map, item.tile, pictures)? {
            let side = pixels.rect.w.max(pixels.rect.h).max(1);
            let scale = (f64::from(THUMB) / f64::from(side)).min(2.0);
            let size = (pixels.rect.size().as_dvec2() * scale).round().as_ivec2();
            let offset = (IVec2::splat(i32::try_from(THUMB).unwrap_or(0)) - size) / 2;
            canvas.tile(&pixels, at + offset, scale);
        }
        canvas.number(
            at + IVec2::new(2, i32::try_from(THUMB + 2).unwrap_or(0)),
            &item.number.to_string(),
        );
    }
    Ok(canvas.into_image())
}

/// A tileset's tiles placed on the sheet: rows of ids, `None` where a tile is left out.
struct Block {
    slot: TilesetSlot,
    rows: Vec<Vec<Option<TileId>>>,
    cell: UVec2,
    scale: u32,
    pitch: UVec2,
}

/// Every tile of the tilesets (or those named), laid out as in its tileset with its id under
/// it, the tiles the map uses outlined; `scale` times their size, else as large as reads well.
pub(crate) fn draw_tiles(
    map: &Map,
    tilesets: &[TilesetPart],
    used: &HashSet<TileKey>,
    scale: Option<u32>,
    pictures: &mut TilePictures,
) -> Result<DecodedImage, Error> {
    let mut blocks: Vec<Block> = Vec::new();
    for wanted in tilesets {
        let Some(tileset) = map.tileset(wanted.slot) else {
            continue;
        };
        let ids: Vec<TileId> = wanted.ids(tileset);
        if ids.is_empty() {
            continue;
        }
        let columns = if tileset.is_collection() {
            COLUMNS
        } else {
            tileset.columns().max(1)
        };
        let rows = layout_rows(&ids, columns, tileset.is_collection());
        let largest = ids
            .iter()
            .map(|id| tileset.tile_pixel_size(*id))
            .fold(UVec2::ONE, UVec2::max);
        let scale = scale.unwrap_or_else(|| (48 / largest.max_element().max(1)).clamp(1, 4));
        let cell = largest * scale;
        let widest = ids
            .iter()
            .map(|id| number_width(&id.0.to_string()))
            .max()
            .unwrap_or(8);
        let pitch = UVec2::new(cell.x.max(widest) + GAP, cell.y + LABEL + GAP);
        blocks.push(Block {
            slot: wanted.slot,
            rows,
            cell,
            scale,
            pitch,
        });
    }
    if blocks.is_empty() {
        return Err(Error::NothingToDraw);
    }
    let width = blocks
        .iter()
        .map(|b| {
            let across = b.rows.iter().map(Vec::len).max().unwrap_or(0);
            u32::try_from(across).unwrap_or(u32::MAX) * b.pitch.x + GAP
        })
        .max()
        .unwrap_or(GAP);
    let height: u32 = blocks
        .iter()
        .map(|b| u32::try_from(b.rows.len()).unwrap_or(u32::MAX) * b.pitch.y + GAP)
        .sum::<u32>()
        + GAP;
    let mut canvas = Canvas::new(width, height)?;
    let mut top = GAP;
    for block in &blocks {
        let Some(tileset) = map.tileset(block.slot) else {
            continue;
        };
        let name = tileset.name.clone();
        for (row, ids) in (0u32..).zip(&block.rows) {
            for (column, id) in (0u32..).zip(ids) {
                let Some(id) = *id else {
                    continue;
                };
                let at = IVec2::new(
                    i32::try_from(GAP + column * block.pitch.x).unwrap_or(0),
                    i32::try_from(top + row * block.pitch.y).unwrap_or(0),
                );
                canvas.checker(at, block.cell);
                if let Some(pixels) = pictures.tile(tileset, &name, id)? {
                    canvas.tile(&pixels, at, f64::from(block.scale));
                }
                if used.contains(&TileKey {
                    slot: block.slot,
                    id,
                }) {
                    canvas.outline(at, block.cell, USED);
                }
                canvas.number(
                    at + IVec2::new(1, i32::try_from(block.cell.y + 2).unwrap_or(0)),
                    &id.0.to_string(),
                );
            }
        }
        top += u32::try_from(block.rows.len()).unwrap_or(0) * block.pitch.y + GAP;
    }
    Ok(canvas.into_image())
}

/// The rows a tileset's tiles are laid out in: an atlas's rows of its image (only those holding
/// a listed tile, its columns from the first to the last listed), a collection's ten a row.
fn layout_rows(ids: &[TileId], columns: u32, collection: bool) -> Vec<Vec<Option<TileId>>> {
    if collection {
        return ids
            .chunks(usize::try_from(columns).unwrap_or(1).max(1))
            .map(|chunk| chunk.iter().copied().map(Some).collect())
            .collect();
    }
    let first_row = ids.iter().map(|id| id.0 / columns).min().unwrap_or(0);
    let last_row = ids.iter().map(|id| id.0 / columns).max().unwrap_or(0);
    let first_column = ids.iter().map(|id| id.0 % columns).min().unwrap_or(0);
    let last_column = ids.iter().map(|id| id.0 % columns).max().unwrap_or(0);
    (first_row..=last_row)
        .map(|row| {
            (first_column..=last_column)
                .map(|column| {
                    let id = TileId(row * columns + column);
                    ids.contains(&id).then_some(id)
                })
                .collect()
        })
        .collect()
}

/// A tile's pixels, or `None` for a tile without an image.
fn tile_pixels(
    map: &Map,
    tile: Cell,
    pictures: &mut TilePictures,
) -> Result<Option<TilePixels>, Error> {
    let Some(tileset) = map.tileset_of(tile) else {
        return Ok(None);
    };
    let name = tileset.name.clone();
    pictures.tile(tileset, &name, tile.tile())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_test_data::fixtures::{Atlas, MapBuilder};

    use super::*;

    #[test]
    fn tiles_are_laid_out_as_in_their_tileset_and_used_ones_outlined() {
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
            .tileset(Arc::clone(&atlas.tileset))
            .tile_layer("ground", "5 6")
            .build()
            .unwrap();
        let slot = TilesetSlot::new(0).unwrap();
        let used: HashSet<TileKey> = [TileKey {
            slot,
            id: TileId(5),
        }]
        .into_iter()
        .collect();
        let sheet = draw_tiles(
            &map,
            &[TilesetPart::whole(slot)],
            &used,
            Some(2),
            &mut TilePictures::new(),
        )
        .unwrap();
        // Four columns of 32 px tiles, 6 px apart; four rows with their labels.
        assert_eq!(sheet.width, 4 * (32 + GAP) + GAP);
        assert_eq!(sheet.height, 4 * (32 + LABEL + GAP) + GAP + GAP);
        // Tile 5 (column 1, row 1) is outlined; its middle shows the tile.
        let at = UVec2::new(GAP + (32 + GAP), GAP + (32 + LABEL + GAP));
        assert_eq!(sheet.pixel(at.x, at.y + 10), Some(USED));
        assert_eq!(
            sheet.pixel(at.x + 16, at.y + 16),
            atlas.tile_pixel(TileId(5), UVec2::new(8, 8))
        );
        let items = draw_items(
            &map,
            &[SheetItem {
                number: 1,
                tile: Cell::new(slot, TileId(3)),
            }],
            &mut TilePictures::new(),
        )
        .unwrap();
        assert_eq!(
            (items.width, items.height),
            (THUMB + 2 * GAP, THUMB + LABEL + 2 * GAP)
        );
        assert_eq!(
            layout_rows(&[TileId(5), TileId(7)], 4, false),
            [vec![Some(TileId(5)), None, Some(TileId(7))]]
        );
    }
}
