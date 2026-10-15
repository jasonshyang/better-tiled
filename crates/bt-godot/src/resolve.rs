//! Model inheritance for the Godot scene, using the file export transformations.
use bt_core::{CustomTypes, Map};
use std::path::Path;

pub(crate) fn resolved(map: &Map, types: &CustomTypes) -> Map {
    bt_tmx::ExportOptions {
        embed_tilesets: false,
        detach_templates: true,
        resolve_types_and_properties: true,
        minimize: false,
    }
    .prepare_map(map, types, Path::new("map.tmx"))
}

pub(crate) fn rule_slots(map: &Map) -> Vec<bool> {
    let builtin = bt_tmx::builtin_automap_tiles();
    map.tilesets.iter().map(|t| *t.tileset == builtin).collect()
}
