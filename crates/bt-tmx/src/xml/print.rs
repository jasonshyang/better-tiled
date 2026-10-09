//! The raw tree laid out as Tiled's `MapWriter` writes it (`libtiled/mapwriter.cpp`):
//! attribute and element order, what is omitted when it has its default, numbers in Qt's
//! formats, layer data in Tiled's layouts.

use bt_core::LayerDataFormat;

use super::writer::XmlWriter;
use crate::data::{encode_base64, encode_csv};
use crate::names;
use crate::numbers::{real_g6, real_shortest};
use crate::raw::{
    RawData, RawImage, RawLayer, RawLayerKind, RawMap, RawObject, RawPayload, RawProperty,
    RawShape, RawTemplate, RawText, RawTile, RawTileset, RawValue, RawWangId, RawWangSet, Scalar,
};

/// Prints raw trees as XML.
pub(crate) struct XmlPrinter {
    w: XmlWriter,
    minimize: bool,
    /// The map's layer data format.
    format: LayerDataFormat,
    /// The map's compression level.
    level: Option<i32>,
}

impl XmlPrinter {
    /// A printer; `minimize` drops all line breaks and indentation.
    pub(crate) fn new(minimize: bool) -> XmlPrinter {
        XmlPrinter {
            w: XmlWriter::new(minimize),
            minimize,
            format: LayerDataFormat::Csv,
            level: None,
        }
    }

    /// A map (`.tmx`).
    pub(crate) fn print_map(mut self, map: &RawMap) -> String {
        let w = &mut self.w;
        w.start_element("map");
        attribute(w, "version", &map.version);
        attribute(w, "tiledversion", &map.tiled_version);
        non_empty(w, "class", &map.class);
        attribute(w, "orientation", &map.orientation);
        attribute(w, "renderorder", &map.render_order);
        attribute(w, "compressionlevel", &map.compression_level);
        attribute(w, "width", &map.width);
        attribute(w, "height", &map.height);
        attribute(w, "tilewidth", &map.tile_width);
        attribute(w, "tileheight", &map.tile_height);
        attribute(w, "infinite", &map.infinite);
        let orientation = text(&map.orientation);
        if orientation == "hexagonal" {
            attribute(w, "hexsidelength", &map.hex_side_length);
        }
        if orientation == "staggered" || orientation == "hexagonal" {
            attribute(w, "staggeraxis", &map.stagger_axis);
            attribute(w, "staggerindex", &map.stagger_index);
        }
        unless_int(w, "skewx", &map.skew_x, 0);
        unless_int(w, "skewy", &map.skew_y, 0);
        if real(&map.parallax_origin_x) != Some(0.0) || real(&map.parallax_origin_y) != Some(0.0) {
            attribute(w, "parallaxoriginx", &map.parallax_origin_x);
            attribute(w, "parallaxoriginy", &map.parallax_origin_y);
        }
        attribute(w, "backgroundcolor", &map.background_color);
        attribute(w, "nextlayerid", &map.next_layer_id);
        attribute(w, "nextobjectid", &map.next_object_id);
        if map.chunk_size.is_some() || map.export.is_some() {
            w.start_element("editorsettings");
            if let Some(size) = &map.chunk_size {
                w.start_element("chunksize");
                attribute(w, "width", &size.width);
                attribute(w, "height", &size.height);
                w.end_element();
            }
            if let Some(export) = &map.export {
                w.start_element("export");
                non_empty(w, "target", &export.target);
                non_empty(w, "format", &export.format);
                w.end_element();
            }
            w.end_element();
        }
        self.print_properties(&map.properties);
        for tileset in &map.tilesets {
            self.print_tileset(tileset);
        }
        let data = first_data(&map.layers);
        self.format = data
            .and_then(|d| {
                names::layer_data_format_from_names(&text(&d.encoding), &text(&d.compression))
            })
            .unwrap_or(LayerDataFormat::Csv);
        self.level = map
            .compression_level
            .as_ref()
            .and_then(Scalar::to_int)
            .and_then(|level| i32::try_from(level).ok());
        self.print_layers(&map.layers);
        self.w.end_element();
        self.w.finish()
    }

