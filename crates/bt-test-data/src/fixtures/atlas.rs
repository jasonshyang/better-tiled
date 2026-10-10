//! Atlas tilesets drawn pixel by pixel.
//!
//! Every tile is filled with a colour of its own and carries a marker in its top-left corner, a
//! bar wider than it is tall, so a test can tell from a few pixels which tile was drawn and how
//! it was mirrored or turned. Margins and spacing are a gutter colour, so a pixel bleeding in
//! from beside a tile shows. A test may give a picture of its own to cut instead
//! ([`AtlasBuilder::picture`]).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use bt_core::{
    EmbeddedImage, FileRef, FillMode, Frame, ImageRef, ImageSource, TileData, TileId,
    TileRenderSize, Tileset, TilesetImage,
};
use glam::{IVec2, UVec2};
use image::{ImageFormat, Rgba, RgbaImage};

use super::error::FixtureError;

/// The colour of every tile's marker.
pub const MARKER: [u8; 4] = [255, 255, 255, 255];

/// The colour of an atlas's margins and spacing.
pub const GUTTER: [u8; 4] = [255, 0, 255, 255];

/// A drawn atlas tileset and the image it is cut from.
#[derive(Clone, Debug)]
pub struct Atlas {
    /// The tileset.
    pub tileset: Arc<Tileset>,
    /// Its image.
    pub image: RgbaImage,
    marker: UVec2,
}

impl Atlas {
    /// A builder with the defaults: “atlas”, 16×16 tiles in 4 columns and 4 rows, no margin or
    /// spacing, a 4×2 marker.
    pub fn builder() -> AtlasBuilder {
        AtlasBuilder {
            name: "atlas".to_owned(),
            tile_size: UVec2::new(16, 16),
            columns: 4,
            rows: 4,
            spacing: 0,
            margin: 0,
            marker: UVec2::new(4, 2),
            tile_offset: IVec2::ZERO,
            render_size: TileRenderSize::Tile,
            fill_mode: FillMode::Stretch,
            animations: BTreeMap::new(),
            picture: None,
        }
    }

    /// The colour a tile is filled with outside its marker: opaque, distinct for the first few
    /// hundred tiles, never the marker or gutter colour.
    pub fn tile_color(id: TileId) -> [u8; 4] {
        let channel = |step: u32, start: u32| {
            u8::try_from(id.0.wrapping_mul(step).wrapping_add(start) % 200 + 20).unwrap_or(0)
        };
        [channel(67, 40), channel(131, 90), channel(29, 150), 255]
    }

    /// The colour of tile `id` at `local` (pixels from its top-left), as drawn: the marker or
    /// the tile's colour. `None` outside the tile. (An atlas cut from a picture of the test's
    /// own is drawn as the picture says, not like this.)
    pub fn tile_pixel(&self, id: TileId, local: UVec2) -> Option<[u8; 4]> {
        let size = self.tileset.tile_size();
        if local.x >= size.x || local.y >= size.y {
            return None;
        }
        if local.x < self.marker.x && local.y < self.marker.y {
            Some(MARKER)
        } else {
            Some(Atlas::tile_color(id))
        }
    }
}

/// Sets up an [`Atlas`].
#[derive(Clone, Debug)]
pub struct AtlasBuilder {
    name: String,
    tile_size: UVec2,
    columns: u32,
    rows: u32,
    spacing: u32,
    margin: u32,
    marker: UVec2,
    tile_offset: IVec2,
    render_size: TileRenderSize,
    fill_mode: FillMode,
    animations: BTreeMap<TileId, Vec<Frame>>,
    picture: Option<RgbaImage>,
}

impl AtlasBuilder {
    /// The tileset's name, also the image file's stem.
    pub fn name(mut self, name: &str) -> AtlasBuilder {
        name.clone_into(&mut self.name);
        self
    }

    /// The size of a tile in pixels.
    pub fn tile_size(mut self, width: u32, height: u32) -> AtlasBuilder {
        self.tile_size = UVec2::new(width, height);
        self
    }

