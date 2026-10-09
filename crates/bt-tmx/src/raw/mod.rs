//! The raw tree: Tiled's files as they state things, before defaults, conversions and
//! references are applied. The XML and JSON readers fill it; `finish` turns it into the model
//! in one place; `unfinish` turns the model back into it with every value present; the
//! printers lay it out. It is the union of what either syntax can say.

mod image;
mod layer;
mod map;
mod object;
mod property;
mod scalar;
mod template;
mod tile;
mod tileset;
mod wang;

pub(crate) use image::{RawImage, RawImageData};
pub(crate) use layer::{RawChunk, RawData, RawLayer, RawLayerKind, RawPayload};
pub(crate) use map::{RawExport, RawMap, RawSize};
pub(crate) use object::{RawObject, RawPoint, RawShape, RawText};
pub(crate) use property::{RawProperty, RawValue};
pub(crate) use scalar::Scalar;
pub(crate) use template::RawTemplate;
pub(crate) use tile::{RawFrame, RawTerrain, RawTile};
pub(crate) use tileset::{RawGrid, RawTileset, RawTransformations};
pub(crate) use wang::{RawWangColor, RawWangId, RawWangSet, RawWangTile, WangColorRole};
