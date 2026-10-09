//! Tilesets as files state them.

use super::image::RawImage;
use super::map::RawExport;
use super::object::RawPoint;
use super::property::RawProperty;
use super::scalar::Scalar;
use super::tile::{RawTerrain, RawTile};
use super::wang::RawWangSet;

/// A `<tileset>`: a map's or template's reference to a tileset file (`firstgid` and `source`), a
/// tileset embedded in a map (`firstgid` and contents), or the root of a tileset file
/// (`version`, `tiledversion` and contents, or a `source` it stands for).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawTileset {
    /// The first global id, in a map or template.
    pub(crate) first_gid: Option<Scalar>,
    /// The tileset file this element refers to.
    pub(crate) source: Option<Scalar>,
    /// The format version, of a tileset file.
    pub(crate) version: Option<Scalar>,
    /// The Tiled version that wrote the tileset file.
    pub(crate) tiled_version: Option<Scalar>,
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// The tile width.
    pub(crate) tile_width: Option<Scalar>,
    /// The tile height.
    pub(crate) tile_height: Option<Scalar>,
    /// Pixels between an atlas's tiles.
    pub(crate) spacing: Option<Scalar>,
    /// Pixels around an atlas's tiles.
    pub(crate) margin: Option<Scalar>,
    /// The tile count as written.
    pub(crate) tile_count: Option<Scalar>,
    /// The column count as written.
    pub(crate) columns: Option<Scalar>,
    /// The background colour.
    pub(crate) background_color: Option<Scalar>,
    /// The object alignment.
    pub(crate) object_alignment: Option<Scalar>,
    /// The tile render size.
    pub(crate) tile_render_size: Option<Scalar>,
    /// The fill mode.
    pub(crate) fill_mode: Option<Scalar>,
    /// `<editorsettings><export>`.
    pub(crate) export: Option<RawExport>,
    /// `<tileoffset>`.
    pub(crate) tile_offset: Option<RawPoint>,
    /// `<grid>`.
    pub(crate) grid: Option<RawGrid>,
    /// `<transformations>`.
    pub(crate) transformations: Option<RawTransformations>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// An atlas's image.
    pub(crate) image: Option<RawImage>,
    /// Tile entries, in file order.
    pub(crate) tiles: Vec<RawTile>,
    /// Terrain types of files before Tiled 1.5.
    pub(crate) terrains: Vec<RawTerrain>,
    /// Terrain sets.
    pub(crate) wang_sets: Vec<RawWangSet>,
    /// Going out: whether Tiled lists every tile, not only those with data (a collection, or an
    /// atlas whose tile ids leave a gap: `Tileset::anyTileOutOfOrder`).
    pub(crate) lists_every_tile: bool,
}

/// A tileset's `<grid>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawGrid {
    /// `orthogonal` or `isometric`.
    pub(crate) orientation: Option<Scalar>,
    /// The cell width.
    pub(crate) width: Option<Scalar>,
    /// The cell height.
    pub(crate) height: Option<Scalar>,
}

/// A tileset's `<transformations>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawTransformations {
    /// Mirroring left-right is allowed.
    pub(crate) hflip: Option<Scalar>,
    /// Mirroring top-bottom is allowed.
    pub(crate) vflip: Option<Scalar>,
    /// Turning is allowed.
    pub(crate) rotate: Option<Scalar>,
    /// Plain tiles are preferred.
    pub(crate) prefer_untransformed: Option<Scalar>,
}