    /// The tiles across and down.
    pub fn grid(mut self, columns: u32, rows: u32) -> AtlasBuilder {
        self.columns = columns;
        self.rows = rows;
        self
    }

    /// Pixels between tiles.
    pub fn spacing(mut self, spacing: u32) -> AtlasBuilder {
        self.spacing = spacing;
        self
    }

    /// Pixels around the tiles.
    pub fn margin(mut self, margin: u32) -> AtlasBuilder {
        self.margin = margin;
        self
    }

    /// The marker's size in pixels; keep it wider than tall so a transpose shows.
    pub fn marker(mut self, width: u32, height: u32) -> AtlasBuilder {
        self.marker = UVec2::new(width, height);
        self
    }

    /// The tileset's tile offset.
    pub fn tile_offset(mut self, offset: IVec2) -> AtlasBuilder {
        self.tile_offset = offset;
        self
    }

    /// The size tiles are drawn at on tile layers.
    pub fn render_size(mut self, render_size: TileRenderSize) -> AtlasBuilder {
        self.render_size = render_size;
        self
    }

    /// How a tile drawn at another size fills it.
    pub fn fill_mode(mut self, fill_mode: FillMode) -> AtlasBuilder {
        self.fill_mode = fill_mode;
        self
    }

    /// Animates `tile` with these frames.
    pub fn animation(mut self, tile: TileId, frames: Vec<Frame>) -> AtlasBuilder {
        self.animations.insert(tile, frames);
        self
    }

    /// Cuts `picture` instead of drawing the tiles: it must be the size the grid, the tile size,
    /// the margin and the spacing make.
    pub fn picture(mut self, picture: RgbaImage) -> AtlasBuilder {
        self.picture = Some(picture);
        self
    }

    /// The atlas with its image embedded in the tileset as PNG bytes.
    pub fn build_embedded(mut self) -> Result<Atlas, FixtureError> {
        let image = self.draw()?;
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .map_err(|error| FixtureError::Image {
                path: Path::new("(embedded)").to_path_buf(),
                message: error.to_string(),
            })?;
        let source = ImageSource::Embedded(EmbeddedImage {
            format: "png".to_owned(),
            bytes: Arc::from(bytes.into_inner()),
        });
        Ok(self.finished(image, source))
    }

    /// The atlas with its image written to `<dir>/<name>.png`.
    pub fn build_in(mut self, dir: &Path) -> Result<Atlas, FixtureError> {
        let image = self.draw()?;
        let file_name = format!("{}.png", self.name);
        let path = dir.join(&file_name);
        image
            .save_with_format(&path, ImageFormat::Png)
            .map_err(|error| FixtureError::Image {
                path: path.clone(),
                message: error.to_string(),
            })?;
        Ok(self.finished(image, ImageSource::File(FileRef::new(file_name, path))))
    }

    fn image_size(&self) -> UVec2 {
        let along = |count: u32, size: u32| {
            2 * self.margin + count * size + count.saturating_sub(1) * self.spacing
        };
        UVec2::new(
            along(self.columns, self.tile_size.x),
            along(self.rows, self.tile_size.y),
        )
    }

    /// The picture given, or the tiles drawn.
    fn draw(&mut self) -> Result<RgbaImage, FixtureError> {
        let size = self.image_size();
        if let Some(picture) = self.picture.take() {
            let found = UVec2::new(picture.width(), picture.height());
            if found != size {
                return Err(FixtureError::PictureSize {
                    expected: size,
                    found,
                });
            }
            return Ok(picture);
        }
        let mut image = RgbaImage::from_pixel(size.x, size.y, Rgba(GUTTER));
        for row in 0..self.rows {
            for column in 0..self.columns {
                let id = TileId(row * self.columns + column);
                let origin = UVec2::splat(self.margin)
                    + UVec2::new(column, row) * (self.tile_size + UVec2::splat(self.spacing));
                for y in 0..self.tile_size.y {
                    for x in 0..self.tile_size.x {
                        let marker = x < self.marker.x && y < self.marker.y;
                        let color = if marker {
                            MARKER
                        } else {
                            Atlas::tile_color(id)
                        };
                        image.put_pixel(origin.x + x, origin.y + y, Rgba(color));
                    }
                }
            }
        }
        Ok(image)
    }