    /// A tileset file (`.tsx`).
    pub(crate) fn print_tileset_file(mut self, tileset: &RawTileset) -> String {
        self.print_tileset(tileset);
        self.w.finish()
    }

    /// A template file (`.tx`).
    pub(crate) fn print_template(mut self, template: &RawTemplate) -> String {
        self.w.start_element("template");
        for tileset in &template.tilesets {
            self.print_tileset(tileset);
        }
        if let Some(object) = &template.object {
            self.print_object(object);
        }
        self.w.end_element();
        self.w.finish()
    }

    /// A `<tileset>`: a reference, an embedded tileset or a tileset file's root.
    fn print_tileset(&mut self, tileset: &RawTileset) {
        let w = &mut self.w;
        w.start_element("tileset");
        if tileset.first_gid.is_some() {
            attribute(w, "firstgid", &tileset.first_gid);
            if tileset.source.is_some() {
                attribute(w, "source", &tileset.source);
                w.end_element();
                return;
            }
        } else {
            attribute(w, "version", &tileset.version);
            attribute(w, "tiledversion", &tileset.tiled_version);
        }
        attribute(w, "name", &tileset.name);
        non_empty(w, "class", &tileset.class);
        attribute(w, "tilewidth", &tileset.tile_width);
        attribute(w, "tileheight", &tileset.tile_height);
        unless_int(w, "spacing", &tileset.spacing, 0);
        unless_int(w, "margin", &tileset.margin, 0);
        attribute(w, "tilecount", &tileset.tile_count);
        attribute(w, "columns", &tileset.columns);
        attribute(w, "backgroundcolor", &tileset.background_color);
        unless_text(
            w,
            "objectalignment",
            &tileset.object_alignment,
            "unspecified",
        );
        unless_text(w, "tilerendersize", &tileset.tile_render_size, "tile");
        unless_text(w, "fillmode", &tileset.fill_mode, "stretch");
        if let (None, Some(export)) = (&tileset.first_gid, &tileset.export) {
            w.start_element("editorsettings");
            w.start_element("export");
            attribute(w, "target", &export.target);
            attribute(w, "format", &export.format);
            w.end_element();
            w.end_element();
        }
        if let Some(offset) = &tileset.tile_offset {
            w.start_element("tileoffset");
            attribute(w, "x", &offset.x);
            attribute(w, "y", &offset.y);
            w.end_element();
        }
        if let Some(grid) = &tileset.grid {
            w.start_element("grid");
            attribute(w, "orientation", &grid.orientation);
            attribute(w, "width", &grid.width);
            attribute(w, "height", &grid.height);
            w.end_element();
        }
        if let Some(flags) = &tileset.transformations {
            w.start_element("transformations");
            attribute(w, "hflip", &flags.hflip);
            attribute(w, "vflip", &flags.vflip);
            attribute(w, "rotate", &flags.rotate);
            attribute(w, "preferuntransformed", &flags.prefer_untransformed);
            w.end_element();
        }
        self.print_properties(&tileset.properties);
        if let Some(image) = &tileset.image {
            self.print_image(image);
        }
        for tile in &tileset.tiles {
            if tileset.lists_every_tile || tile_has_data(tile) {
                self.print_tile(tile);
            }
        }
        if !tileset.wang_sets.is_empty() {
            self.w.start_element("wangsets");
            for set in &tileset.wang_sets {
                self.print_wang_set(set);
            }
            self.w.end_element();
        }
        self.w.end_element();
    }

    fn print_tile(&mut self, tile: &RawTile) {
        let w = &mut self.w;
        w.start_element("tile");
        attribute(w, "id", &tile.id);
        attribute(w, "x", &tile.x);
        attribute(w, "y", &tile.y);
        attribute(w, "width", &tile.width);
        attribute(w, "height", &tile.height);
        non_empty(w, "class", &tile.class);
        non_empty(w, "type", &tile.class_as_type);
        if real(&tile.probability).is_some_and(|p| p != 1.0) {
            attribute(w, "probability", &tile.probability);
        }
        self.print_properties(&tile.properties);
        if let Some(image) = &tile.image {
            self.print_image(image);
        }
        if let Some(group) = &tile.collision {
            self.print_object_group(group);
        }
        if let Some(frames) = &tile.animation {
            self.w.start_element("animation");
            for frame in frames {
                self.w.start_element("frame");
                attribute(&mut self.w, "tileid", &frame.tile_id);
                attribute(&mut self.w, "duration", &frame.duration);
                self.w.end_element();
            }
            self.w.end_element();
        }
        self.w.end_element();
    }

