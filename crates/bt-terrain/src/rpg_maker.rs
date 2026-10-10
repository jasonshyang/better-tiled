//! RPG Maker A2 autotiles made into a terrain set Tiled can paint with
//! (`docs/design/07-terrain.md`, §12). An A2 floor is 2×3 tiles drawn in quarters that RPG Maker
//! composes for each neighbourhood as it draws a map; Tiled draws tiles as they are. So
//! [`expand`] composes each autotile once into the 47 tiles of the Blob 7×7 layout and writes
//! the result beside the sheet, and [`tileset`] makes it a tileset with one terrain per
//! autotile, drawn over nothing. [`autotiles`], [`guess_tile`] and [`sheet_warning`] read a
//! sheet's shape and name before that.

mod compose;
mod sheet;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bt_core::{
    Color, FileRef, ImageRef, TileId, Tileset, TilesetImage, WangColor, WangSet, WangSetType,
};
use bt_image::DecodedImage;
use glam::UVec2;

use crate::color::TerrainColor;
use crate::layouts::{BLOB_7X7, block_tile};

pub use sheet::{SheetWarning, autotiles, guess_tile, sheet_warning};

/// Why a sheet could not be expanded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RpgMakerError {
    /// Tiles are composed from quarters, so their size is even, and more than 0.
    #[error("autotiles are made of tiles with an even size in pixels, not {0}")]
    TileSize(u32),
    /// The sheet does not hold one whole autotile.
    #[error("{width}×{height} pixels hold no whole autotile of 2×3 {tile}-pixel tiles")]
    NoAutotile {
        /// The sheet's width.
        width: u32,
        /// The sheet's height.
        height: u32,
        /// The tile size.
        tile: u32,
    },
    /// Every autotile of the sheet is see-through.
    #[error("every autotile of the sheet is empty")]
    NothingDrawn,
    /// More autotiles have something drawn than a terrain set holds terrains.
    #[error("the sheet has {0} autotiles, more than the {max} terrains a set holds", max = WangSet::MAX_COLORS)]
    TooManyAutotiles(usize),
    /// The sheet could not be read, or the composed image written.
    #[error(transparent)]
    Image(#[from] bt_image::Error),
    /// Every name the composed image could take beside the sheet is taken.
    #[error("every name for the composed image beside {} is taken", sheet.display())]
    NoFreeName {
        /// The sheet.
        sheet: PathBuf,
    },
}

/// A sheet's autotiles composed into blob blocks, written beside the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    path: PathBuf,
    image: DecodedImage,
    tile: u32,
    blocks: Vec<UVec2>,
}

impl Expansion {
    /// Where the composed image was written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The composed image: each autotile's 7×7 block where the autotile is in the sheet.
    pub fn image(&self) -> &DecodedImage {
        &self.image
    }

    /// The tile size in pixels.
    pub fn tile(&self) -> u32 {
        self.tile
    }

    /// The top-left tile (column, row) of each block whose autotile has something drawn, in
    /// reading order: the autotiles that become terrains.
    pub fn blocks(&self) -> &[UVec2] {
        &self.blocks
    }
}

/// How many names beside the sheet are tried for the composed image.
const NAMES_TRIED: u32 = 1000;

/// Composes each A2 autotile of the sheet at `sheet` (tiles `tile` pixels) into the 47 tiles of
/// the Blob 7×7 layout, and writes the result as a new PNG beside the sheet: `<stem> blob.png`,
/// or `<stem> blob 2.png` and on when that is taken. A file already there is never written over.
pub fn expand(sheet: &Path, tile: u32) -> Result<Expansion, RpgMakerError> {
    if tile == 0 || !tile.is_multiple_of(2) {
        return Err(RpgMakerError::TileSize(tile));
    }
    let pixels = bt_image::decode_file(sheet, None)?;
    if autotiles(pixels.width, pixels.height, tile).min_element() == 0 {
        return Err(RpgMakerError::NoAutotile {
            width: pixels.width,
            height: pixels.height,
            tile,
        });
    }
    let composed = compose::compose(&pixels, tile);
    if composed.blocks.is_empty() {
        return Err(RpgMakerError::NothingDrawn);
    }
    if composed.blocks.len() > WangSet::MAX_COLORS {
        return Err(RpgMakerError::TooManyAutotiles(composed.blocks.len()));
    }
    let path = write_beside(sheet, &composed.image)?;
    Ok(Expansion {
        path,
        image: composed.image,
        tile,
        blocks: composed.blocks,
    })
}

