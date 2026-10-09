//! Terrain sets as files state them.

use super::property::RawProperty;
use super::scalar::Scalar;

/// A `<wangset>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawWangSet {
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// `corner`, `edge` or `mixed`.
    pub(crate) kind: Option<Scalar>,
    /// The tile that shows the set.
    pub(crate) tile: Option<Scalar>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// The colours, in file order.
    pub(crate) colors: Vec<RawWangColor>,
    /// The tiles with terrain.
    pub(crate) tiles: Vec<RawWangTile>,
}

/// A colour of a terrain set.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawWangColor {
    /// Which lanes the colour was for, in files of Tiled 1.1 to 1.4.
    pub(crate) role: WangColorRole,
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// The colour.
    pub(crate) color: Option<Scalar>,
    /// The tile that shows it.
    pub(crate) tile: Option<Scalar>,
    /// The probability.
    pub(crate) probability: Option<Scalar>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
}

/// Which lanes a colour is for.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum WangColorRole {
    /// Any (`<wangcolor>`, Tiled 1.5 and later).
    #[default]
    Any,
    /// Corners (`<wangcornercolor>`, Tiled 1.1 to 1.4).
    Corner,
    /// Edges (`<wangedgecolor>`, Tiled 1.1 to 1.4).
    Edge,
}

/// A `<wangtile>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawWangTile {
    /// The tile.
    pub(crate) tile_id: Option<Scalar>,
    /// Its terrain.
    pub(crate) wang_id: Option<RawWangId>,
}

/// A tile's terrain as written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RawWangId {
    /// Eight comma-separated colours, or a hexadecimal number with four bits a lane (before
    /// Tiled 1.5).
    Text(String),
    /// Eight colours in a JSON array.
    Lanes(Vec<Scalar>),
    /// A number with four bits a lane (JSON before Tiled 1.5).
    Packed(i64),
}