    fn print_wang_set(&mut self, set: &RawWangSet) {
        let w = &mut self.w;
        w.start_element("wangset");
        attribute(w, "name", &set.name);
        non_empty(w, "class", &set.class);
        attribute(w, "type", &set.kind);
        attribute(w, "tile", &set.tile);
        for color in &set.colors {
            self.w.start_element("wangcolor");
            attribute(&mut self.w, "name", &color.name);
            non_empty(&mut self.w, "class", &color.class);
            attribute(&mut self.w, "color", &color.color);
            attribute(&mut self.w, "tile", &color.tile);
            attribute(&mut self.w, "probability", &color.probability);
            self.print_properties(&color.properties);
            self.w.end_element();
        }
        for tile in &set.tiles {
            self.w.start_element("wangtile");
            attribute(&mut self.w, "tileid", &tile.tile_id);
            if let Some(id) = &tile.wang_id {
                let written = match id {
                    RawWangId::Lanes(lanes) => {
                        lanes.iter().map(xml_value).collect::<Vec<_>>().join(",")
                    }
                    RawWangId::Text(text) => text.clone(),
                    RawWangId::Packed(value) => value.to_string(),
                };
                self.w.attribute("wangid", &written);
            }
            self.w.end_element();
        }
        self.print_properties(&set.properties);
        self.w.end_element();
    }

    /// An `<image>`: a file reference, or bytes embedded as base64.
    fn print_image(&mut self, image: &RawImage) {
        let w = &mut self.w;
        if image.source.as_ref().is_none_or(Scalar::is_empty_text) && image.data.is_none() {
            return;
        }
        w.start_element("image");
        non_empty(w, "source", &image.source);
        if let Some(trans) = &image.trans {
            let text = trans.to_text();
            w.attribute("trans", text.strip_prefix('#').unwrap_or(&text));
        }
        unless_int(w, "width", &image.width, 0);
        unless_int(w, "height", &image.height, 0);
        if let Some(data) = &image.data {
            attribute(w, "format", &image.format);
            w.start_element("data");
            attribute(w, "encoding", &data.encoding);
            w.characters(&data.text);
            w.end_element();
        }
        w.end_element();
    }

    fn print_layers(&mut self, layers: &[RawLayer]) {
        for layer in layers {
            match layer.kind {
                RawLayerKind::Tile => {
                    self.w.start_element("layer");
                    self.print_layer_attributes(layer);
                    self.print_properties(&layer.properties);
                    if let Some(data) = &layer.data {
                        self.print_data(data, layer);
                    }
                    self.w.end_element();
                }
                RawLayerKind::Object => self.print_object_group(layer),
                RawLayerKind::Image => {
                    self.w.start_element("imagelayer");
                    self.print_layer_attributes(layer);
                    if bool_of(&layer.repeat_x) {
                        self.w.attribute("repeatx", "1");
                    }
                    if bool_of(&layer.repeat_y) {
                        self.w.attribute("repeaty", "1");
                    }
                    if let Some(image) = &layer.image {
                        self.print_image(image);
                    }
                    self.print_properties(&layer.properties);
                    self.w.end_element();
                }
                RawLayerKind::Group => {
                    self.w.start_element("group");
                    self.print_layer_attributes(layer);
                    self.print_properties(&layer.properties);
                    self.print_layers(&layer.layers);
                    self.w.end_element();
                }
            }
        }
    }

    fn print_object_group(&mut self, layer: &RawLayer) {
        self.w.start_element("objectgroup");
        attribute(&mut self.w, "color", &layer.color);
        unless_text(&mut self.w, "draworder", &layer.draw_order, "topdown");
        self.print_layer_attributes(layer);
        self.print_properties(&layer.properties);
        for object in &layer.objects {
            self.print_object(object);
        }
        self.w.end_element();
    }

