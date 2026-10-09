//! File ▸ Export's options, applied to a copy of the value as Tiled's export does: tilesets
//! embedded, template instances detached, classes resolved into properties, output minimized.

use std::path::Path;
use std::sync::Arc;

use bt_core::{ClassUsage, CustomTypes, Layer, LayerKind, Map, ObjectOverrides, Tileset};

use crate::loader::Loader;
use crate::rebase::rebase_tileset;

/// What File ▸ Export does beyond a save. A plain save uses none of them.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// Copy external tilesets into the map.
    pub embed_tilesets: bool,
    /// Turn template instances into plain objects with their template's values.
    pub detach_templates: bool,
    /// Write every element's properties with its class's members and its tile's properties
    /// resolved in.
    pub resolve_types_and_properties: bool,
    /// Leave out line breaks and indentation.
    pub minimize: bool,
}

/// The map to write under `options`: `None` when the options change nothing but the layout.
pub(crate) fn exported_map(
    map: &Map,
    options: &ExportOptions,
    types: Option<&CustomTypes>,
    _loader: Option<&Loader>,
    path: &Path,
) -> Option<Map> {
    if !options.embed_tilesets && !options.detach_templates && !options.resolve_types_and_properties
    {
        return None;
    }
    let mut map = map.clone();
    let map_dir = path.parent().unwrap_or(Path::new(""));
    if options.embed_tilesets {
        for tileset in &mut map.tilesets {
            if let Some(file) = tileset.file.take() {
                let from_dir = file.path.parent().unwrap_or(Path::new("")).to_path_buf();
                let mut embedded = (*tileset.tileset).clone();
                rebase_tileset(&mut embedded, &from_dir, map_dir);
                embedded.file = Default::default();
                tileset.tileset = Arc::new(embedded);
            }
        }
    }
    if options.detach_templates {
        visit_layers(&mut map.layers, &mut |layer| {
            if let LayerKind::Object(objects) = &mut layer.kind {
                for object in Arc::make_mut(&mut objects.objects) {
                    // Instances already hold their template's values.
                    object.template = None;
                    object.overrides = ObjectOverrides::empty();
                }
            }
        });
    }
    if options.resolve_types_and_properties
        && let Some(types) = types
    {
        resolve_map(&mut map, types);
    }
    Some(map)
}

/// The tileset to write under `options`: `None` when they change nothing but the layout.
pub(crate) fn exported_tileset(
    tileset: &Tileset,
    options: &ExportOptions,
    types: Option<&CustomTypes>,
) -> Option<Tileset> {
    let types = types.filter(|_| options.resolve_types_and_properties)?;
    let mut tileset = tileset.clone();
    resolve_tileset(&mut tileset, types);
    Some(tileset)
}

/// Resolves classes into the properties of a map and everything in it.
fn resolve_map(map: &mut Map, types: &CustomTypes) {
    map.properties = types.resolved_properties(&map.class, ClassUsage::MAP, &map.properties);
    let tilesets = map.tilesets.clone();
    visit_layers(&mut map.layers, &mut |layer| {
        layer.properties =
            types.resolved_properties(&layer.class, ClassUsage::LAYER, &layer.properties);
        if let LayerKind::Object(objects) = &mut layer.kind {
            for object in Arc::make_mut(&mut objects.objects) {
                let tile = object.tile.and_then(|cell| {
                    let tileset = tilesets.get(cell.slot()?.index())?;
                    tileset.tileset.tile(cell.tile())
                });
                object.properties = types.object_properties(object, tile, None);
            }
        }
    });
    for tileset in &mut map.tilesets {
        resolve_tileset(Arc::make_mut(&mut tileset.tileset), types);
    }
}

/// Resolves classes into the properties of a tileset, its tiles and its terrain sets.
fn resolve_tileset(tileset: &mut Tileset, types: &CustomTypes) {
    tileset.properties =
        types.resolved_properties(&tileset.class, ClassUsage::TILESET, &tileset.properties);
    for tile in tileset.tiles.values_mut() {
        tile.properties =
            types.resolved_properties(&tile.class, ClassUsage::TILE, &tile.properties);
    }
    for set in &mut tileset.wang_sets {
        set.properties =
            types.resolved_properties(&set.class, ClassUsage::WANG_SET, &set.properties);
        for color in &mut set.colors {
            color.properties =
                types.resolved_properties(&color.class, ClassUsage::WANG_COLOR, &color.properties);
        }
    }
}

fn visit_layers(layers: &mut [Layer], visit: &mut dyn FnMut(&mut Layer)) {
    for layer in layers {
        visit(layer);
        if let Some(children) = layer.children_mut() {
            visit_layers(children, visit);
        }
    }
}