    fn finished(self, image: RgbaImage, source: ImageSource) -> Atlas {
        let tiles = self
            .animations
            .into_iter()
            .map(|(tile, frames)| {
                (
                    tile,
                    TileData {
                        animation: frames,
                        ..TileData::default()
                    },
                )
            })
            .collect();
        let tileset = Tileset {
            name: self.name,
            tile_width: self.tile_size.x,
            tile_height: self.tile_size.y,
            spacing: self.spacing,
            margin: self.margin,
            image: TilesetImage::Atlas {
                image: ImageRef {
                    source,
                    declared_size: UVec2::new(image.width(), image.height()),
                    transparent: None,
                },
                columns: self.columns,
                tile_count: self.columns * self.rows,
            },
            tile_offset: self.tile_offset,
            tile_render_size: self.render_size,
            fill_mode: self.fill_mode,
            tiles,
            next_tile_id: self.columns * self.rows,
            ..Tileset::default()
        };
        Atlas {
            tileset: Arc::new(tileset),
            image,
            marker: self.marker,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scratch;

    #[test]
    fn tiles_are_drawn_where_the_tileset_says_they_are() {
        let atlas = Atlas::builder()
            .tile_size(8, 6)
            .grid(3, 2)
            .spacing(1)
            .margin(2)
            .build_embedded()
            .unwrap();
        assert_eq!(
            (atlas.image.width(), atlas.image.height()),
            (2 * 2 + 24 + 2, 2 * 2 + 12 + 1)
        );
        for id in 0..6 {
            let rect = atlas.tileset.tile_image(TileId(id)).unwrap().rect;
            let at = |x: i32, y: i32| {
                atlas
                    .image
                    .get_pixel(
                        u32::try_from(rect.x + x).unwrap(),
                        u32::try_from(rect.y + y).unwrap(),
                    )
                    .0
            };
            assert_eq!(at(0, 0), MARKER);
            assert_eq!(at(3, 1), MARKER);
            assert_eq!(at(4, 0), Atlas::tile_color(TileId(id)));
            assert_eq!(at(7, 5), Atlas::tile_color(TileId(id)));
            assert_eq!(
                atlas.tile_pixel(TileId(id), UVec2::new(7, 5)),
                Some(at(7, 5))
            );
        }
        assert_eq!(atlas.image.get_pixel(0, 0).0, GUTTER);
        let colors: std::collections::HashSet<[u8; 4]> =
            (0..300).map(|id| Atlas::tile_color(TileId(id))).collect();
        assert!(colors.len() >= 200 && !colors.contains(&MARKER) && !colors.contains(&GUTTER));
    }

    #[test]
    fn an_atlas_cuts_a_picture_of_the_size_its_grid_makes() {
        let picture = RgbaImage::from_pixel(32, 16, Rgba([1, 2, 3, 255]));
        let atlas = Atlas::builder()
            .grid(2, 1)
            .picture(picture.clone())
            .build_embedded()
            .unwrap();
        assert_eq!(atlas.image, picture);
        assert_eq!(atlas.tileset.tile_count(), 2);
        let wrong = Atlas::builder()
            .grid(3, 1)
            .picture(picture)
            .build_embedded();
        assert_eq!(
            wrong.unwrap_err(),
            FixtureError::PictureSize {
                expected: UVec2::new(48, 16),
                found: UVec2::new(32, 16)
            }
        );
    }

    #[test]
    fn an_atlas_in_a_folder_is_a_png_file() {
        let scratch = Scratch::new("bt-test-data-atlas").unwrap();
        let atlas = Atlas::builder()
            .name("tiles")
            .build_in(scratch.path())
            .unwrap();
        let TilesetImage::Atlas { image, .. } = &atlas.tileset.image else {
            panic!("an atlas");
        };
        let path = image.path().unwrap();
        assert_eq!(path, scratch.join("tiles.png"));
        let read = image::open(path).unwrap().to_rgba8();
        assert_eq!(read, atlas.image);
    }
}