    /// `MapWriter::writeLayerAttributes`.
    fn print_layer_attributes(&mut self, layer: &RawLayer) {
        let w = &mut self.w;
        unless_int(w, "id", &layer.id, 0);
        non_empty(w, "name", &layer.name);
        non_empty(w, "class", &layer.class);
        unless_int(w, "x", &layer.x, 0);
        unless_int(w, "y", &layer.y, 0);
        if layer.kind == RawLayerKind::Tile {
            attribute(w, "width", &layer.width);
            attribute(w, "height", &layer.height);
        }
        if layer.visible.as_ref().and_then(Scalar::to_bool) == Some(false) {
            w.attribute("visible", "0");
        }
        if bool_of(&layer.locked) {
            w.attribute("locked", "1");
        }
        if real(&layer.opacity).is_some_and(|o| o != 1.0) {
            attribute(w, "opacity", &layer.opacity);
        }
        attribute(w, "tintcolor", &layer.tint_color);
        if real(&layer.offset_x).is_some_and(|o| o != 0.0)
            || real(&layer.offset_y).is_some_and(|o| o != 0.0)
        {
            attribute(w, "offsetx", &layer.offset_x);
            attribute(w, "offsety", &layer.offset_y);
        }
        if real(&layer.parallax_x).is_some_and(|p| p != 1.0) {
            attribute(w, "parallaxx", &layer.parallax_x);
        }
        if real(&layer.parallax_y).is_some_and(|p| p != 1.0) {
            attribute(w, "parallaxy", &layer.parallax_y);
        }
        unless_text(w, "mode", &layer.mode, "normal");
    }

    /// A tile layer's `<data>`, finite or in chunks.
    fn print_data(&mut self, data: &RawData, layer: &RawLayer) {
        let w = &mut self.w;
        w.start_element("data");
        non_empty(w, "encoding", &data.encoding);
        non_empty(w, "compression", &data.compression);
        let format =
            names::layer_data_format_from_names(&text(&data.encoding), &text(&data.compression))
                .unwrap_or(self.format);
        if data.chunks.is_empty() {
            let width = layer.width.as_ref().and_then(Scalar::to_int).unwrap_or(1);
            self.print_cells(&data.payload, format, width);
        }
        for chunk in &data.chunks {
            self.w.start_element("chunk");
            attribute(&mut self.w, "x", &chunk.x);
            attribute(&mut self.w, "y", &chunk.y);
            attribute(&mut self.w, "width", &chunk.width);
            attribute(&mut self.w, "height", &chunk.height);
            let width = chunk.width.as_ref().and_then(Scalar::to_int).unwrap_or(1);
            self.print_cells(&chunk.payload, format, width);
            self.w.end_element();
        }
        self.w.end_element();
    }

    /// Cells in Tiled's layouts: CSV rows, or base64 between fixed whitespace.
    fn print_cells(&mut self, payload: &RawPayload, format: LayerDataFormat, width: i64) {
        let gids = match payload {
            RawPayload::Gids(gids) => gids,
            RawPayload::Empty | RawPayload::Text(_) => return,
        };
        match format {
            LayerDataFormat::Csv | LayerDataFormat::Xml => {
                let width = usize::try_from(width).unwrap_or(1);
                self.w.characters(&encode_csv(gids, width, self.minimize));
            }
            _ => {
                let encoded = encode_base64(gids, format, self.level);
                if self.minimize {
                    self.w.characters(&encoded);
                } else {
                    self.w.characters("\n   ");
                    self.w.characters(&encoded);
                    self.w.characters("\n  ");
                }
            }
        }
    }

