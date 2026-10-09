//! Maps: attributes, tileset references and their global ids, layers, and the ids and
//! invariants the model needs.

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{
    Cell, LayerDataFormat, LayerId, LayerKind, Map, ObjectId, TileId, Tileset, TilesetRef,
    TilesetSlot, Violation,
};
use bt_utils::paths::file_stem;
use glam::{DVec2, IVec2, UVec2};

use super::context::{Finisher, text_of};
use crate::gid::{GidError, GidMapper};
use crate::names;
use crate::paths::is_verbatim_reference;
use crate::raw::{RawMap, RawTileset};
use crate::warning::{Target, WarningKind};

/// What one slot of a [`GidSpace`] is.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct SlotUse {
    /// The highest tile id cells and objects use.
    pub(crate) highest_tile: Option<TileId>,
    /// A stand-in for a tileset that could not be read.
    pub(crate) placeholder: bool,
    /// Added for a template instance's tile; it has no global ids in the file.
    pub(crate) from_template: bool,
}

/// The tilesets of a map or template being finished, with the global ids naming their tiles.
#[derive(Default)]
pub(crate) struct GidSpace {
    /// The tilesets, in slot order.
    pub(crate) tilesets: Vec<TilesetRef>,
    /// First global ids to slots.
    pub(crate) gids: GidMapper,
    /// What each slot is.
    pub(crate) slots: Vec<SlotUse>,
}

impl GidSpace {
    /// Adds a tileset; one read from the file takes its range of global ids.
    pub(crate) fn add_tileset(
        &mut self,
        tileset: TilesetRef,
        placeholder: bool,
        from_template: bool,
    ) -> Option<TilesetSlot> {
        let slot = TilesetSlot::new(self.tilesets.len())?;
        if let (false, Some(first_gid)) = (from_template, tileset.first_gid) {
            self.gids.insert_tileset(first_gid, slot);
        }
        self.tilesets.push(tileset);
        self.slots.push(SlotUse {
            highest_tile: None,
            placeholder,
            from_template,
        });
        Some(slot)
    }

    /// The cell a global id names, noting the tile's use.
    pub(crate) fn cell_of(&mut self, gid: u32) -> Result<Cell, GidError> {
        let cell = self.gids.cell(gid)?;
        if let Some(slot) = cell.slot() {
            self.record_tile_use(slot, cell.tile());
        }
        Ok(cell)
    }

    /// Notes that a tile of `slot` is used.
    pub(crate) fn record_tile_use(&mut self, slot: TilesetSlot, tile: TileId) {
        if let Some(use_) = self.slots.get_mut(slot.index()) {
            use_.highest_tile = Some(use_.highest_tile.map_or(tile, |t| t.max(tile)));
        }
    }

    /// Placeholders keep the range of global ids they had: up to the next tileset's first gid,
    /// or past the highest tile used for the last one.
    pub(crate) fn settle_placeholder_ranges(&mut self) {
        let firsts: Vec<u32> = self
            .tilesets
            .iter()
            .zip(&self.slots)
            .filter(|(_, use_)| !use_.from_template)
            .filter_map(|(tileset, _)| tileset.first_gid)
            .collect();
        for (tileset, use_) in self.tilesets.iter_mut().zip(&self.slots) {
            if !use_.placeholder {
                continue;
            }
            let Some(first) = tileset.first_gid else {
                continue;
            };
            let next = firsts.iter().copied().filter(|g| *g > first).min();
            let span = match next {
                Some(next) => next - first,
                None => use_.highest_tile.map_or(0, |t| t.0.saturating_add(1)),
            };
            Arc::make_mut(&mut tileset.tileset).next_tile_id = span;
        }
    }
}

/// What finishing a map's layers needs to know about the map.
pub(crate) struct MapFinish {
    /// The tilesets and global ids.
    pub(crate) space: GidSpace,
    /// Whether the map is infinite.
    pub(crate) infinite: bool,
    /// The map's size in tiles.
    pub(crate) size: UVec2,
    /// The format of the last layer data read: the map's, as in Tiled.
    pub(crate) layer_data: Option<LayerDataFormat>,
}

