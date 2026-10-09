//! Tiled's JSON formats: `.tmj` maps, `.tsj` tilesets, `.tj` templates, and the JSON documents
//! of projects, worlds and stamps.

mod print;
mod qt;
mod read;
mod variant;

pub(crate) use print::{minimized_layout, tiled_layout};
pub(crate) use qt::{qt_compact_layout, qt_layout};
pub(crate) use read::{
    JsonReader, parse_document, read_map, read_template, read_tileset_file, typed_property,
};
pub(crate) use variant::{map_json, properties_json, template_json, tileset_json};