/// Writes `image` beside `sheet` under the first free name.
fn write_beside(sheet: &Path, image: &DecodedImage) -> Result<PathBuf, RpgMakerError> {
    let stem = sheet.file_stem().map_or_else(
        || "autotiles".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let folder = sheet.parent().unwrap_or(Path::new("."));
    for n in 1..=NAMES_TRIED {
        let name = if n == 1 {
            format!("{stem} blob.png")
        } else {
            format!("{stem} blob {n}.png")
        };
        let path = folder.join(name);
        match bt_image::write_png(&path, image) {
            Ok(()) => return Ok(path),
            Err(bt_image::Error::Exists { .. }) => continue,
            Err(other) => return Err(other.into()),
        }
    }
    Err(RpgMakerError::NoFreeName {
        sheet: sheet.to_path_buf(),
    })
}

/// A tileset named `name` of an expansion's image: one mixed terrain set, also named `name`,
/// with one terrain per autotile that has something drawn ("Autotile 1" and on, in the colour of
/// its full tile), drawn over nothing, its blob labelled as the Blob 7×7 layout labels it.
pub fn tileset(expansion: &Expansion, name: &str) -> Tileset {
    let tile = expansion.tile;
    let image = &expansion.image;
    let columns = image.width / tile;
    let rows = image.height / tile;
    let full = BLOB_7X7
        .slots()
        .iter()
        .position(|slot| slot.is_some_and(|s| s.terrain == 0xFF));
    let mut set = WangSet {
        name: name.to_owned(),
        class: String::new(),
        kind: WangSetType::Mixed,
        tile: None,
        colors: Vec::new(),
        tiles: BTreeMap::new(),
        properties: Default::default(),
    };
    for (index, &block) in expansion.blocks.iter().enumerate() {
        let Ok(color) = u8::try_from(index + 1) else {
            break;
        };
        let full_tile = full.and_then(|slot| block_tile(block, columns, BLOB_7X7.size().x, slot));
        for (tile_id, label) in
            BLOB_7X7.labels(block, columns, TerrainColor(color), TerrainColor::NONE)
        {
            if let Some(label) = label {
                set.tiles.insert(tile_id, label);
            }
        }
        set.tile = set.tile.or(full_tile);
        set.colors.push(WangColor {
            name: format!("Autotile {}", index + 1),
            class: String::new(),
            color: full_tile.map_or(Color::GRAY, |id| tile_color(image, columns, tile, id)),
            tile: full_tile,
            probability: 1.0,
            properties: Default::default(),
        });
    }
    let file_name = expansion
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Tileset {
        name: name.to_owned(),
        tile_width: tile,
        tile_height: tile,
        image: TilesetImage::Atlas {
            image: ImageRef::file(
                FileRef::new(file_name, expansion.path.clone()),
                UVec2::new(image.width, image.height),
            ),
            columns,
            tile_count: columns * rows,
        },
        wang_sets: vec![set],
        next_tile_id: columns * rows,
        ..Tileset::default()
    }
}

/// The mean colour of the opaque pixels of tile `id` of an image `columns` tiles wide; grey
/// when it has none.
fn tile_color(image: &DecodedImage, columns: u32, tile: u32, id: TileId) -> Color {
    let origin = UVec2::new(id.0 % columns, id.0 / columns) * tile;
    let mut sum = [0_u64; 3];
    let mut count = 0_u64;
    for y in 0..tile {
        for x in 0..tile {
            if let Some([r, g, b, a]) = image.pixel(origin.x + x, origin.y + y)
                && a >= 128
            {
                for (total, channel) in sum.iter_mut().zip([r, g, b]) {
                    *total += u64::from(channel);
                }
                count += 1;
            }
        }
    }
    if count == 0 {
        return Color::GRAY;
    }
    let [r, g, b] = sum.map(|total| u8::try_from(total / count).unwrap_or(u8::MAX));
    Color::rgb(r, g, b)
}