    /// An `<object>`: a template instance writes what it states, other objects what is not
    /// default (`MapWriter::writeObject`).
    fn print_object(&mut self, object: &RawObject) {
        let instance = object.template.is_some();
        let w = &mut self.w;
        w.start_element("object");
        attribute(w, "id", &object.id);
        attribute(w, "template", &object.template);
        if instance {
            attribute(w, "name", &object.name);
        } else {
            non_empty(w, "name", &object.name);
        }
        non_empty(w, "class", &object.class);
        non_empty(w, "type", &object.class_as_type);
        attribute(w, "gid", &object.gid);
        attribute(w, "x", &object.x);
        attribute(w, "y", &object.y);
        unless_real(w, "width", &object.width, 0.0);
        unless_real(w, "height", &object.height, 0.0);
        if instance {
            attribute(w, "rotation", &object.rotation);
            attribute(w, "opacity", &object.opacity);
            if let Some(visible) = &object.visible {
                w.attribute(
                    "visible",
                    if visible.to_bool() == Some(false) {
                        "0"
                    } else {
                        "1"
                    },
                );
            }
        } else {
            unless_real(w, "rotation", &object.rotation, 0.0);
            unless_real(w, "opacity", &object.opacity, 1.0);
            if object.visible.as_ref().and_then(Scalar::to_bool) == Some(false) {
                w.attribute("visible", "0");
            }
        }
        self.print_properties(&object.properties);
        match &object.shape {
            None => {}
            Some(RawShape::Ellipse) => self.w.empty_element("ellipse"),
            Some(RawShape::Capsule) => self.w.empty_element("capsule"),
            Some(RawShape::Point) => self.w.empty_element("point"),
            Some(RawShape::Polygon(points) | RawShape::Polyline(points)) => {
                let name = if matches!(object.shape, Some(RawShape::Polygon(_))) {
                    "polygon"
                } else {
                    "polyline"
                };
                let written: Vec<String> = points
                    .iter()
                    .map(|p| {
                        format!(
                            "{},{}",
                            p.x.as_ref().map(xml_value).unwrap_or_default(),
                            p.y.as_ref().map(xml_value).unwrap_or_default()
                        )
                    })
                    .collect();
                self.w.start_element(name);
                self.w.attribute("points", &written.join(" "));
                self.w.end_element();
            }
            Some(RawShape::Text(text)) => self.print_text(text),
        }
        self.w.end_element();
    }

    fn print_text(&mut self, text: &RawText) {
        let w = &mut self.w;
        w.start_element("text");
        unless_text(w, "fontfamily", &text.font_family, "sans-serif");
        unless_int(w, "pixelsize", &text.pixel_size, 16);
        for (name, value) in [("wrap", &text.wrap)] {
            if bool_of(value) {
                w.attribute(name, "1");
            }
        }
        unless_text(w, "color", &text.color, "#000000");
        for (name, value) in [
            ("bold", &text.bold),
            ("italic", &text.italic),
            ("underline", &text.underline),
            ("strikeout", &text.strikeout),
        ] {
            if bool_of(value) {
                w.attribute(name, "1");
            }
        }
        if text.kerning.as_ref().and_then(Scalar::to_bool) == Some(false) {
            w.attribute("kerning", "0");
        }
        unless_text(w, "halign", &text.halign, "left");
        unless_text(w, "valign", &text.valign, "top");
        w.characters(&text.text);
        w.end_element();
    }

    /// `<properties>`, nothing when there are none.
    fn print_properties(&mut self, properties: &[RawProperty]) {
        if properties.is_empty() {
            return;
        }
        self.w.start_element("properties");
        for property in properties {
            self.w.start_element("property");
            self.w.attribute("name", &property.name);
            self.print_property_value(property);
            self.w.end_element();
        }
        self.w.end_element();
    }

