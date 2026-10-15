//! Tilesets: their contents, images, tiles, collision groups and animations.

use bt_core::{
    CollisionGroup, EmbeddedImage, ExportSettings, Frame, GridOrientation, ImageRef, ImageSource,
    LayerId, ObjectLayer, Properties, TileId, Tileset, TilesetGrid, TilesetImage, Transformations,
    Value,
};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use super::context::{Finisher, text_of};
use super::property::PropertyDecoder;
use super::wang::TileTerrain;
use crate::data::decode_base64_bytes;
use crate::names;
use crate::paths::is_verbatim_reference;
use crate::raw::{RawExport, RawImage, RawLayer, RawTile, RawTileset, Scalar};
use crate::warning::{Target, WarningKind};

/// An image reference with the size its file has, when it could be read.
pub(crate) struct FinishedImage {
    /// The reference.
    pub(crate) image: ImageRef,
    /// The size of the image file or bytes.
    pub(crate) actual_size: Option<UVec2>,
}

impl Finisher<'_> {
    /// A tileset's contents: embedded in a map or template, or the root of a tileset file.
    pub(crate) fn tileset_of(&mut self, raw: RawTileset) -> Tileset {
        let name = text_of(&raw.name);
        let mut tileset = Tileset {
            class: text_of(&raw.class),
            tile_width: self
                .u32_of(
                    &raw.tile_width,
                    format_args!("tileset “{name}”'s tile width"),
                )
                .unwrap_or(0),
            tile_height: self
                .u32_of(
                    &raw.tile_height,
                    format_args!("tileset “{name}”'s tile height"),
                )
                .unwrap_or(0),
            spacing: self
                .u32_of(&raw.spacing, format_args!("tileset “{name}”'s spacing"))
                .unwrap_or(0),
            margin: self
                .u32_of(&raw.margin, format_args!("tileset “{name}”'s margin"))
                .unwrap_or(0),
            background_color: self.color_of(
                &raw.background_color,
                format_args!("tileset “{name}”'s background colour"),
            ),
            object_alignment: self
                .named_of(
                    &raw.object_alignment,
                    format_args!("tileset “{name}”'s object alignment"),
                    names::alignment_from_name,
                )
                .unwrap_or_default(),
            tile_render_size: self
                .named_of(
                    &raw.tile_render_size,
                    format_args!("tileset “{name}”'s tile render size"),
                    names::tile_render_size_from_name,
                )
                .unwrap_or_default(),
            fill_mode: self
                .named_of(
                    &raw.fill_mode,
                    format_args!("tileset “{name}”'s fill mode"),
                    names::fill_mode_from_name,
                )
                .unwrap_or_default(),
            name,
            ..Tileset::default()
        };
        let name = tileset.name.clone();
        if let Some(export) = raw.export {
            tileset.file.export = self.export_of(export);
        }
        if let Some(offset) = raw.tile_offset {
            tileset.tile_offset = IVec2::new(
                self.i32_of(&offset.x, format_args!("tileset “{name}”'s tile offset"))
                    .unwrap_or(0),
                self.i32_of(&offset.y, format_args!("tileset “{name}”'s tile offset"))
                    .unwrap_or(0),
            );
        }
        if let Some(grid) = raw.grid {
            let orientation = self
                .named_of(
                    &grid.orientation,
                    format_args!("tileset “{name}”'s grid orientation"),
                    names::grid_orientation_from_name,
                )
                .unwrap_or_default();
            let width = self
                .u32_of(&grid.width, format_args!("tileset “{name}”'s grid width"))
                .unwrap_or(0);
            let height = self
                .u32_of(&grid.height, format_args!("tileset “{name}”'s grid height"))
                .unwrap_or(0);
            let size = if width > 0 && height > 0 {
                UVec2::new(width, height)
            } else {
                tileset.tile_size()
            };
            if orientation != GridOrientation::Orthogonal || size != tileset.tile_size() {
                tileset.grid = Some(TilesetGrid { orientation, size });
            }
        }
        if let Some(raw_flags) = raw.transformations {
            let mut flags = Transformations::empty();
            for (value, flag) in [
                (&raw_flags.hflip, Transformations::HFLIP),
                (&raw_flags.vflip, Transformations::VFLIP),
                (&raw_flags.rotate, Transformations::ROTATE),
                (
                    &raw_flags.prefer_untransformed,
                    Transformations::PREFER_UNTRANSFORMED,
                ),
            ] {
                if self.bool_of(value, format_args!("tileset “{name}”'s transformations"))
                    == Some(true)
                {
                    flags |= flag;
                }
            }
            tileset.transformations = flags;
        }
        tileset.properties = self.properties_of(raw.properties);

        let atlas = raw
            .image
            .and_then(|image| self.image_of(image, true, format_args!("tileset “{name}”'s image")));
        let mut terrains = Vec::new();
        let mut order = Vec::new();
        for raw_tile in raw.tiles {
            if let Some(id) = self.tile_into(&mut tileset, raw_tile, &mut terrains) {
                order.push(id);
            }
        }
        tileset.image = match atlas {
            Some(finished) => self.atlas_of(&tileset, finished, &raw.tile_count, &raw.columns),
            None => TilesetImage::Collection,
        };
        if let TilesetImage::Atlas {
            image, tile_count, ..
        } = &tileset.image
        {
            // An atlas's tiles exist whether listed or not; an entry inside the image the file
            // declares, with no data (a tile that only had terrain corners, say), says nothing.
            let declared = image.declared_size;
            let grid = if declared.x > 0 && declared.y > 0 {
                tileset
                    .columns_for_width(declared.x)
                    .saturating_mul(tileset.rows_for_height(declared.y))
            } else {
                *tile_count
            };
            let empty = bt_core::TileData::default();
            tileset
                .tiles
                .retain(|id, tile| id.0 >= grid || *tile != empty);
        }
        let after_last_listed = tileset
            .tiles
            .keys()
            .next_back()
            .map_or(0, |id| id.0.saturating_add(1));
        tileset.next_tile_id = tileset.tile_count().max(after_last_listed);
        if tileset.is_collection() {
            let mut seen = std::collections::HashSet::new();
            order.retain(|id| seen.insert(*id));
            let by_id: Vec<TileId> = tileset.tiles.keys().copied().collect();
            if order != by_id {
                tileset.set_tile_order(order);
            }
        }
        self.wang_sets_into(&mut tileset, raw.terrains, terrains, raw.wang_sets);
        tileset
    }

    /// An export target (`.` and empty are none).
    pub(crate) fn export_of(&mut self, raw: RawExport) -> ExportSettings {
        let target = text_of(&raw.target);
        ExportSettings {
            target: (!target.trim().is_empty() && target != ".").then(|| self.file_ref(&target)),
            format: text_of(&raw.format),
        }
    }

    /// Properties, decoded with the loader's custom types.
    pub(crate) fn properties_of(&mut self, raw: Vec<crate::raw::RawProperty>) -> Properties {
        let types = std::sync::Arc::clone(&self.types);
        PropertyDecoder::new(&types, &mut self.warnings).properties_of(raw)
    }

    /// An image reference, with the size of the image itself. `None` for an `<image>` with
    /// neither a source nor data. `trans` is kept only where Tiled reads it.
    pub(crate) fn image_of(
        &mut self,
        raw: RawImage,
        keep_trans: bool,
        what: std::fmt::Arguments<'_>,
    ) -> Option<FinishedImage> {
        let source = text_of(&raw.source);
        let declared = UVec2::new(
            self.u32_of(&raw.width, format_args!("{what}'s width"))
                .unwrap_or(0),
            self.u32_of(&raw.height, format_args!("{what}'s height"))
                .unwrap_or(0),
        );
        let transparent = if keep_trans {
            self.color_of(&raw.trans, format_args!("{what}'s transparent colour"))
        } else {
            None
        };
        let mut finished = if !source.trim().is_empty() {
            let file = self.file_ref(&source);
            let actual_size = self.probed_size(&file.path, &source, what);
            FinishedImage {
                image: ImageRef {
                    source: ImageSource::File(file),
                    declared_size: declared,
                    transparent,
                },
                actual_size,
            }
        } else if let Some(data) = raw.data {
            let encoding = text_of(&data.encoding);
            if !encoding.is_empty() && encoding != "base64" {
                self.record(
                    WarningKind::InvalidValue,
                    format!("{what} is embedded in the encoding “{encoding}”, which is not base64; it is left out"),
                );
                return None;
            }
            let Some(bytes) = decode_base64_bytes(&data.text) else {
                self.record(
                    WarningKind::InvalidValue,
                    format!("{what} is embedded as invalid base64; it is left out"),
                );
                return None;
            };
            let format = text_of(&raw.format);
            let actual_size = bt_image::probe_size_of_bytes(&bytes, &format).ok();
            if actual_size.is_none() {
                self.record(
                    WarningKind::InvalidValue,
                    format!("{what} is embedded, but its bytes are not an image that can be read"),
                );
            }
            FinishedImage {
                image: ImageRef {
                    source: ImageSource::Embedded(EmbeddedImage {
                        format,
                        bytes: bytes.into(),
                    }),
                    declared_size: declared,
                    transparent,
                },
                actual_size,
            }
        } else {
            return None;
        };
        // Tiled writes the size of the image it loaded; a file that states none gets it.
        if let Some(actual) = finished.actual_size
            && (declared.x == 0 || declared.y == 0)
        {
            finished.image.declared_size = actual;
        }
        Some(finished)
    }

    /// The size of an image file from its header; `None` (with a warning) when it cannot be
    /// read. Built-in resources and maps used as images are not probed.
    fn probed_size(
        &mut self,
        path: &std::path::Path,
        source: &str,
        what: std::fmt::Arguments<'_>,
    ) -> Option<UVec2> {
        if is_verbatim_reference(source.trim()) || bt_image::is_map_file(path) {
            return None;
        }
        match self.loader.image_size(path) {
            Ok(size) => Some(size),
            Err(error) => {
                let kind = if path.exists() {
                    WarningKind::InvalidValue
                } else {
                    WarningKind::MissingFile
                };
                self.warnings.record_file(
                    kind,
                    path,
                    None,
                    format!("{what} “{source}” cannot be read ({error})"),
                );
                None
            }
        }
    }

    /// An atlas: its grid counted from the image that was read, as Tiled counts it; the
    /// declared size when the image could not be read; the `tilecount` and `columns` as written
    /// when no size is known at all.
    fn atlas_of(
        &mut self,
        tileset: &Tileset,
        finished: FinishedImage,
        tile_count: &Option<Scalar>,
        columns: &Option<Scalar>,
    ) -> TilesetImage {
        let name = tileset.name.clone();
        if tileset.tile_width == 0 || tileset.tile_height == 0 {
            self.record(
                WarningKind::InvalidValue,
                format!("tileset “{name}” has an image but no tile size; it has no tiles"),
            );
        }
        let size = finished.actual_size.or_else(|| {
            (finished.image.declared_size.min_element() > 0).then_some(finished.image.declared_size)
        });
        match size {
            Some(size) => {
                let columns = tileset.columns_for_width(size.x);
                TilesetImage::Atlas {
                    image: finished.image,
                    columns,
                    tile_count: columns.saturating_mul(tileset.rows_for_height(size.y)),
                }
            }
            None => {
                let columns = self
                    .u32_of(columns, format_args!("tileset “{name}”'s columns"))
                    .unwrap_or(0);
                let count = self
                    .u32_of(tile_count, format_args!("tileset “{name}”'s tile count"))
                    .unwrap_or(0);
                let listed_beyond =
                    u32::try_from(tileset.tiles.keys().filter(|id| id.0 >= count).count())
                        .unwrap_or(0);
                TilesetImage::Atlas {
                    image: finished.image,
                    columns,
                    tile_count: count.saturating_sub(listed_beyond),
                }
            }
        }
    }

    /// Adds a `<tile>`'s data to the tileset (merging into an earlier entry with its id, as
    /// Tiled does); returns its id, or `None` when the entry is unusable.
    fn tile_into(
        &mut self,
        tileset: &mut Tileset,
        raw: RawTile,
        terrains: &mut Vec<TileTerrain>,
    ) -> Option<TileId> {
        let name = tileset.name.clone();
        let id = match &raw.id {
            None => TileId(0),
            Some(_) => TileId(self.u32_of(&raw.id, format_args!("a tile id of tileset “{name}”"))?),
        };
        let what = format!("tile {id} of tileset “{name}”");
        let class = match text_of(&raw.class) {
            class if !class.is_empty() => class,
            _ => text_of(&raw.class_as_type),
        };
        let probability = self.real_of(&raw.probability, format_args!("{what}'s probability"));
        let rect = {
            let x = self
                .i32_of(&raw.x, format_args!("{what}'s image rectangle"))
                .unwrap_or(0);
            let y = self
                .i32_of(&raw.y, format_args!("{what}'s image rectangle"))
                .unwrap_or(0);
            let w = self
                .i32_of(&raw.width, format_args!("{what}'s image rectangle"))
                .unwrap_or(0);
            let h = self
                .i32_of(&raw.height, format_args!("{what}'s image rectangle"))
                .unwrap_or(0);
            (w > 0 && h > 0).then_some(IRect { x, y, w, h })
        };
        if let Some(corners) = raw.terrain {
            terrains.push(TileTerrain { tile: id, corners });
        }
        let mut properties = self.properties_of(raw.properties);
        let image = raw
            .image
            .and_then(|image| self.image_of(image, false, format_args!("{what}'s image")))
            .map(|finished| finished.image);
        let collision = raw
            .collision
            .map(|group| self.collision_of(group, &mut properties, &name, id));
        let animation = raw.animation.map(|frames| {
            frames
                .into_iter()
                .map(|frame| Frame {
                    tile: TileId(
                        self.u32_of(&frame.tile_id, format_args!("{what}'s animation frame"))
                            .unwrap_or(0),
                    ),
                    duration_ms: self
                        .u32_of(&frame.duration, format_args!("{what}'s frame duration"))
                        .unwrap_or(0),
                })
                .collect::<Vec<_>>()
        });

        let tile = tileset.tiles.entry(id).or_default();
        tile.class = class;
        tile.rect = rect;
        if let Some(probability) = probability {
            tile.probability = probability;
        }
        tile.properties.extend(properties);
        if image.is_some() {
            tile.image = image;
        }
        if collision.is_some() {
            tile.collision = collision;
        }
        if let Some(animation) = animation {
            tile.animation = animation;
        }
        if tile.animation.is_empty() {
            let frames = mana_world_animation(&tile.properties);
            if !frames.is_empty() {
                tile.animation = frames;
            }
        }
        Some(id)
    }

    /// A tile's collision group. Properties on the group (before Tiled 1.1) move onto the tile,
    /// the tile's own winning; layer attributes the model does not keep are reported.
    fn collision_of(
        &mut self,
        raw: RawLayer,
        tile_properties: &mut Properties,
        tileset: &str,
        tile: TileId,
    ) -> CollisionGroup {
        let target = Some(Target::Tile {
            tileset: tileset.to_owned(),
            tile,
        });
        let group_properties = self.properties_of(raw.properties.clone());
        if !group_properties.is_empty() {
            self.warnings.record_about(
                WarningKind::LegacyShape,
                target.clone(),
                format!(
                    "the collision group of tile {tile} has properties; they move onto the tile"
                ),
            );
            for (name, property) in group_properties {
                tile_properties.entry(name).or_insert(property);
            }
        }
        let unkept = [
            (
                "name",
                raw.name.as_ref().filter(|v| !v.is_empty_text()).is_some(),
            ),
            (
                "class",
                raw.class.as_ref().filter(|v| !v.is_empty_text()).is_some(),
            ),
            (
                "visible",
                raw.visible
                    .as_ref()
                    .is_some_and(|v| v.to_bool() == Some(false)),
            ),
            (
                "locked",
                raw.locked
                    .as_ref()
                    .is_some_and(|v| v.to_bool() == Some(true)),
            ),
            (
                "opacity",
                raw.opacity
                    .as_ref()
                    .is_some_and(|v| v.to_real().is_some_and(|o| o != 1.0)),
            ),
            ("tintcolor", raw.tint_color.is_some()),
            (
                "offsetx",
                raw.offset_x
                    .as_ref()
                    .is_some_and(|v| v.to_real().is_some_and(|o| o != 0.0)),
            ),
            (
                "offsety",
                raw.offset_y
                    .as_ref()
                    .is_some_and(|v| v.to_real().is_some_and(|o| o != 0.0)),
            ),
            (
                "parallaxx",
                raw.parallax_x
                    .as_ref()
                    .is_some_and(|v| v.to_real().is_some_and(|o| o != 1.0)),
            ),
            (
                "parallaxy",
                raw.parallax_y
                    .as_ref()
                    .is_some_and(|v| v.to_real().is_some_and(|o| o != 1.0)),
            ),
            (
                "mode",
                raw.mode
                    .as_ref()
                    .filter(|v| !v.is_empty_text())
                    .is_some_and(|v| v.to_text() != "normal"),
            ),
        ];
        for (attribute, stated) in unkept {
            if stated {
                self.warnings.record_about(
                    WarningKind::UnknownKey,
                    target.clone(),
                    format!("the collision group of tile {tile} has a {attribute}; it is not kept"),
                );
            }
        }
        let id = LayerId(
            self.u32_of(
                &raw.id,
                format_args!("the collision group id of tile {tile}"),
            )
            .unwrap_or(0),
        );
        let color = self.color_of(
            &raw.color,
            format_args!("the collision group colour of tile {tile}"),
        );
        let draw_order = self
            .named_of(
                &raw.draw_order,
                format_args!("the collision group draw order of tile {tile}"),
                names::draw_order_from_name,
            )
            .unwrap_or_default();
        let objects = self.collision_objects_of(raw.objects, tileset, tile);
        CollisionGroup {
            id,
            layer: ObjectLayer {
                color,
                draw_order,
                objects: objects.into(),
            },
        }
    }
}

/// The Mana World's animation properties (`animation-frameN`, `animation-delayN` in tens of
/// milliseconds), which Tiled still turns into frames; the properties stay.
fn mana_world_animation(properties: &Properties) -> Vec<Frame> {
    let int = |name: String| {
        properties
            .get(&name)
            .and_then(|property| match &property.value {
                Value::Int(value) => Some(*value),
                Value::Float(value) => crate::numbers::integer_of_real(*value),
                Value::String(text) => crate::numbers::parse_int(text),
                _ => None,
            })
    };
    let mut frames = Vec::new();
    for i in 0.. {
        match (
            int(format!("animation-frame{i}")),
            int(format!("animation-delay{i}")),
        ) {
            (Some(tile), Some(delay)) => frames.push(Frame {
                tile: TileId(u32::try_from(tile).unwrap_or(0)),
                duration_ms: u32::try_from(delay.saturating_mul(10)).unwrap_or(0),
            }),
            _ => break,
        }
    }
    frames
}
