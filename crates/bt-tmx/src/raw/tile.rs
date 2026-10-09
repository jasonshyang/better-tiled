//! Tile entries of tilesets as files state them.

use super::image::RawImage;
use super::layer::RawLayer;
use super::property::RawProperty;
use super::scalar::Scalar;

/// A tileset's `<tile>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawTile {
    /// The tile id.
    pub(crate) id: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// The class, spelled `type` (before Tiled 1.9 and again from 1.10).
    pub(crate) class_as_type: Option<Scalar>,
    /// The probability.
    pub(crate) probability: Option<Scalar>,
    /// The left of the tile's part of its image.
    pub(crate) x: Option<Scalar>,
    /// The top of that part.
    pub(crate) y: Option<Scalar>,
    /// Its width.
    pub(crate) width: Option<Scalar>,
    /// Its height.
    pub(crate) height: Option<Scalar>,
    /// The terrain indexes of the four corners in files before Tiled 1.5: top-left, top-right,
    /// bottom-left, bottom-right; an absent corner has none.
    pub(crate) terrain: Option<Vec<Option<Scalar>>>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// A collection tile's image.
    pub(crate) image: Option<RawImage>,
    /// The collision shapes' object group.
    pub(crate) collision: Option<RawLayer>,
    /// The animation frames.
    pub(crate) animation: Option<Vec<RawFrame>>,
}

/// An animation `<frame>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawFrame {
    /// The tile shown.
    pub(crate) tile_id: Option<Scalar>,
    /// For how many milliseconds.
    pub(crate) duration: Option<Scalar>,
}

/// A `<terrain>` of files before Tiled 1.5.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawTerrain {
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The tile that shows it.
    pub(crate) tile: Option<Scalar>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
}
