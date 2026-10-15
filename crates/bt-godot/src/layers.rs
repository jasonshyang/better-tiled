//! Layers as nodes: tile layers as `TileMapLayer`s, object layers and groups as `Node2D`s, image
//! layers as `Sprite2D`s, and a `Parallax2D` for a parallax factor. What the scene can't hold (the
//! map's background colour, its parallax origin) gets a `godot/left-out` note.
//!
//! The layer conventions (docs/design/12-godot-export.md) are read here, as Tiled's Godot exporter reads them:
//! `noExport` leaves a layer out (a group with its children), `tilesetOnly` leaves a tile layer
//! out but keeps its tilesets in the TileSet, and `ySortEnabled`, `zIndex` and
//! `godotYSortOrigin` become the node's own settings. Each is consumed, so not metadata.

use std::path::Path;

use bt_core::{
    BlendMode, Cell, DrawOrder, Layer, LayerKind, Orientation, Properties, Property, TileLayer,
    Value,
};
use glam::{DVec2, IVec2};

use super::conventions::{
    GODOT_Y_SORT_ORIGIN, NO_EXPORT, TILESET_ONLY, TILESET_RES_PATH, Y_SORT_ENABLED, Z_INDEX,
};
use super::grid::{FLIP_H, FLIP_V, TRANSPOSE};
use super::report::{Element, Note, Severity};
use super::{RULE_TILES, names, text, version};
use crate::build::Build;

/// Godot's `z_index` range (RenderingServer's CANVAS_ITEM_Z_MIN and _MAX); Godot refuses a value
/// beyond it on load and leaves the node at 0.
const Z_LIMIT: i64 = 4096;

/// `noExport` on a layer (or one of its groups, which leaves out the group whole).
pub(super) fn no_export(layer: &Layer) -> bool {
    layer
        .properties
        .get(NO_EXPORT)
        .is_some_and(|p| flag(&p.value))
}

/// Left out of the scene: `noExport`, or `tilesetOnly` on a tile layer.
fn left_out(layer: &Layer) -> bool {
    let tileset_only = matches!(layer.kind, LayerKind::Tile(_))
        && layer
            .properties
            .get(TILESET_ONLY)
            .is_some_and(|p| flag(&p.value));
    no_export(layer) || tileset_only
}

/// A value as a flag, the way Tiled's exporter reads one (`QVariant::toBool`): a number is on
/// unless 0, a string unless empty, `0` or `false`; colors, files, objects, classes and lists are
/// off.
pub(super) fn flag(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::String(s) => !(s.is_empty() || s == "0" || s.eq_ignore_ascii_case("false")),
        _ => false,
    }
}

