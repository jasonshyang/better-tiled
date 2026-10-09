//! Maps as files state them.

use super::layer::RawLayer;
use super::property::RawProperty;
use super::scalar::Scalar;
use super::tileset::RawTileset;

/// A `<map>` or a JSON map.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawMap {
    /// The format version.
    pub(crate) version: Option<Scalar>,
    /// The Tiled version that wrote the file.
    pub(crate) tiled_version: Option<Scalar>,
    /// The class.
    pub(crate) class: Option<Scalar>,
    /// The orientation.
    pub(crate) orientation: Option<Scalar>,
    /// The render order.
    pub(crate) render_order: Option<Scalar>,
    /// The compression level of layer data.
    pub(crate) compression_level: Option<Scalar>,
    /// The width in tiles.
    pub(crate) width: Option<Scalar>,
    /// The height in tiles.
    pub(crate) height: Option<Scalar>,
    /// The grid cell width.
    pub(crate) tile_width: Option<Scalar>,
    /// The grid cell height.
    pub(crate) tile_height: Option<Scalar>,
    /// Whether the map is infinite.
    pub(crate) infinite: Option<Scalar>,
    /// The hexagon side length.
    pub(crate) hex_side_length: Option<Scalar>,
    /// The stagger axis.
    pub(crate) stagger_axis: Option<Scalar>,
    /// The stagger index.
    pub(crate) stagger_index: Option<Scalar>,
    /// The horizontal shear of oblique maps.
    pub(crate) skew_x: Option<Scalar>,
    /// The vertical shear of oblique maps.
    pub(crate) skew_y: Option<Scalar>,
    /// The parallax origin's x.
    pub(crate) parallax_origin_x: Option<Scalar>,
    /// The parallax origin's y.
    pub(crate) parallax_origin_y: Option<Scalar>,
    /// The background colour.
    pub(crate) background_color: Option<Scalar>,
    /// The next layer id.
    pub(crate) next_layer_id: Option<Scalar>,
    /// The next object id.
    pub(crate) next_object_id: Option<Scalar>,
    /// `<editorsettings><chunksize>`.
    pub(crate) chunk_size: Option<RawSize>,
    /// `<editorsettings><export>`.
    pub(crate) export: Option<RawExport>,
    /// Custom properties.
    pub(crate) properties: Vec<RawProperty>,
    /// The tilesets, in file order.
    pub(crate) tilesets: Vec<RawTileset>,
    /// The layers, bottom first.
    pub(crate) layers: Vec<RawLayer>,
}

/// A width and a height.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawSize {
    /// The width.
    pub(crate) width: Option<Scalar>,
    /// The height.
    pub(crate) height: Option<Scalar>,
}

/// The last export of a map or tileset (`<editorsettings><export target format>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawExport {
    /// The file exported to, relative to the containing file.
    pub(crate) target: Option<Scalar>,
    /// The export format's short name.
    pub(crate) format: Option<Scalar>,
}
