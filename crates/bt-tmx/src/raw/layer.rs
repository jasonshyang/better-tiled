//! Layers as files state them.

use super::image::RawImage;
use super::object::RawObject;
use super::property::RawProperty;
use super::scalar::Scalar;

/// A layer of any kind; the fields of the other kinds stay empty. A tile's collision shapes
/// are an object layer too.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawLayer {
    /// What kind of layer it is.
    pub(crate) kind: RawLayerKind,
    /// The id.
    pub(crate) id: Option<Scalar>,
    /// The name.
    pub(crate) name: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// A position in tiles that Tiled never writes (except as an image layer's offset before
    /// Tiled 0.15, and always as 0 in JSON).
    pub(crate) x: Option<Scalar>,
    /// See `x`.
    pub(crate) y: Option<Scalar>,
    /// A tile layer's width.
    pub(crate) width: Option<Scalar>,
    /// A tile layer's height.
    pub(crate) height: Option<Scalar>,
    /// Visibility.
    pub(crate) visible: Option<Scalar>,
    /// Lock.
    pub(crate) locked: Option<Scalar>,
    /// Opacity.
    pub(crate) opacity: Option<Scalar>,
    /// Tint colour.
    pub(crate) tint_color: Option<Scalar>,
    /// Horizontal offset.
    pub(crate) offset_x: Option<Scalar>,
    /// Vertical offset.
    pub(crate) offset_y: Option<Scalar>,
    /// Horizontal parallax factor.
    pub(crate) parallax_x: Option<Scalar>,
    /// Vertical parallax factor.
    pub(crate) parallax_y: Option<Scalar>,
    /// Blend mode.
    pub(crate) mode: Option<Scalar>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// The left of a JSON tile layer's data, or of an infinite one's chunks.
    pub(crate) start_x: Option<Scalar>,
    /// The top of the same.
    pub(crate) start_y: Option<Scalar>,
    /// A tile layer's cells.
    pub(crate) data: Option<RawData>,
    /// An object layer's colour.
    pub(crate) color: Option<Scalar>,
    /// An object layer's draw order.
    pub(crate) draw_order: Option<Scalar>,
    /// An object layer's objects.
    pub(crate) objects: Vec<RawObject>,
    /// An image layer's image.
    pub(crate) image: Option<RawImage>,
    /// Whether an image layer repeats horizontally.
    pub(crate) repeat_x: Option<Scalar>,
    /// Whether it repeats vertically.
    pub(crate) repeat_y: Option<Scalar>,
    /// A group's layers, bottom first.
    pub(crate) layers: Vec<RawLayer>,
}

/// The kinds of layers.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum RawLayerKind {
    /// `<layer>`, `tilelayer`.
    #[default]
    Tile,
    /// `<objectgroup>`, `objectgroup`.
    Object,
    /// `<imagelayer>`, `imagelayer`.
    Image,
    /// `<group>`, `group`.
    Group,
}

/// A tile layer's `<data>` (in JSON, the layer's `encoding`, `compression`, `data` and
/// `chunks`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawData {
    /// `csv`, `base64`, or absent for `<tile>` elements (or a JSON array).
    pub(crate) encoding: Option<Scalar>,
    /// `zlib`, `gzip` or `zstd`.
    pub(crate) compression: Option<Scalar>,
    /// The cells of a finite layer.
    pub(crate) payload: RawPayload,
    /// The chunks of an infinite layer.
    pub(crate) chunks: Vec<RawChunk>,
    /// Going out: the bounds of an infinite layer's cells in whole 16×16 chunks, which Tiled's
    /// JSON gives as the layer's `startx`, `starty`, `width` and `height`.
    pub(crate) chunk_bounds: Option<RawChunk>,
}

/// A `<chunk>` of an infinite layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawChunk {
    /// The left, in tiles.
    pub(crate) x: Option<Scalar>,
    /// The top, in tiles.
    pub(crate) y: Option<Scalar>,
    /// The width, in tiles.
    pub(crate) width: Option<Scalar>,
    /// The height, in tiles.
    pub(crate) height: Option<Scalar>,
    /// The cells.
    pub(crate) payload: RawPayload,
}

/// Cells as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RawPayload {
    /// None.
    #[default]
    Empty,
    /// CSV or base64 text.
    Text(String),
    /// One global id per cell: `<tile gid>` elements, or a JSON array.
    Gids(Vec<u32>),
}