/// A value as a whole number, the way Tiled's exporter reads one (`QVariant::toInt`): floats
/// rounded, a string parsed, anything else 0.
fn whole(value: &Value) -> i64 {
    match value {
        Value::Int(i) => *i,
        Value::Float(f) if f.is_finite() => f.round() as i64,
        Value::Bool(b) => i64::from(*b),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// What the layers' conventions came to, for one note of each kind once every layer is built.
#[derive(Default)]
struct Found {
    /// Tile layers whose `ySortEnabled` is false: Tiled's exporter y-sorts them anyway.
    y_sort_false: Vec<(u32, String)>,
    /// `zIndex` beyond Godot's range: the layer and its value.
    clamped: Vec<(u32, String, i64)>,
    /// Where `ySortEnabled` turned y-sort on and `zIndex` moved a node although Tiled's exporter
    /// reads them on tile layers only: the map (for `ySortEnabled`), then the layers.
    map_y_sort: bool,
    y_sort_beyond: Vec<(u32, String)>,
    z_beyond: Vec<(u32, String)>,
    /// Object layers drawn top down with `ySortEnabled` true: they are y-sorted without it.
    y_sort_top_down: Vec<(u32, String)>,
    /// Non-orthogonal tile layers with tiles reaching above their cell and no `ySortEnabled`.
    tall: Vec<(u32, String)>,
    /// The project's Godot merges the collision shapes of each physics quadrant (4.5 and later).
    merged_quadrants: bool,
    /// Tile layers made to collide cell by cell, for their mirrored tiles with collision shapes.
    by_cell: Vec<(u32, String)>,
    /// Layers in a Parallax2D.
    parallax: Vec<(u32, String)>,
}

/// What a layer's groups come to: Tiled multiplies their parallax factors and adds their
/// offsets.
#[derive(Clone, Copy)]
struct Above {
    parallax: DVec2,
    offset: DVec2,
}

/// Whether Godot's transform flags in `alternative` mirror the tile: an odd number of them (a
/// flip both ways, or a flip and a transpose, is a turn).
fn mirrored(alternative: u16) -> bool {
    (alternative & (FLIP_H | FLIP_V | TRANSPOSE)).count_ones() % 2 == 1
}

impl Build<'_> {
    /// The map's own conventions on the root node, its class and other properties as metadata,
    /// then its layers, then the notes on what the conventions did.
    pub(super) fn map_layers(&mut self, root: usize) -> Result<(), String> {
        let map = self.map;
        let mut found = Found {
            merged_quadrants: version::merges_physics_quadrants(&self.project),
            ..Found::default()
        };
        let rest = self.unconsumed(&map.properties, &[Y_SORT_ENABLED, TILESET_RES_PATH]);
        if map
            .properties
            .get(Y_SORT_ENABLED)
            .is_some_and(|p| flag(&p.value))
        {
            self.prop(root, "y_sort_enabled", "true".to_owned());
            found.map_y_sort = true;
        }
        self.meta(root, &map.class, &rest);
        self.layers(
            &map.layers,
            ".",
            Above {
                parallax: DVec2::ONE,
                offset: DVec2::ZERO,
            },
            &mut found,
        )?;
        self.left_out_notes(&found.parallax);
        self.convention_notes(found);
        Ok(())
    }

    /// The `godot/left-out` notes on what the map has and its scene can't hold: the background
    /// colour, and the parallax origin of the layers in a Parallax2D.
    fn left_out_notes(&mut self, parallax: &[(u32, String)]) {
        let map = self.map;
        if let Some(c) = map.background_color {
            // As Tiled writes it: #RRGGBB, or #AARRGGBB when it isn't opaque.
            let alpha = if c.a == 255 {
                String::new()
            } else {
                format!("{:02x}", c.a)
            };
            let message = format!(
                "the map's background colour #{alpha}{:02x}{:02x}{:02x} isn't in the scene, which has no background of its own: in Godot it is Project Settings ▸ Rendering ▸ Environment ▸ Defaults ▸ Default Clear Color",
                c.r, c.g, c.b
            );
            self.notes.push(Note {
                code: crate::Code::LeftOut,
                severity: Severity::Info,
                message,
                elements: Vec::new(),
            });
        }
        // A Parallax2D sits at (1 - scroll_scale) × the top-left corner of the camera's view,
        // where Tiled draws a layer at (1 - factor) × (the view's centre - the parallax origin):
        // the same when that origin is the centre of the game's window, at zoom 1.
        if parallax.is_empty() {
            return;
        }
        let (o, centre) = (map.parallax_origin, window_size(&self.project) / 2.0);
        if o != centre {
            let message = format!(
                "the map's parallax origin ({}, {}) isn't in the scene: Godot's Parallax2D scrolls {} about the top-left corner of the camera's view, where Tiled scrolls about the parallax origin, so they scroll as in Tiled only with the parallax origin at the centre of the game's window, ({}, {}), and the camera not zoomed",
                o.x,
                o.y,
                listed(parallax),
                centre.x,
                centre.y
            );
            self.notes.push(Note {
                code: crate::Code::LeftOut,
                severity: Severity::Info,
                message,
                elements: parallax.iter().map(|(id, _)| Element::Layer(*id)).collect(),
            });
        }
    }

    /// `properties` without those of `names` it has, which are consumed.
    fn unconsumed(&mut self, properties: &Properties, names: &[&'static str]) -> Properties {
        let mut rest = properties.clone();
        for &name in names {
            if self.consumed.take(properties, name).is_some() {
                rest.remove(name);
            }
        }
        rest
    }

    /// `layers` under the node `parent`, inside groups that come to `above`.
    fn layers(
        &mut self,
        layers: &[Layer],
        parent: &str,
        above: Above,
        found: &mut Found,
    ) -> Result<(), String> {
        let map = self.map;
        // Left out before naming, so a layer left out never pushes a name off a sibling.
        let mut kept = Vec::new();
        for layer in layers {
            if left_out(layer) {
                self.unconsumed(&layer.properties, &[NO_EXPORT, TILESET_ONLY]);
            } else {
                kept.push(layer);
            }
        }
        for (layer, name) in kept
            .iter()
            .copied()
            .zip(names::layer_names(kept.iter().copied()))
        {
            self.counts.layers += 1;
            let tile = matches!(layer.kind, LayerKind::Tile(_));
            let conventions: &[&'static str] = if tile {
                &[
                    NO_EXPORT,
                    TILESET_ONLY,
                    Y_SORT_ENABLED,
                    Z_INDEX,
                    GODOT_Y_SORT_ORIGIN,
                ]
            } else {
                &[NO_EXPORT, Y_SORT_ENABLED, Z_INDEX]
            };
            let rest = self.unconsumed(&layer.properties, conventions);
            let get = |name: &str| layer.properties.get(name).map(|p| &p.value);
            let id = (layer.id.0, layer.name.clone());

            // y-sort as the layer says; without ySortEnabled, only a top-down object layer is.
            let y_sort_set = get(Y_SORT_ENABLED).map(flag);
            let top_down = matches!(&layer.kind, LayerKind::Object(objects) if objects.draw_order == DrawOrder::TopDown);
            let y_sort = y_sort_set.unwrap_or(top_down);
            match (tile, y_sort_set) {
                (true, Some(false)) => found.y_sort_false.push(id.clone()),
                (false, Some(true)) if top_down => found.y_sort_top_down.push(id.clone()),
                (false, Some(true)) => found.y_sort_beyond.push(id.clone()),
                _ => {}
            }
            if let LayerKind::Tile(tiles) = &layer.kind
                && y_sort_set.is_none()
                && map.orientation != Orientation::Orthogonal
                && self.paints_tall_tiles(tiles)
            {
                found.tall.push(id.clone());
            }
            let z_index = get(Z_INDEX).map(whole).unwrap_or(0);
            let z_clamped = z_index.clamp(-Z_LIMIT, Z_LIMIT);
            if z_clamped != z_index {
                found
                    .clamped
                    .push((layer.id.0, layer.name.clone(), z_index));
            }
            if !tile && z_clamped != 0 {
                found.z_beyond.push(id.clone());
            }
            let y_sort_origin = if tile {
                get(GODOT_Y_SORT_ORIGIN).map(whole).unwrap_or(0)
            } else {
                0
            };

            let unique_id = self.names.layer(layer.id.0);
            // Tiled scrolls a layer by its parallax factor times its groups', and a group by
            // nothing of its own, while Godot adds up the scrolls of nested Parallax2Ds and sets a
            // Parallax2D's position itself. So a group stays a Node2D at its offset, and a layer
            // whose factor with its groups' isn't 1 goes in a Parallax2D with that product.
            let group = matches!(layer.kind, LayerKind::Group(_));
            let factor = above.parallax * layer.parallax;
            let holder = if factor != DVec2::ONE && !group {
                let node = self.node(Some(parent), &name, "Parallax2D");
                // Named after its layer, so it needs an id to keep its overrides through a rename.
                {
                    let uid = self.names.helper(unique_id, "parallax", 0);
                    self.set_node_id(node, uid);
                }
                self.prop(node, "scroll_scale", text::vector2(factor));
                found.parallax.push(id.clone());
                Some(self.node_path(node))
            } else {
                None
            };
            let parent = holder.as_deref().unwrap_or(parent);
            let kind = match &layer.kind {
                LayerKind::Tile(_) => "TileMapLayer",
                LayerKind::Object(_) | LayerKind::Group(_) => "Node2D",
                LayerKind::Image(_) => "Sprite2D",
            };
            let node = self.node(Some(parent), &name, kind);
            {
                let uid = unique_id;
                self.set_node_id(node, uid);
            }
            let position = self.grid.layer_position(&layer.kind, layer.offset);
            if position != DVec2::ZERO {
                self.prop(node, "position", text::vector2(position));
            }
            let tint = layer
                .tint
                .unwrap_or(bt_core::Color::rgba(255, 255, 255, 255));
            if layer.tint.is_some() || layer.opacity != 1.0 {
                let alpha = (f64::from(tint.a) * layer.opacity)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                self.prop(
                    node,
                    "modulate",
                    text::color(bt_core::Color::rgba(tint.r, tint.g, tint.b, alpha)),
                );
            }
            if !layer.visible {
                self.prop(node, "visible", "false".to_owned());
            }
            match layer.blend_mode {
                BlendMode::Normal => {}
                BlendMode::Add | BlendMode::Multiply => {
                    let mode = if layer.blend_mode == BlendMode::Add {
                        "1"
                    } else {
                        "3"
                    };
                    let material = self.sub(
                        "CanvasItemMaterial",
                        vec![("blend_mode".to_owned(), mode.to_owned())],
                    );
                    self.prop(node, "material", material);
                }
                other => self.warn_once(
                    crate::Code::BlendMode,
                    format!(
                        "layer {}: Godot has no {other:?} blend mode; it is drawn normally",
                        layer.name
                    ),
                    vec![Element::Layer(layer.id.0)],
                ),
            }
            // On the layer's own node, inside a parallax holder too: z_index adds up through
            // parents, and a TileMapLayer y-sorts its own tiles.
            if z_clamped != 0 {
                self.prop(node, "z_index", z_clamped.to_string());
            }
            if y_sort {
                self.prop(node, "y_sort_enabled", "true".to_owned());
            }
            // As objects carry `metadata/id`: the next export finds the layer by it, whatever the
            // Godot version.
            self.push_node_meta(
                node,
                "layer_id".to_owned(),
                Property::new(Value::Int(i64::from(layer.id.0))),
            );
            self.meta(node, &layer.class, &rest);
            let path = self.node_path(node);
            self.names.placed(Element::Layer(layer.id.0), &path);
            match &layer.kind {
                LayerKind::Tile(tiles) => {
                    let mut data: Vec<u8> = vec![0, 0];
                    let mut cells: Vec<(IVec2, Cell)> =
                        tiles.cells().map(|p| (p.at, p.cell)).collect();
                    cells.sort_by_key(|(p, _)| (p.y, p.x));
                    let (mut mirrored_shapes, mut reaching_shapes) = (false, false);
                    for (p, cell) in cells {
                        if !(i16::MIN as i32..=i16::MAX as i32).contains(&p.x)
                            || !(i16::MIN as i32..=i16::MAX as i32).contains(&p.y)
                        {
                            return Err(format!(
                                "layer {}: cell {}, {} is beyond what Godot stores",
                                layer.name, p.x, p.y
                            ));
                        }
                        let Some((source, atlas, alternative)) = self.cell(cell) else {
                            continue;
                        };
                        for v in [
                            p.x as i16 as u16,
                            p.y as i16 as u16,
                            source,
                            atlas.x as u16,
                            atlas.y as u16,
                            alternative,
                        ] {
                            data.extend_from_slice(&v.to_le_bytes());
                        }
                        self.counts.cells += 1;
                        if let Some(reaching) = self.collision(source, atlas, alternative) {
                            mirrored_shapes |= mirrored(alternative);
                            reaching_shapes |= reaching;
                        }
                    }
                    if data.len() > 2 {
                        self.prop(node, "tile_map_data", text::bytes(&data));
                    }
                    let tile_set = self.tile_set();
                    self.prop(node, "tile_set", tile_set);
                    // Godot merges the collision shapes of a physics quadrant's cells into one
                    // body, and cuts a mirrored tile's shapes away where they overlap another
                    // cell's (or loses the quadrant's): such layers collide cell by cell. Shapes
                    // inside their cells only touch, which Godot merges well.
                    if mirrored_shapes && reaching_shapes && found.merged_quadrants {
                        self.prop(node, "physics_quadrant_size", "1".to_owned());
                        found.by_cell.push(id.clone());
                    }
                    if y_sort_origin != 0 {
                        let origin = y_sort_origin.clamp(i32::MIN as i64, i32::MAX as i64);
                        self.prop(node, "y_sort_origin", origin.to_string());
                    }
                }
                LayerKind::Object(objects) => {
                    let mut kept = Vec::new();
                    for object in objects.objects.iter() {
                        if self.left_out(object) {
                            continue;
                        }
                        // One of AutoMap's rule tiles: left out, as their cells are.
                        let slot = object.tile.and_then(|c| c.slot()).map(|s| s.index());
                        if slot.is_some_and(|s| self.is_rule_slot(s)) {
                            self.warn_once(
                                crate::Code::RuleTiles,
                                RULE_TILES.to_owned(),
                                Vec::new(),
                            );
                            continue;
                        }
                        kept.push(object);
                    }
                    for (object, name) in kept.iter().zip(names::object_names(&kept)) {
                        self.object(object, &path, &name);
                    }
                }
                LayerKind::Image(image) => {
                    if let Some(source) = &image.image {
                        let texture = self.texture(source);
                        self.prop(node, "texture", texture);
                        self.prop(node, "centered", "false".to_owned());
                        if image.repeat_x || image.repeat_y {
                            // Tiled repeats the image from the layer's origin without end; here
                            // it covers the map, in whole images from that origin so that each
                            // falls where Tiled draws it.
                            let bounds = map.bounding_rect();
                            let origin = above.offset + layer.offset;
                            let span =
                                |repeat: bool, start: i32, length: i32, from: f64, size: u32| {
                                    let size = f64::from(size.max(1));
                                    if !repeat {
                                        return (0.0, size);
                                    }
                                    let first = ((f64::from(start) - from) / size).floor() * size;
                                    let last = ((f64::from(start) + f64::from(length) - from)
                                        / size)
                                        .ceil()
                                        * size;
                                    (first, last.max(first + size))
                                };
                            let (x0, x1) = span(
                                image.repeat_x,
                                bounds.x,
                                bounds.w,
                                origin.x,
                                source.declared_size.x,
                            );
                            let (y0, y1) = span(
                                image.repeat_y,
                                bounds.y,
                                bounds.h,
                                origin.y,
                                source.declared_size.y,
                            );
                            self.prop(node, "texture_repeat", "2".to_owned());
                            self.prop(node, "region_enabled", "true".to_owned());
                            self.prop(
                                node,
                                "region_rect",
                                text::rect2(DVec2::new(x0, y0), DVec2::new(x1 - x0, y1 - y0)),
                            );
                            if (x0, y0) != (0.0, 0.0) {
                                self.prop(node, "offset", text::vector2(DVec2::new(x0, y0)));
                            }
                        }
                    }
                }
                LayerKind::Group(children) => self.layers(
                    children,
                    &path,
                    Above {
                        parallax: factor,
                        offset: above.offset + layer.offset,
                    },
                    found,
                )?,
            }
        }
        Ok(())
    }

    /// Whether a tile layer paints a tile reaching above its cell (taller than the cell, or
    /// raised by its tileset's offset): Godot draws such tiles by rendering quadrant, not in row
    /// order, unless the layer is y-sorted.
    fn paints_tall_tiles(&self, tiles: &TileLayer) -> bool {
        let cell = self.grid.cell_box().y;
        tiles.cells().any(|p| {
            let c = p.cell;
            let Some(slot) = c
                .slot()
                .map(|s| s.index())
                .filter(|&s| !self.is_rule_slot(s))
            else {
                return false;
            };
            let Some(ts) = self.map.tilesets.get(slot).map(|t| &t.tileset) else {
                return false;
            };
            let height = if ts.is_collection() {
                ts.tile_image(c.tile()).map_or(0, |tile| tile.rect.h)
            } else {
                ts.tile_height as i32
            };
            height - ts.tile_offset.y > cell
        })
    }

    /// The collision of the tile at `atlas` of the source numbered `source`, on a cell with
    /// Godot's `alternative`: `None` without collision shapes, else whether they may reach out
    /// of the cell, where another cell's shapes can overlap them.
    fn collision(&self, source: u16, atlas: IVec2, alternative: u16) -> Option<bool> {
        let source = self
            .tileset
            .sources
            .iter()
            .find(|s| s.id == usize::from(source))?;
        let tile = source.tiles.get(&(atlas.x, atlas.y))?;
        let mut points = tile
            .polygons
            .iter()
            .filter(|p| p.physics_layer.is_some())
            .flat_map(|p| &p.points)
            .peekable();
        points.peek()?;
        // Off the orthogonal grid a cell isn't its box.
        if self.map.orientation != Orientation::Orthogonal {
            return Some(true);
        }
        // Godot's polygons are around the cell's centre, moved on the tile's own alternatives
        // (grid.rs); mirroring keeps them in the cell's box, transposing swaps its sides.
        let id = alternative & !(FLIP_H | FLIP_V | TRANSPOSE);
        let moved = tile
            .flipped
            .iter()
            .find(|f| id != 0 && f.id == id)
            .map_or(DVec2::ZERO, |f| (tile.origin - f.origin).as_dvec2());
        let half = self.grid.cell_box().as_dvec2() / 2.0;
        let half = if alternative & TRANSPOSE != 0 {
            DVec2::new(half.y, half.x)
        } else {
            half
        };
        Some(
            points
                .map(|p| *p + moved)
                .any(|p| p.x.abs() > half.x + 1e-6 || p.y.abs() > half.y + 1e-6),
        )
    }

    /// One note of each kind for what the layer conventions did.
    fn convention_notes(&mut self, found: Found) {
        let elements = |layers: &[(u32, String)]| {
            layers
                .iter()
                .map(|(id, _)| Element::Layer(*id))
                .collect::<Vec<_>>()
        };
        if !found.y_sort_false.is_empty() {
            let (layers, them) = (
                listed(&found.y_sort_false),
                if found.y_sort_false.len() == 1 {
                    "it"
                } else {
                    "them"
                },
            );
            let message = format!(
                "{layers}: ySortEnabled is false, so Godot doesn't y-sort {them} (Tiled's Godot exporter y-sorts a tile layer with any ySortEnabled value); make it true to y-sort {them}, or remove it to leave {them} unsorted"
            );
            self.warn_once(
                crate::Code::YsortFalse,
                message,
                elements(&found.y_sort_false),
            );
        }
        if !found.clamped.is_empty() {
            let values: Vec<String> = found
                .clamped
                .iter()
                .map(|&(_, ref name, z)| {
                    format!("layer {name} {z} becomes {}", z.clamp(-Z_LIMIT, Z_LIMIT))
                })
                .collect();
            let message = format!(
                "zIndex beyond Godot's range (-{Z_LIMIT} to {Z_LIMIT}), which Godot would refuse and leave at 0, is clamped: {}",
                values.join(", ")
            );
            self.warn_once(
                crate::Code::ZindexClamped,
                message,
                found
                    .clamped
                    .iter()
                    .map(|(id, ..)| Element::Layer(*id))
                    .collect(),
            );
        }
        if !found.y_sort_top_down.is_empty() {
            let message = format!(
                "{}: ySortEnabled changes nothing: an object layer drawn top down is y-sorted without it (Tiled's Godot exporter y-sorts no object layer)",
                listed(&found.y_sort_top_down)
            );
            self.notes.push(Note {
                code: crate::Code::YsortTopDown,
                severity: Severity::Info,
                message,
                elements: elements(&found.y_sort_top_down),
            });
        }
        let mut beyond = Vec::new();
        if found.map_y_sort || !found.y_sort_beyond.is_empty() {
            let map = if found.map_y_sort {
                Some("the map")
            } else {
                None
            };
            let layers = (!found.y_sort_beyond.is_empty()).then(|| listed(&found.y_sort_beyond));
            beyond.push(format!(
                "ySortEnabled on {}",
                map.into_iter()
                    .map(str::to_owned)
                    .chain(layers)
                    .collect::<Vec<_>>()
                    .join(" and ")
            ));
        }
        if !found.z_beyond.is_empty() {
            beyond.push(format!("zIndex on {}", listed(&found.z_beyond)));
        }
        if !beyond.is_empty() {
            let message = format!(
                "Tiled's Godot exporter reads ySortEnabled and zIndex on tile layers only; here they also apply: {}",
                beyond.join("; ")
            );
            let mut layers = found.y_sort_beyond;
            for layer in found.z_beyond {
                if !layers.contains(&layer) {
                    layers.push(layer);
                }
            }
            self.notes.push(Note {
                code: crate::Code::DiffersFromTiled,
                severity: Severity::Info,
                message,
                elements: elements(&layers),
            });
        }
        if !found.tall.is_empty() {
            let message = format!(
                "{}: tiles taller than their cell overlap by rendering quadrant, not row by row, unless the layer has ySortEnabled; give a layer ySortEnabled = true to draw its tiles in row order",
                listed(&found.tall)
            );
            self.notes.push(Note {
                code: crate::Code::TallTilesNoYsort,
                severity: Severity::Info,
                message,
                elements: elements(&found.tall),
            });
        }
        if !found.by_cell.is_empty() {
            let message = format!(
                "{}: mirrored tiles with collision shapes that can overlap another cell's, so each cell collides on its own (physics_quadrant_size = 1); Godot 4.5 and later merge the shapes of 16×16 cells into one body, which cuts a mirrored tile's shapes away where they overlap another cell's",
                listed(&found.by_cell)
            );
            self.notes.push(Note {
                code: crate::Code::PhysicsQuadrantSize,
                severity: Severity::Info,
                message,
                elements: elements(&found.by_cell),
            });
        }
    }
}

/// The size of the game's window: `window/size/viewport_width` and `viewport_height` in the
/// `[display]` section of the project's project.godot, else Godot's 1152×648.
fn window_size(project: &Path) -> DVec2 {
    let text = std::fs::read_to_string(project.join("project.godot")).unwrap_or_default();
    let mut size = DVec2::new(1152.0, 648.0);
    let mut section = "";
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim();
        } else if section == "display"
            && let Some((key, value)) = line.split_once('=')
            && let Ok(value) = value.trim().parse::<f64>()
        {
            match key.trim() {
                "window/size/viewport_width" => size.x = value,
                "window/size/viewport_height" => size.y = value,
                _ => {}
            }
        }
    }
    size
}

/// `layer a` or `layers a, b`.
fn listed(layers: &[(u32, String)]) -> String {
    let names: Vec<&str> = layers.iter().map(|(_, name)| name.as_str()).collect();
    format!(
        "{} {}",
        if names.len() == 1 { "layer" } else { "layers" },
        names.join(", ")
    )
}
