//! The layers as the resolved JSON lists them: in draw order from the bottom, each group before
//! its layers, each with its groups' visibility, opacity, offset, parallax and tint applied (as
//! `bt-core`'s effective layer combines them), and what it holds: rows of tile keys over the
//! written area, objects where they are drawn, or an image.

use bt_core::{BlendMode, ClassUsage, DrawOrder, EffectiveLayer, Layer, LayerKind, TileLayer};
use bt_utils::IRect;
use glam::IVec2;

use super::document::{LayerContent, LayerEntry};
use super::error::ResolvedWarning;
use super::objects::object_entry;
use super::palette::{Palette, Unkeyed};
use super::resolving::Resolving;
use super::values::{color_text, resolved_properties};

/// Every layer of the map, bottom first, each group before its layers, with tile layers'
/// rows over `area`.
pub(crate) fn layer_entries(
    resolving: &mut Resolving,
    palette: &mut Palette,
    area: IRect,
) -> Vec<LayerEntry> {
    let map = resolving.map;
    let mut walk = Walk {
        resolving,
        palette,
        area,
        out: Vec::new(),
    };
    walk.add_layers(&map.layers, EffectiveLayer::ROOT, "");
    walk.out
}

/// The walk over the layer tree: what it reads, and the entries made so far.
struct Walk<'w, 'm> {
    resolving: &'w mut Resolving<'m>,
    palette: &'w mut Palette,
    area: IRect,
    out: Vec<LayerEntry>,
}

impl Walk<'_, '_> {
    /// Adds `layers` (bottom first) under groups whose settings combine to `above`, their names
    /// joined into `path`.
    fn add_layers(&mut self, layers: &[Layer], above: EffectiveLayer, path: &str) {
        for layer in layers {
            let here = above.combined_with(layer);
            let path = if path.is_empty() {
                layer.name.clone()
            } else {
                format!("{path}/{}", layer.name)
            };
            let content = match &layer.kind {
                LayerKind::Tile(tiles) => self.tile_content(layer, tiles),
                LayerKind::Object(objects) => {
                    let entries = objects
                        .objects
                        .iter()
                        .filter_map(|object| {
                            object_entry(self.resolving, self.palette, object, here.offset)
                        })
                        .collect();
                    LayerContent::Objects {
                        draw_order: match objects.draw_order {
                            DrawOrder::TopDown => "top-down",
                            DrawOrder::Index => "list",
                        },
                        color: objects.color.map(color_text),
                        objects: entries,
                    }
                }
                LayerKind::Image(image) => {
                    let source = image.image.as_ref();
                    LayerContent::Image {
                        image: source.and_then(|source| {
                            self.resolving
                                .image_path(source, || format!("layer “{}”", layer.name))
                        }),
                        image_size: source
                            .map(|source| source.declared_size)
                            .filter(|size| size.x > 0 && size.y > 0),
                        repeat_x: image.repeat_x,
                        repeat_y: image.repeat_y,
                    }
                }
                LayerKind::Group(_) => LayerContent::Group {},
            };
            self.out.push(LayerEntry {
                id: layer.id.0,
                name: layer.name.clone(),
                path: path.clone(),
                kind: content.kind_word(),
                visible: here.visible,
                opacity: here.opacity,
                offset: here.offset,
                parallax: here.parallax,
                tint: here.tint.map(color_text),
                blend_mode: blend_word(layer.blend_mode),
                class: layer.class.clone(),
                properties: resolved_properties(
                    self.resolving.types,
                    &layer.class,
                    ClassUsage::LAYER,
                    &layer.properties,
                ),
                content,
            });
            self.add_layers(layer.children(), here, &path);
        }
    }

    /// A tile layer's rows of keys over the written area. Cells of AutoMap's rule tiles are
    /// left empty, and so are cells of a tileset the map does not have, which are said.
    fn tile_content(&mut self, layer: &Layer, tiles: &TileLayer) -> LayerContent {
        let area = self.area;
        let mut filled = 0;
        let mut no_tileset = 0;
        let mut rows = Vec::new();
        for y in area.y..area.y + area.h {
            let mut row = Vec::new();
            for x in area.x..area.x + area.w {
                let cell = tiles.cell(IVec2::new(x, y));
                let key = if cell.is_empty() {
                    0
                } else {
                    match self.palette.key_of(self.resolving, cell) {
                        Ok(key) => {
                            filled += 1;
                            key
                        }
                        Err(Unkeyed::RuleTile(slot)) => {
                            self.resolving.record_rule_cell(slot);
                            0
                        }
                        Err(Unkeyed::NoTileset) => {
                            no_tileset += 1;
                            0
                        }
                    }
                };
                row.push(key);
            }
            rows.push(row);
        }
        if no_tileset > 0 {
            self.resolving
                .record_warning(ResolvedWarning::CellsWithoutTileset {
                    layer: layer.name.clone(),
                    cells: no_tileset,
                });
        }
        LayerContent::Tiles {
            filled_cells: filled,
            rows,
        }
    }
}

/// How a layer is composited, in the words of Tiled's files.
fn blend_word(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Normal => "normal",
        BlendMode::Add => "add",
        BlendMode::Multiply => "multiply",
        BlendMode::Screen => "screen",
        BlendMode::Overlay => "overlay",
        BlendMode::Darken => "darken",
        BlendMode::Lighten => "lighten",
        BlendMode::ColorDodge => "color-dodge",
        BlendMode::ColorBurn => "color-burn",
        BlendMode::HardLight => "hard-light",
        BlendMode::SoftLight => "soft-light",
        BlendMode::Difference => "difference",
        BlendMode::Exclusion => "exclusion",
    }
}
