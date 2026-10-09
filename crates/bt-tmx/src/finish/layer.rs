//! Layers: their shared attributes, and tile layers' cells.

use bt_core::{
    BlendMode, ImageLayer, Layer, LayerDataFormat, LayerId, LayerKind, ObjectLayer, TileLayer,
};
use bt_utils::IRect;
use glam::{DVec2, IVec2, UVec2};

use super::context::{Finisher, text_of};
use super::map::MapFinish;
use super::object::{ObjectPlace, UnknownGids};
use crate::data::{DecodedGids, checked_length, decode_base64, decode_csv};
use crate::gid::GidError;
use crate::names;
use crate::raw::{RawLayer, RawLayerKind, RawPayload};
use crate::warning::{Target, WarningKind};

impl Finisher<'_> {
    /// A map's (or group's) layers, bottom first.
    pub(crate) fn layers_of(&mut self, raw: Vec<RawLayer>, map: &mut MapFinish) -> Vec<Layer> {
        raw.into_iter()
            .map(|layer| self.layer_of(layer, map))
            .collect()
    }

    fn layer_of(&mut self, raw: RawLayer, map: &mut MapFinish) -> Layer {
        let name = text_of(&raw.name);
        let id = LayerId(
            self.u32_of(&raw.id, format_args!("layer “{name}”'s id"))
                .unwrap_or(0),
        );
        let target = Some(Target::Layer(id));
        let what = format!("layer “{name}”");
        let position = DVec2::new(
            self.real_of(&raw.x, format_args!("{what}'s x"))
                .unwrap_or(0.0),
            self.real_of(&raw.y, format_args!("{what}'s y"))
                .unwrap_or(0.0),
        );
        let offset_stated = raw.offset_x.is_some();
        let mut layer = Layer::new(id, name, LayerKind::Group(Vec::new()));
        layer.class = text_of(&raw.class);
        layer.visible = self
            .bool_of(&raw.visible, format_args!("{what}'s visibility"))
            .unwrap_or(true);
        layer.locked = self
            .bool_of(&raw.locked, format_args!("{what}'s lock"))
            .unwrap_or(false);
        layer.opacity = self
            .real_of(&raw.opacity, format_args!("{what}'s opacity"))
            .unwrap_or(1.0);
        layer.tint = self.color_of(&raw.tint_color, format_args!("{what}'s tint colour"));
        layer.offset = DVec2::new(
            self.real_of(&raw.offset_x, format_args!("{what}'s offset"))
                .unwrap_or(0.0),
            self.real_of(&raw.offset_y, format_args!("{what}'s offset"))
                .unwrap_or(0.0),
        );
        layer.parallax = DVec2::new(
            self.real_of(&raw.parallax_x, format_args!("{what}'s parallax"))
                .unwrap_or(1.0),
            self.real_of(&raw.parallax_y, format_args!("{what}'s parallax"))
                .unwrap_or(1.0),
        );
        layer.blend_mode = self
            .named_of(
                &raw.mode,
                format_args!("{what}'s blend mode"),
                names::blend_mode_from_name,
            )
            .unwrap_or(BlendMode::Normal);
        layer.properties = self.properties_of(raw.properties.clone());
        layer.kind = match raw.kind {
            RawLayerKind::Tile => LayerKind::Tile(self.tile_layer_of(&raw, &what, id, map)),
            RawLayerKind::Object => {
                let mut unknown = UnknownGids::default();
                let objects = raw
                    .objects
                    .into_iter()
                    .map(|object| {
                        self.object_of(object, &mut ObjectPlace::Map(&mut map.space), &mut unknown)
                    })
                    .collect::<Vec<_>>();
                self.record_unknown_gids(&unknown, &what, target.clone());
                LayerKind::Object(ObjectLayer {
                    color: self.color_of(&raw.color, format_args!("{what}'s colour")),
                    draw_order: self
                        .named_of(
                            &raw.draw_order,
                            format_args!("{what}'s draw order"),
                            names::draw_order_from_name,
                        )
                        .unwrap_or_default(),
                    objects: objects.into(),
                })
            }
            RawLayerKind::Image => {
                if !offset_stated && position != DVec2::ZERO {
                    // Image layers had a position before Tiled 0.15; it is their offset.
                    layer.offset = position;
                    self.warnings.record_about(
                        WarningKind::LegacyShape,
                        target.clone(),
                        format!("{what}'s position becomes its offset"),
                    );
                }
                LayerKind::Image(ImageLayer {
                    image: raw
                        .image
                        .and_then(|image| {
                            self.image_of(image, true, format_args!("{what}'s image"))
                        })
                        .map(|finished| finished.image),
                    repeat_x: self
                        .bool_of(&raw.repeat_x, format_args!("{what}'s horizontal repeat"))
                        .unwrap_or(false),
                    repeat_y: self
                        .bool_of(&raw.repeat_y, format_args!("{what}'s vertical repeat"))
                        .unwrap_or(false),
                })
            }
            RawLayerKind::Group => LayerKind::Group(self.layers_of(raw.layers, map)),
        };
        if raw.kind != RawLayerKind::Image && position != DVec2::ZERO {
            self.warnings.record_about(
                WarningKind::UnknownKey,
                target,
                format!("{what} has a position, which Tiled never writes; it is not kept"),
            );
        }
        layer
    }

    fn record_unknown_gids(&mut self, unknown: &UnknownGids, what: &str, target: Option<Target>) {
        if unknown.count > 0 {
            self.warnings.record_about(
                WarningKind::UnknownGid,
                target,
                format!(
                    "{} tiles of {what} have global ids no tileset owns (the first is {}); they are left empty",
                    unknown.count, unknown.first
                ),
            );
        }
    }

    /// A tile layer's cells: the data of a finite layer over its size, the chunks of an infinite
    /// one (or of a finite one, keeping what lies inside).
    fn tile_layer_of(
        &mut self,
        raw: &RawLayer,
        what: &str,
        id: LayerId,
        map: &mut MapFinish,
    ) -> TileLayer {
        let target = Some(Target::Layer(id));
        let mut size = UVec2::new(
            self.u32_of(&raw.width, format_args!("{what}'s width"))
                .unwrap_or(0),
            self.u32_of(&raw.height, format_args!("{what}'s height"))
                .unwrap_or(0),
        );
        if !map.infinite && (size.x == 0 || size.y == 0) {
            if raw.data.is_some() && map.size != size {
                self.warnings.record_about(
                    WarningKind::InvalidValue,
                    target.clone(),
                    format!("{what} has no size; it takes the map's"),
                );
            }
            size = map.size;
        }
        let mut layer = if map.infinite {
            TileLayer::infinite(size)
        } else {
            TileLayer::finite(size)
        };
        let Some(data) = &raw.data else {
            return layer;
        };
        let encoding = text_of(&data.encoding);
        let compression = text_of(&data.compression);
        let Some(format) = names::layer_data_format_from_names(&encoding, &compression) else {
            self.warnings.record_about(
                WarningKind::CorruptLayerData,
                target,
                format!("{what}'s data is in the encoding “{encoding}” with the compression “{compression}”, which Tiled does not write; its cells are not read"),
            );
            return layer;
        };
        map.layer_data = Some(format);
        let mut placed = Placement::default();
        let start = IVec2::new(
            self.i32_of(&raw.start_x, format_args!("{what}'s start"))
                .unwrap_or(0),
            self.i32_of(&raw.start_y, format_args!("{what}'s start"))
                .unwrap_or(0),
        );
        if !matches!(data.payload, RawPayload::Empty) {
            let bounds = IRect::from_min_size(
                if map.infinite { start } else { IVec2::ZERO },
                size.as_ivec2(),
            );
            self.place_cells(
                &data.payload,
                format,
                bounds,
                &mut layer,
                map,
                &mut placed,
                what,
                target.clone(),
            );
        }
        for chunk in &data.chunks {
            let bounds = IRect {
                x: self
                    .i32_of(&chunk.x, format_args!("a chunk of {what}"))
                    .unwrap_or(0),
                y: self
                    .i32_of(&chunk.y, format_args!("a chunk of {what}"))
                    .unwrap_or(0),
                w: self
                    .i32_of(&chunk.width, format_args!("a chunk of {what}"))
                    .unwrap_or(0),
                h: self
                    .i32_of(&chunk.height, format_args!("a chunk of {what}"))
                    .unwrap_or(0),
            };
            self.place_cells(
                &chunk.payload,
                format,
                bounds,
                &mut layer,
                map,
                &mut placed,
                what,
                target.clone(),
            );
        }
        self.record_unknown_gids(&placed.unknown, what, target.clone());
        if placed.outside > 0 {
            self.warnings.record_about(
                WarningKind::CorruptLayerData,
                target,
                format!(
                    "{} cells of {what} lie outside it; they are dropped",
                    placed.outside
                ),
            );
        }
        let _ = data_format_name(format);
        layer
    }

    /// Decodes a payload covering `bounds` and sets its cells.
    #[allow(clippy::too_many_arguments)]
    fn place_cells(
        &mut self,
        payload: &RawPayload,
        format: LayerDataFormat,
        bounds: IRect,
        layer: &mut TileLayer,
        map: &mut MapFinish,
        placed: &mut Placement,
        what: &str,
        target: Option<Target>,
    ) {
        let area = usize::try_from(bounds.w.max(0)).unwrap_or(0)
            * usize::try_from(bounds.h.max(0)).unwrap_or(0);
        let decoded = match payload {
            RawPayload::Empty => DecodedGids::default(),
            RawPayload::Gids(gids) => checked_length(
                DecodedGids {
                    gids: gids.clone(),
                    problem: None,
                },
                area,
            ),
            RawPayload::Text(text) => match format {
                LayerDataFormat::Csv | LayerDataFormat::Xml => decode_csv(text, area),
                _ => decode_base64(text, format, area),
            },
        };
        if let Some(problem) = decoded.problem
            && !matches!(payload, RawPayload::Empty)
        {
            let width = bounds.w.max(1);
            let at = i32::try_from(problem.at).unwrap_or(i32::MAX);
            let cell = IVec2::new(bounds.x + at % width, bounds.y + at / width);
            self.warnings.record_about(
                WarningKind::CorruptLayerData,
                target,
                format!(
                    "{what}: {} (at cell {}, {})",
                    problem.message, cell.x, cell.y
                ),
            );
        }
        let width = bounds.w.max(1);
        for (index, gid) in (0i32..).zip(decoded.gids) {
            let position = IVec2::new(bounds.x + index % width, bounds.y + index / width);
            let cell = match map.space.cell_of(gid) {
                Ok(cell) => cell,
                Err(GidError::NoTilesets | GidError::BelowFirstTileset) => {
                    placed.unknown.record_gid(gid);
                    continue;
                }
            };
            if cell.is_empty() {
                continue;
            }
            if layer.set_cell(position, cell).is_err() {
                placed.outside += 1;
            }
        }
    }
}

/// What placing a layer's cells found.
#[derive(Default)]
struct Placement {
    /// Global ids no tileset owns.
    unknown: UnknownGids,
    /// Cells outside a finite layer.
    outside: usize,
}

/// The name of a layer data format, for messages.
fn data_format_name(format: LayerDataFormat) -> &'static str {
    match format {
        LayerDataFormat::Xml => "XML",
        LayerDataFormat::Csv => "CSV",
        LayerDataFormat::Base64 => "base64",
        LayerDataFormat::Base64Zlib => "zlib",
        LayerDataFormat::Base64Gzip => "gzip",
        LayerDataFormat::Base64Zstd => "zstd",
    }
}