impl Finisher<'_> {
    /// A map.
    pub(crate) fn map_of(&mut self, raw: RawMap) -> Map {
        let mut map = Map {
            orientation: self
                .named_of(
                    &raw.orientation,
                    format_args!("the map's orientation"),
                    names::orientation_from_name,
                )
                .unwrap_or_default(),
            render_order: self
                .named_of(
                    &raw.render_order,
                    format_args!("the map's render order"),
                    names::render_order_from_name,
                )
                .unwrap_or_default(),
            width: self
                .u32_of(&raw.width, format_args!("the map's width"))
                .unwrap_or(0),
            height: self
                .u32_of(&raw.height, format_args!("the map's height"))
                .unwrap_or(0),
            tile_width: self
                .u32_of(&raw.tile_width, format_args!("the map's tile width"))
                .unwrap_or(0),
            tile_height: self
                .u32_of(&raw.tile_height, format_args!("the map's tile height"))
                .unwrap_or(0),
            infinite: self
                .bool_of(&raw.infinite, format_args!("the map's infinite"))
                .unwrap_or(false),
            hex_side_length: self
                .i32_of(
                    &raw.hex_side_length,
                    format_args!("the map's hex side length"),
                )
                .unwrap_or(0),
            stagger_axis: self
                .named_of(
                    &raw.stagger_axis,
                    format_args!("the map's stagger axis"),
                    names::stagger_axis_from_name,
                )
                .unwrap_or_default(),
            stagger_index: self
                .named_of(
                    &raw.stagger_index,
                    format_args!("the map's stagger index"),
                    names::stagger_index_from_name,
                )
                .unwrap_or_default(),
            skew: IVec2::new(
                self.i32_of(&raw.skew_x, format_args!("the map's horizontal skew"))
                    .unwrap_or(0),
                self.i32_of(&raw.skew_y, format_args!("the map's vertical skew"))
                    .unwrap_or(0),
            ),
            parallax_origin: DVec2::new(
                self.real_of(
                    &raw.parallax_origin_x,
                    format_args!("the map's parallax origin"),
                )
                .unwrap_or(0.0),
                self.real_of(
                    &raw.parallax_origin_y,
                    format_args!("the map's parallax origin"),
                )
                .unwrap_or(0.0),
            ),
            background_color: self.color_of(
                &raw.background_color,
                format_args!("the map's background colour"),
            ),
            class: text_of(&raw.class),
            next_layer_id: self
                .u32_of(&raw.next_layer_id, format_args!("the map's next layer id"))
                .unwrap_or(0)
                .max(1),
            next_object_id: self
                .u32_of(
                    &raw.next_object_id,
                    format_args!("the map's next object id"),
                )
                .unwrap_or(0)
                .max(1),
            ..Map::default()
        };
        map.file.version = text_of(&raw.version);
        map.file.tiled_version = text_of(&raw.tiled_version);
        map.file.compression_level = self
            .i32_of(
                &raw.compression_level,
                format_args!("the map's compression level"),
            )
            .filter(|level| *level >= 0);
        if let Some(chunk_size) = raw.chunk_size {
            // As in Tiled: 0 is the default, anything else at least 4.
            let side = |value: u32| if value == 0 { 16 } else { value.max(4) };
            map.file.output_chunk_size = UVec2::new(
                side(
                    self.u32_of(&chunk_size.width, format_args!("the map's chunk width"))
                        .unwrap_or(0),
                ),
                side(
                    self.u32_of(&chunk_size.height, format_args!("the map's chunk height"))
                        .unwrap_or(0),
                ),
            );
        }
        if let Some(export) = raw.export {
            map.file.export = self.export_of(export);
        }
        map.properties = self.properties_of(raw.properties);

        let placeholder_size = if map.tile_width > 0 && map.tile_height > 0 {
            map.tile_size()
        } else {
            UVec2::splat(32)
        };
        let mut finish = MapFinish {
            space: GidSpace::default(),
            infinite: map.infinite,
            size: UVec2::new(map.width, map.height),
            layer_data: None,
        };
        for raw_tileset in raw.tilesets {
            let finished = self.tileset_ref_of(raw_tileset, placeholder_size);
            if finish
                .space
                .add_tileset(finished.tileset_ref, finished.placeholder, false)
                .is_none()
            {
                self.record(
                    WarningKind::InvalidValue,
                    "the map has more tilesets than can be held; the rest are dropped",
                );
                break;
            }
        }
        map.layers = self.layers_of(raw.layers, &mut finish);
        if let Some(format) = finish.layer_data {
            map.file.layer_data = format;
        }
        finish.space.settle_placeholder_ranges();
        map.tilesets = finish.space.tilesets;
        let stated = StatedNextIds {
            layer: raw.next_layer_id.is_some(),
            object: raw.next_object_id.is_some(),
        };
        self.settle_ids(&mut map, stated);
        self.record_violations(&map, &finish.space.slots);
        map
    }

    /// A map's or template's `<tileset>`: an embedded tileset, or a tileset file (the built-in
    /// rule tileset for `:/automap-tiles.tsx`); a file that cannot be read becomes a placeholder
    /// named after it that keeps its global ids.
    pub(crate) fn tileset_ref_of(
        &mut self,
        raw: RawTileset,
        placeholder_size: UVec2,
    ) -> FinishedTilesetRef {
        let source = text_of(&raw.source);
        let first_gid = self.u32_of(
            &raw.first_gid,
            format_args!(
                "the first gid of tileset “{}”",
                if source.is_empty() {
                    text_of(&raw.name)
                } else {
                    source.clone()
                }
            ),
        );
        if first_gid.unwrap_or(0) == 0 {
            self.record(
                WarningKind::InvalidValue,
                format!(
                    "tileset “{}” has no first gid; its tiles cannot be named",
                    if source.is_empty() {
                        text_of(&raw.name)
                    } else {
                        source.clone()
                    }
                ),
            );
        }
        if source.trim().is_empty() {
            let tileset = self.tileset_of(raw);
            return FinishedTilesetRef {
                tileset_ref: TilesetRef {
                    file: None,
                    first_gid,
                    tileset: Arc::new(tileset),
                },
                placeholder: false,
            };
        }
        let file = self.file_ref(&source);
        if is_verbatim_reference(source.trim()) {
            if source.trim() == crate::automap_tiles::REFERENCE {
                return FinishedTilesetRef {
                    tileset_ref: TilesetRef {
                        file: Some(file),
                        first_gid,
                        tileset: Arc::clone(self.loader.automap_tiles()),
                    },
                    placeholder: false,
                };
            }
        } else {
            match self.loader.load_tileset_cached(&file.path) {
                Ok(loaded) => {
                    self.warnings.record_all(loaded.warnings);
                    return FinishedTilesetRef {
                        tileset_ref: TilesetRef {
                            file: Some(file),
                            first_gid,
                            tileset: loaded.value,
                        },
                        placeholder: false,
                    };
                }
                Err(error) => {
                    self.warnings.record_about(
                        WarningKind::MissingFile,
                        Some(Target::Tileset(file_stem(&source).to_owned())),
                        format!("the tileset “{source}” cannot be read ({error}); a placeholder keeps its tiles"),
                    );
                }
            }
        }
        let placeholder = Tileset {
            name: file_stem(&source).to_owned(),
            tile_width: placeholder_size.x,
            tile_height: placeholder_size.y,
            ..Tileset::default()
        };
        FinishedTilesetRef {
            tileset_ref: TilesetRef {
                file: Some(file),
                first_gid,
                tileset: Arc::new(placeholder),
            },
            placeholder: true,
        }
    }

    /// Gives layers and objects without a usable id a new one (the second of two with the same
    /// id too), in document order, and raises next ids below the ids in use.
    fn settle_ids(&mut self, map: &mut Map, stated: StatedNextIds) {
        let mut layer_ids = Vec::new();
        let mut object_ids = Vec::new();
        map.for_each_layer(|layer, _| {
            layer_ids.push(layer.id.0);
            if let Some(objects) = layer.as_object_layer() {
                object_ids.extend(objects.objects.iter().map(|o| o.id.0));
            }
        });
        let highest_layer = layer_ids.iter().copied().max().unwrap_or(0);
        let highest_object = object_ids.iter().copied().max().unwrap_or(0);
        if map.next_layer_id <= highest_layer {
            if stated.layer {
                self.record(
                    WarningKind::InvalidValue,
                    format!("the map's next layer id {} is not above the highest in use, {highest_layer}; it is raised", map.next_layer_id),
                );
            }
            map.next_layer_id = highest_layer.saturating_add(1);
        }
        if map.next_object_id <= highest_object {
            if stated.object {
                self.record(
                    WarningKind::InvalidValue,
                    format!("the map's next object id {} is not above the highest in use, {highest_object}; it is raised", map.next_object_id),
                );
            }
            map.next_object_id = highest_object.saturating_add(1);
        }
        let mut seen_layers = HashSet::new();
        let mut seen_objects = HashSet::new();
        let mut next_layer = map.next_layer_id;
        let mut next_object = map.next_object_id;
        let mut renumbered = Vec::new();
        visit_layers_mut(&mut map.layers, &mut |layer| {
            if layer.id.0 == 0 || !seen_layers.insert(layer.id.0) {
                if layer.id.0 != 0 {
                    renumbered.push(format!(
                        "layer {} (“{}”) has the id of another; it becomes {next_layer}",
                        layer.id, layer.name
                    ));
                }
                layer.id = LayerId(next_layer);
                seen_layers.insert(next_layer);
                next_layer = next_layer.saturating_add(1);
            }
            if let LayerKind::Object(objects) = &mut layer.kind {
                let mut layer_seen = HashSet::new();
                let needs = objects.objects.iter().any(|o| {
                    o.id.0 == 0 || seen_objects.contains(&o.id.0) || !layer_seen.insert(o.id.0)
                });
                let list = if needs {
                    Some(Arc::make_mut(&mut objects.objects))
                } else {
                    None
                };
                match list {
                    Some(list) => {
                        for object in list.iter_mut() {
                            if object.id.0 == 0 || !seen_objects.insert(object.id.0) {
                                if object.id.0 != 0 {
                                    renumbered.push(format!(
                                        "object {} has the id of another; it becomes {next_object}",
                                        object.id
                                    ));
                                }
                                object.id = ObjectId(next_object);
                                seen_objects.insert(next_object);
                                next_object = next_object.saturating_add(1);
                            }
                        }
                    }
                    None => seen_objects.extend(objects.objects.iter().map(|o| o.id.0)),
                }
            }
        });
        map.next_layer_id = next_layer;
        map.next_object_id = next_object;
        for message in renumbered {
            self.record(WarningKind::DuplicateId, message);
        }
    }

    /// Reports what `Map::validate` finds that loading did not already report.
    fn record_violations(&mut self, map: &Map, slots: &[super::map::SlotUse]) {
        for violation in map.validate() {
            match violation {
                Violation::TilesOutOfRange { layer, slot, cells } => {
                    if slots.get(slot.index()).is_some_and(|s| s.placeholder) {
                        continue;
                    }
                    let name = map
                        .tileset(slot)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    self.warnings.record_about(
                        WarningKind::UnknownGid,
                        Some(Target::Layer(layer)),
                        format!("{cells} cells of layer {layer} show tiles past the end of tileset “{name}”; they draw nothing until it has them"),
                    );
                }
                Violation::TooManyColors { slot, set } => {
                    let name = map
                        .tileset(slot)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    self.warnings.record_about(
                        WarningKind::InvalidValue,
                        Some(Target::Tileset(name)),
                        format!("terrain set “{set}” has more terrains than a tile can name"),
                    );
                }
                other => self.record(WarningKind::InvalidValue, other.to_string()),
            }
        }
    }
}

/// Whether a map file stated its next ids (a raised one is reported only then).
#[derive(Copy, Clone)]
struct StatedNextIds {
    layer: bool,
    object: bool,
}

/// A finished `<tileset>` of a map or template.
pub(crate) struct FinishedTilesetRef {
    /// The reference.
    pub(crate) tileset_ref: TilesetRef,
    /// Whether it stands in for a file that could not be read.
    pub(crate) placeholder: bool,
}

/// Visits every layer depth-first, parents before their children.
pub(crate) fn visit_layers_mut(
    layers: &mut [bt_core::Layer],
    visit: &mut dyn FnMut(&mut bt_core::Layer),
) {
    for layer in layers {
        visit(layer);
        if let Some(children) = layer.children_mut() {
            visit_layers_mut(children, visit);
        }
    }
}