    /// `MapWriter::writePropertyValue`: the type unless a string, the custom type, then nested
    /// members, list items, or the value (as text when it has a line break).
    fn print_property_value(&mut self, property: &RawProperty) {
        let type_name = property.type_name.as_deref().unwrap_or("string");
        if type_name != "string" {
            self.w.attribute("type", type_name);
        }
        if let Some(custom_type) = &property.custom_type {
            self.w.attribute("propertytype", custom_type);
        }
        match &property.value {
            RawValue::Members(members) => {
                if !members.is_empty() {
                    self.w.start_element("properties");
                    for member in members {
                        self.w.start_element("property");
                        self.w.attribute("name", &member.name);
                        self.print_property_value(member);
                        self.w.end_element();
                    }
                    self.w.end_element();
                }
            }
            RawValue::Items(items) => {
                for item in items {
                    self.w.start_element("item");
                    self.print_property_value(item);
                    self.w.end_element();
                }
            }
            RawValue::Scalar(scalar) => {
                let written = match scalar {
                    Scalar::Text(text) => text.clone(),
                    Scalar::Int(value) => value.to_string(),
                    Scalar::Real(value) => real_shortest(*value),
                    Scalar::Bool(value) => value.to_string(),
                };
                if written.contains('\n') {
                    self.w.characters(&written);
                } else {
                    self.w.attribute("value", &written);
                }
            }
            RawValue::Absent => self.w.attribute("value", ""),
        }
    }
}

/// Whether Tiled lists an atlas tile (`includeTile` in `mapwriter.cpp`).
fn tile_has_data(tile: &RawTile) -> bool {
    let stated = |value: &Option<Scalar>| value.as_ref().is_some_and(|v| !v.is_empty_text());
    stated(&tile.class)
        || stated(&tile.class_as_type)
        || !tile.properties.is_empty()
        || tile.image.is_some()
        || tile.collision.is_some()
        || tile
            .animation
            .as_ref()
            .is_some_and(|frames| !frames.is_empty())
        || real(&tile.probability).is_some_and(|p| p != 1.0)
}

/// The data of the first tile layer, which says the map's layer data format.
fn first_data(layers: &[RawLayer]) -> Option<&RawData> {
    layers.iter().find_map(|layer| match layer.kind {
        RawLayerKind::Tile => layer.data.as_ref(),
        RawLayerKind::Group => first_data(&layer.layers),
        RawLayerKind::Object | RawLayerKind::Image => None,
    })
}

/// A value as an XML attribute: integers in decimal, reals with six significant digits,
/// booleans as 1 and 0.
fn xml_value(value: &Scalar) -> String {
    match value {
        Scalar::Text(text) => text.clone(),
        Scalar::Int(value) => value.to_string(),
        Scalar::Real(value) => real_g6(*value),
        Scalar::Bool(value) => if *value { "1" } else { "0" }.to_owned(),
    }
}

/// Writes an attribute when the value is stated.
fn attribute(w: &mut XmlWriter, name: &str, value: &Option<Scalar>) {
    if let Some(value) = value {
        w.attribute(name, &xml_value(value));
    }
}

/// Writes an attribute when the value is stated and not empty text.
fn non_empty(w: &mut XmlWriter, name: &str, value: &Option<Scalar>) {
    if let Some(value) = value.as_ref().filter(|v| !v.is_empty_text()) {
        w.attribute(name, &xml_value(value));
    }
}

/// Writes an integer attribute unless it is `default`.
fn unless_int(w: &mut XmlWriter, name: &str, value: &Option<Scalar>, default: i64) {
    if value
        .as_ref()
        .and_then(Scalar::to_int)
        .is_some_and(|v| v != default)
    {
        attribute(w, name, value);
    }
}

/// Writes a real attribute unless it is `default`.
fn unless_real(w: &mut XmlWriter, name: &str, value: &Option<Scalar>, default: f64) {
    if real(value).is_some_and(|v| v != default) {
        attribute(w, name, value);
    }
}

/// Writes a text attribute unless it is `default`.
fn unless_text(w: &mut XmlWriter, name: &str, value: &Option<Scalar>, default: &str) {
    if value
        .as_ref()
        .is_some_and(|v| !v.is_empty_text() && v.to_text() != default)
    {
        attribute(w, name, value);
    }
}

/// A stated value as text; empty when not stated.
fn text(value: &Option<Scalar>) -> String {
    value.as_ref().map(Scalar::to_text).unwrap_or_default()
}

/// A stated value as a real.
fn real(value: &Option<Scalar>) -> Option<f64> {
    value.as_ref().and_then(Scalar::to_real)
}

/// Whether a stated value is true.
fn bool_of(value: &Option<Scalar>) -> bool {
    value.as_ref().and_then(Scalar::to_bool) == Some(true)
}
