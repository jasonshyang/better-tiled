//! Layers back into the raw tree, cells as global ids.

use bt_core::{ChunkPos, Layer, LayerDataFormat, LayerKind, Map, ObjectLayer, TileLayer};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use super::map::Unfinisher;
use super::object::Place;
use super::property::raw_properties;
use crate::names;
use crate::raw::{RawChunk, RawData, RawLayer, RawLayerKind, RawPayload, Scalar};

impl Unfinisher<'_> {
    /// A map's (or group's) layers.
    pub(crate) fn layers_of(&mut self, layers: &[Layer], map: &Map) -> Vec<RawLayer> {
        layers
            .iter()
            .map(|layer| self.layer_of(layer, map))
            .collect()
    }

    fn layer_of(&mut self, layer: &Layer, map: &Map) -> RawLayer {
        let mut raw = RawLayer {
            id: Some(Scalar::int(layer.id.0)),
            name: Some(Scalar::text(&layer.name)),
            class: Some(Scalar::text(&layer.class)),
            x: Some(Scalar::Int(0)),
            y: Some(Scalar::Int(0)),
            visible: Some(Scalar::Bool(layer.visible)),
            locked: Some(Scalar::Bool(layer.locked)),
            opacity: Some(Scalar::Real(layer.opacity)),
            tint_color: layer.tint.map(|c| Scalar::Text(c.to_tiled_text())),
            offset_x: Some(Scalar::Real(layer.offset.x)),
            offset_y: Some(Scalar::Real(layer.offset.y)),
            parallax_x: Some(Scalar::Real(layer.parallax.x)),
            parallax_y: Some(Scalar::Real(layer.parallax.y)),
            mode: Some(Scalar::text(names::blend_mode_name(layer.blend_mode))),
            properties: raw_properties(&layer.properties),
            ..RawLayer::default()
        };
        match &layer.kind {
            LayerKind::Tile(tiles) => {
                raw.kind = RawLayerKind::Tile;
                let size = tiles.size();
                raw.width = Some(Scalar::int(size.x));
                raw.height = Some(Scalar::int(size.y));
                raw.data = Some(self.data_of(tiles, map));
            }
            LayerKind::Object(objects) => {
                raw.kind = RawLayerKind::Object;
                self.object_layer_into(objects, &mut raw, Some(map));
            }
            LayerKind::Image(image_layer) => {
                raw.kind = RawLayerKind::Image;
                raw.image = image_layer
                    .image
                    .as_ref()
                    .map(|image| self.image_of(image, true));
                raw.repeat_x = Some(Scalar::Bool(image_layer.repeat_x));
                raw.repeat_y = Some(Scalar::Bool(image_layer.repeat_y));
            }
            LayerKind::Group(children) => {
                raw.kind = RawLayerKind::Group;
                raw.layers = self.layers_of(children, map);
            }
        }
        raw
    }

    /// An object layer's colour, draw order and objects (also a tile's collision group, which
    /// has no map).
    pub(crate) fn object_layer_into(
        &mut self,
        objects: &ObjectLayer,
        raw: &mut RawLayer,
        map: Option<&Map>,
    ) {
        raw.color = objects.color.map(|c| Scalar::Text(c.to_tiled_text()));
        raw.draw_order = Some(Scalar::text(names::draw_order_name(objects.draw_order)));
        raw.name.get_or_insert(Scalar::text(""));
        raw.visible.get_or_insert(Scalar::Bool(true));
        raw.opacity.get_or_insert(Scalar::Real(1.0));
        raw.x.get_or_insert(Scalar::Int(0));
        raw.y.get_or_insert(Scalar::Int(0));
        raw.objects = objects
            .objects
            .iter()
            .map(|object| {
                let place = match map {
                    Some(map) => Place::Map(map),
                    None => Place::Collision,
                };
                self.object_of(object, place)
            })
            .collect();
    }

    /// A tile layer's cells: all of a finite layer, or the chunks of the map's chunk size that
    /// hold a tile, by row then column.
    fn data_of(&mut self, tiles: &TileLayer, map: &Map) -> RawData {
        // `<tile>` elements are read, never written: such maps are written as CSV.
        let format = match map.file.layer_data {
            LayerDataFormat::Xml => LayerDataFormat::Csv,
            format => format,
        };
        let mut data = RawData {
            encoding: names::encoding_name(format).map(Scalar::text),
            compression: names::compression_name(format).map(Scalar::text),
            ..RawData::default()
        };
        if map.infinite {
            let chunk_size = map.file.output_chunk_size.max(UVec2::ONE).as_ivec2();
            for rect in chunks_to_write(tiles, chunk_size) {
                data.chunks.push(RawChunk {
                    x: Some(Scalar::int(rect.x)),
                    y: Some(Scalar::int(rect.y)),
                    width: Some(Scalar::int(rect.w)),
                    height: Some(Scalar::int(rect.h)),
                    payload: RawPayload::Gids(self.gids_in(tiles, rect)),
                });
            }
            let bounds = tiles.chunk_bounds().unwrap_or_default();
            data.chunk_bounds = Some(RawChunk {
                x: Some(Scalar::int(bounds.x)),
                y: Some(Scalar::int(bounds.y)),
                width: Some(Scalar::int(bounds.w)),
                height: Some(Scalar::int(bounds.h)),
                payload: RawPayload::Empty,
            });
        } else {
            let rect = IRect::from_min_size(IVec2::ZERO, tiles.size().as_ivec2());
            data.payload = RawPayload::Gids(self.gids_in(tiles, rect));
        }
        data
    }

    /// The global ids of the cells in `rect`, row by row.
    fn gids_in(&mut self, tiles: &TileLayer, rect: IRect) -> Vec<u32> {
        rect.points()
            .map(|p| self.gids.gid_of(tiles.cell(p)))
            .collect()
    }
}

/// The chunks Tiled writes for an infinite layer (`TileLayer::sortedChunksToWrite`): aligned to
/// `chunk_size` with floor division, only those with a tile, by row then column.
fn chunks_to_write(tiles: &TileLayer, chunk_size: IVec2) -> Vec<IRect> {
    let mut starts: Vec<IVec2> = tiles
        .cells()
        .map(|placed| {
            IVec2::new(
                placed.at.x.div_euclid(chunk_size.x) * chunk_size.x,
                placed.at.y.div_euclid(chunk_size.y) * chunk_size.y,
            )
        })
        .collect();
    starts.sort_by_key(|start| (start.y, start.x));
    starts.dedup();
    let _ = ChunkPos::of;
    starts
        .into_iter()
        .map(|start| IRect::from_min_size(start, chunk_size))
        .collect()
}
