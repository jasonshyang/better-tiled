//! Object templates as files state them.

use super::object::RawObject;
use super::tileset::RawTileset;

/// A `<template>` or a JSON template.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawTemplate {
    /// The tileset of the object's tile.
    pub(crate) tilesets: Vec<RawTileset>,
    /// The object.
    pub(crate) object: Option<RawObject>,
}
