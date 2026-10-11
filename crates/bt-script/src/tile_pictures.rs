//! Tiles' pixels, for what is measured on them (opaque edges, blank tiles) and the catalog's
//! sheets: each image decoded once, with its colour key, and a tile's part of it found.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{ImageSource, TileId, Tileset};
use bt_image::{ColorKey, DecodedImage};
use bt_utils::IRect;
use bt_utils::hash::fnv1a_64;
use glam::IVec2;

use crate::error::Error;

/// Which decoded image: a file, or bytes embedded in a Tiled file, with its colour key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    source: Source,
    key: Option<ColorKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Source {
    File(PathBuf),
    Embedded { hash: u64, length: usize },
}

/// A tile's pixels: its part of a decoded image.
#[derive(Clone, Debug)]
pub(crate) struct TilePixels {
    /// The whole image.
    pub(crate) image: Arc<DecodedImage>,
    /// The tile's part of it, inside it.
    pub(crate) rect: IRect,
}

impl TilePixels {
    /// The pixel at `x`, `y` of the tile, if inside.
    pub(crate) fn pixel(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.rect.w || y >= self.rect.h {
            return None;
        }
        let x = u32::try_from(self.rect.x + x).ok()?;
        let y = u32::try_from(self.rect.y + y).ok()?;
        self.image.pixel(x, y)
    }
}

/// Decoded images, by what they are.
#[derive(Debug, Default)]
pub(crate) struct TilePictures {
    images: HashMap<ImageKey, Result<Arc<DecodedImage>, bt_image::Error>>,
}

impl TilePictures {
    /// No image decoded yet.
    pub(crate) fn new() -> TilePictures {
        TilePictures::default()
    }

    /// The pixels of tile `id` of `tileset` (called `tileset_name` in errors), unflipped; `None`
    /// for a tile without an image, an error for an image that can't be read.
    pub(crate) fn tile(
        &mut self,
        tileset: &Tileset,
        tileset_name: &str,
        id: TileId,
    ) -> Result<Option<TilePixels>, Error> {
        let Some(image) = tileset.tile_image(id) else {
            return Ok(None);
        };
        let key = ImageKey {
            source: match &image.image.source {
                ImageSource::File(file) => Source::File(file.path.clone()),
                ImageSource::Embedded(embedded) => Source::Embedded {
                    hash: fnv1a_64(&embedded.bytes),
                    length: embedded.bytes.len(),
                },
            },
            key: image.image.transparent.map(|c| ColorKey {
                r: c.r,
                g: c.g,
                b: c.b,
            }),
        };
        let decoded = self
            .images
            .entry(key.clone())
            .or_insert_with(|| {
                match &image.image.source {
                    ImageSource::File(file) => bt_image::decode_file(&file.path, key.key),
                    ImageSource::Embedded(embedded) => {
                        bt_image::decode_bytes(&embedded.bytes, &embedded.format, key.key)
                    }
                }
                .map(Arc::new)
            })
            .clone()
            .map_err(|error| Error::Image {
                tile: format!("tile {id} of “{tileset_name}”"),
                error,
            })?;
        let whole = IRect::from_min_size(
            IVec2::ZERO,
            IVec2::new(
                i32::try_from(decoded.width).unwrap_or(i32::MAX),
                i32::try_from(decoded.height).unwrap_or(i32::MAX),
            ),
        );
        let rect = if image.rect.is_empty() {
            whole
        } else {
            image.rect.intersection(whole)
        };
        Ok(Some(TilePixels {
            image: decoded,
            rect,
        }))
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::Atlas;

    use super::*;

    #[test]
    fn a_tile_is_its_part_of_its_image_decoded_once() {
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let mut pictures = TilePictures::new();
        let tile = pictures
            .tile(&atlas.tileset, "ground", TileId(5))
            .unwrap()
            .unwrap();
        assert_eq!(tile.rect.size(), IVec2::new(16, 16));
        assert_eq!(
            tile.pixel(8, 8),
            atlas.tile_pixel(TileId(5), glam::UVec2::new(8, 8))
        );
        assert_eq!(tile.pixel(16, 0), None);
        let again = pictures
            .tile(&atlas.tileset, "ground", TileId(6))
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&tile.image, &again.image));
    }
}
