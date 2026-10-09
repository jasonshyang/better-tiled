//! The raw tree as Tiled's JSON writer states it (`libtiled/maptovariantconverter.cpp`): which
//! keys are always there, which only when not the default, and how values are typed.

use bt_core::LayerDataFormat;
use serde_json::{Map as JsonObject, Value as Json};

use crate::data::encode_base64;
use crate::names;
use crate::raw::{
    RawData, RawImage, RawLayer, RawLayerKind, RawMap, RawObject, RawPayload, RawProperty,
    RawShape, RawTemplate, RawText, RawTile, RawTileset, RawValue, RawWangId, RawWangSet, Scalar,
};

/// A JSON value of a stated value: text, number or boolean as stated.
fn json_of(value: &Scalar) -> Json {
    match value {
        Scalar::Text(text) => Json::String(text.clone()),
        Scalar::Int(value) => Json::from(*value),
        Scalar::Real(value) => {
            serde_json::Number::from_f64(*value).map_or(Json::Null, Json::Number)
        }
        Scalar::Bool(value) => Json::Bool(*value),
    }
}

/// Builds JSON objects with Tiled's keys.
struct Object(JsonObject<String, Json>);

impl Object {
    fn new() -> Object {
        Object(JsonObject::new())
    }

    /// Sets `key` to a stated value.
    fn set(&mut self, key: &str, value: &Option<Scalar>) {
        if let Some(value) = value {
            self.0.insert(key.to_owned(), json_of(value));
        }
    }

    /// Sets `key` to a stated value that is not empty text.
    fn set_non_empty(&mut self, key: &str, value: &Option<Scalar>) {
        if value.as_ref().is_some_and(|v| !v.is_empty_text()) {
            self.set(key, value);
        }
    }

    /// Sets `key` to a stated value unless it is `default`.
    fn set_unless(&mut self, key: &str, value: &Option<Scalar>, default: f64) {
        if value
            .as_ref()
            .and_then(Scalar::to_real)
            .is_some_and(|v| v != default)
        {
            self.set(key, value);
        }
    }

    /// Sets `key` to a stated text unless it is `default`.
    fn set_unless_text(&mut self, key: &str, value: &Option<Scalar>, default: &str) {
        if value
            .as_ref()
            .is_some_and(|v| !v.is_empty_text() && v.to_text() != default)
        {
            self.set(key, value);
        }
    }

    /// Sets `key` to `true` when the stated value is true.
    fn set_true(&mut self, key: &str, value: &Option<Scalar>) {
        if value.as_ref().and_then(Scalar::to_bool) == Some(true) {
            self.0.insert(key.to_owned(), Json::Bool(true));
        }
    }

    fn insert(&mut self, key: &str, value: Json) {
        self.0.insert(key.to_owned(), value);
    }

    fn done(self) -> Json {
        Json::Object(self.0)
    }
}

/// A map; the array wrap is the map's width, or its chunk width when infinite.
pub(crate) fn map_json(map: &RawMap) -> Json {
    let mut json = Object::new();
    json.insert("type", Json::from("map"));
    json.set_non_empty("class", &map.class);
    json.set("version", &map.version);
    json.set("tiledversion", &map.tiled_version);
    json.set("orientation", &map.orientation);
    json.set("renderorder", &map.render_order);
    json.set("width", &map.width);
    json.set("height", &map.height);
    json.set("tilewidth", &map.tile_width);
    json.set("tileheight", &map.tile_height);
    json.set("infinite", &map.infinite);
    json.set("nextlayerid", &map.next_layer_id);
    json.set("nextobjectid", &map.next_object_id);
    json.insert(
        "compressionlevel",
        map.compression_level
            .as_ref()
            .map_or(Json::from(-1), json_of),
    );
    if map.chunk_size.is_some() || map.export.is_some() {
        let mut settings = Object::new();
        if let Some(size) = &map.chunk_size {
            let mut chunk = Object::new();
            chunk.set("width", &size.width);
            chunk.set("height", &size.height);
            settings.insert("chunksize", chunk.done());
        }
        if let Some(export) = &map.export {
            let mut out = Object::new();
            out.set_non_empty("target", &export.target);
            out.set_non_empty("format", &export.format);
            settings.insert("export", out.done());
        }
        json.insert("editorsettings", settings.done());
    }
    set_properties(&mut json, &map.properties);
    let orientation = map
        .orientation
        .as_ref()
        .map(Scalar::to_text)
        .unwrap_or_default();
    if orientation == "hexagonal" {
        json.set("hexsidelength", &map.hex_side_length);
    }
    if orientation == "hexagonal" || orientation == "staggered" {
        json.set("staggeraxis", &map.stagger_axis);
        json.set("staggerindex", &map.stagger_index);
    }
    json.set_unless("skewx", &map.skew_x, 0.0);
    json.set_unless("skewy", &map.skew_y, 0.0);
    let real = |v: &Option<Scalar>| v.as_ref().and_then(Scalar::to_real).unwrap_or(0.0);
    if real(&map.parallax_origin_x) != 0.0 || real(&map.parallax_origin_y) != 0.0 {
        json.set("parallaxoriginx", &map.parallax_origin_x);
        json.set("parallaxoriginy", &map.parallax_origin_y);
    }
    json.set("backgroundcolor", &map.background_color);
    json.insert(
        "tilesets",
        Json::Array(map.tilesets.iter().map(tileset_json).collect()),
    );
    let level = map
        .compression_level
        .as_ref()
        .and_then(Scalar::to_int)
        .and_then(|level| i32::try_from(level).ok());
    json.insert(
        "layers",
        Json::Array(
            map.layers
                .iter()
                .map(|layer| layer_json(layer, level))
                .collect(),
        ),
    );
    json.done()
}

/// A tileset: a reference, an embedded tileset (with its first gid) or a tileset file.
pub(crate) fn tileset_json(tileset: &RawTileset) -> Json {
    let mut json = Object::new();
    if tileset.first_gid.is_some() {
        json.set("firstgid", &tileset.first_gid);
        if tileset.source.is_some() {
            json.set("source", &tileset.source);
            return json.done();
        }
    } else {
        json.insert("type", Json::from("tileset"));
        json.set("version", &tileset.version);
        json.set("tiledversion", &tileset.tiled_version);
    }
    json.set("name", &tileset.name);
    json.set_non_empty("class", &tileset.class);
    json.set("tilewidth", &tileset.tile_width);
    json.set("tileheight", &tileset.tile_height);
    json.set("spacing", &tileset.spacing);
    json.set("margin", &tileset.margin);
    json.set("tilecount", &tileset.tile_count);
    json.set("columns", &tileset.columns);
    if let (None, Some(export)) = (&tileset.first_gid, &tileset.export) {
        let mut out = Object::new();
        out.set("target", &export.target);
        out.set("format", &export.format);
        let mut settings = Object::new();
        settings.insert("export", out.done());
        json.insert("editorsettings", settings.done());
    }
    json.set("backgroundcolor", &tileset.background_color);
    json.set_unless_text("objectalignment", &tileset.object_alignment, "unspecified");
    json.set_unless_text("tilerendersize", &tileset.tile_render_size, "tile");
    json.set_unless_text("fillmode", &tileset.fill_mode, "stretch");
    set_properties(&mut json, &tileset.properties);
    if let Some(offset) = &tileset.tile_offset {
        let mut out = Object::new();
        out.set("x", &offset.x);
        out.set("y", &offset.y);
        json.insert("tileoffset", out.done());
    }
    if let Some(grid) = &tileset.grid {
        let mut out = Object::new();
        out.set("orientation", &grid.orientation);
        out.set("width", &grid.width);
        out.set("height", &grid.height);
        json.insert("grid", out.done());
    }
    if let Some(image) = tileset.image.as_ref().filter(|i| has_source(i)) {
        json.set("image", &image.source);
        json.set("transparentcolor", &image.trans);
        json.set("imagewidth", &image.width);
        json.set("imageheight", &image.height);
    }
    if let Some(flags) = &tileset.transformations {
        let mut out = Object::new();
        out.set("hflip", &flags.hflip);
        out.set("vflip", &flags.vflip);
        out.set("rotate", &flags.rotate);
        out.set("preferuntransformed", &flags.prefer_untransformed);
        json.insert("transformations", out.done());
    }
    let tiles: Vec<Json> = tileset
        .tiles
        .iter()
        .filter_map(|tile| tile_json(tile, tileset.lists_every_tile))
        .collect();
    if !tiles.is_empty() {
        json.insert("tiles", Json::Array(tiles));
    }
    if !tileset.wang_sets.is_empty() {
        json.insert(
            "wangsets",
            Json::Array(tileset.wang_sets.iter().map(wang_set_json).collect()),
        );
    }
    json.done()
}

/// A tile entry with its data; `None` for a tile without any, unless every tile is listed.
fn tile_json(tile: &RawTile, every_tile: bool) -> Option<Json> {
    let mut json = Object::new();
    set_properties(&mut json, &tile.properties);
    json.set_non_empty("class", &tile.class);
    json.set_non_empty("type", &tile.class_as_type);
    json.set_unless("probability", &tile.probability, 1.0);
    if let Some(image) = tile.image.as_ref().filter(|i| has_source(i)) {
        json.set("image", &image.source);
        let size = |v: &Option<Scalar>| v.as_ref().and_then(Scalar::to_int).unwrap_or(0);
        if size(&image.width) != 0 || size(&image.height) != 0 {
            json.set("imagewidth", &image.width);
            json.set("imageheight", &image.height);
        }
        json.set("x", &tile.x);
        json.set("y", &tile.y);
        json.set("width", &tile.width);
        json.set("height", &tile.height);
    }
    if let Some(group) = &tile.collision {
        json.insert("objectgroup", layer_json(group, None));
    }
    if let Some(frames) = tile.animation.as_ref().filter(|f| !f.is_empty()) {
        let frames = frames
            .iter()
            .map(|frame| {
                let mut out = Object::new();
                out.set("tileid", &frame.tile_id);
                out.set("duration", &frame.duration);
                out.done()
            })
            .collect();
        json.insert("animation", Json::Array(frames));
    }
    if json.0.is_empty() && !every_tile {
        return None;
    }
    json.set("id", &tile.id);
    Some(json.done())
}

fn wang_set_json(set: &RawWangSet) -> Json {
    let mut json = Object::new();
    json.set("name", &set.name);
    json.set_non_empty("class", &set.class);
    json.set("type", &set.kind);
    json.set("tile", &set.tile);
    let colors = set
        .colors
        .iter()
        .map(|color| {
            let mut out = Object::new();
            out.set("color", &color.color);
            out.set("name", &color.name);
            out.set_non_empty("class", &color.class);
            out.set("probability", &color.probability);
            out.set("tile", &color.tile);
            set_properties(&mut out, &color.properties);
            out.done()
        })
        .collect();
    json.insert("colors", Json::Array(colors));
    let tiles = set
        .tiles
        .iter()
        .map(|tile| {
            let mut out = Object::new();
            out.set("tileid", &tile.tile_id);
            if let Some(RawWangId::Lanes(lanes)) = &tile.wang_id {
                out.insert("wangid", Json::Array(lanes.iter().map(json_of).collect()));
            }
            out.done()
        })
        .collect();
    json.insert("wangtiles", Json::Array(tiles));
    set_properties(&mut json, &set.properties);
    json.done()
}

/// A layer; `level` is the map's compression level for base64 data.
fn layer_json(layer: &RawLayer, level: Option<i32>) -> Json {
    let mut json = Object::new();
    match layer.kind {
        RawLayerKind::Tile => {
            json.insert("type", Json::from("tilelayer"));
            match layer.data.as_ref().and_then(|d| d.chunk_bounds.as_ref()) {
                Some(bounds) => {
                    json.set("width", &bounds.width);
                    json.set("height", &bounds.height);
                    json.set("startx", &bounds.x);
                    json.set("starty", &bounds.y);
                }
                None => {
                    json.set("width", &layer.width);
                    json.set("height", &layer.height);
                }
            }
            set_layer_attributes(&mut json, layer);
            if let Some(data) = &layer.data {
                set_data(&mut json, data, layer, level);
            }
        }
        RawLayerKind::Object => {
            json.insert("type", Json::from("objectgroup"));
            json.set("color", &layer.color);
            json.set("draworder", &layer.draw_order);
            set_layer_attributes(&mut json, layer);
            json.insert(
                "objects",
                Json::Array(layer.objects.iter().map(object_json).collect()),
            );
        }
        RawLayerKind::Image => {
            json.insert("type", Json::from("imagelayer"));
            set_layer_attributes(&mut json, layer);
            let image = layer.image.as_ref().filter(|i| has_source(i));
            json.insert(
                "image",
                image
                    .and_then(|i| i.source.as_ref())
                    .map_or(Json::from(""), json_of),
            );
            if let Some(image) = image {
                let size = |v: &Option<Scalar>| v.as_ref().and_then(Scalar::to_int).unwrap_or(0);
                if size(&image.width) != 0 || size(&image.height) != 0 {
                    json.set("imagewidth", &image.width);
                    json.set("imageheight", &image.height);
                }
                json.set("transparentcolor", &image.trans);
            }
            json.set_true("repeatx", &layer.repeat_x);
            json.set_true("repeaty", &layer.repeat_y);
        }
        RawLayerKind::Group => {
            json.insert("type", Json::from("group"));
            set_layer_attributes(&mut json, layer);
            json.insert(
                "layers",
                Json::Array(
                    layer
                        .layers
                        .iter()
                        .map(|inner| layer_json(inner, level))
                        .collect(),
                ),
            );
        }
    }
    json.done()
}

/// `MapToVariantConverter::addLayerAttributes`.
fn set_layer_attributes(json: &mut Object, layer: &RawLayer) {
    json.set_unless("id", &layer.id, 0.0);
    json.insert("name", layer.name.as_ref().map_or(Json::from(""), json_of));
    json.set_non_empty("class", &layer.class);
    json.insert("x", layer.x.as_ref().map_or(Json::from(0), json_of));
    json.insert("y", layer.y.as_ref().map_or(Json::from(0), json_of));
    json.insert(
        "visible",
        Json::Bool(
            layer
                .visible
                .as_ref()
                .and_then(Scalar::to_bool)
                .unwrap_or(true),
        ),
    );
    json.set_true("locked", &layer.locked);
    json.insert(
        "opacity",
        layer.opacity.as_ref().map_or(Json::from(1), json_of),
    );
    let real = |v: &Option<Scalar>| v.as_ref().and_then(Scalar::to_real).unwrap_or(0.0);
    if real(&layer.offset_x) != 0.0 || real(&layer.offset_y) != 0.0 {
        json.set("offsetx", &layer.offset_x);
        json.set("offsety", &layer.offset_y);
    }
    json.set_unless("parallaxx", &layer.parallax_x, 1.0);
    json.set_unless("parallaxy", &layer.parallax_y, 1.0);
    json.set("tintcolor", &layer.tint_color);
    json.set_unless_text("mode", &layer.mode, "normal");
    set_properties(json, &layer.properties);
}

/// A tile layer's encoding and cells: an array of gids for CSV, a string for base64.
fn set_data(json: &mut Object, data: &RawData, layer: &RawLayer, level: Option<i32>) {
    let encoding = data
        .encoding
        .as_ref()
        .map(Scalar::to_text)
        .unwrap_or_default();
    let compression = data
        .compression
        .as_ref()
        .map(Scalar::to_text)
        .unwrap_or_default();
    let format = names::layer_data_format_from_names(&encoding, &compression)
        .unwrap_or(LayerDataFormat::Csv);
    if encoding == "base64" {
        json.insert("encoding", Json::from("base64"));
        json.insert("compression", Json::from(compression));
    }
    let cells = |payload: &RawPayload| -> Json {
        let RawPayload::Gids(gids) = payload else {
            return Json::Array(Vec::new());
        };
        match format {
            LayerDataFormat::Csv | LayerDataFormat::Xml => {
                Json::Array(gids.iter().map(|gid| Json::from(*gid)).collect())
            }
            _ => Json::String(encode_base64(gids, format, level)),
        }
    };
    if data.chunk_bounds.is_some() || !data.chunks.is_empty() {
        let chunks = data
            .chunks
            .iter()
            .map(|chunk| {
                let mut out = Object::new();
                out.set("x", &chunk.x);
                out.set("y", &chunk.y);
                out.set("width", &chunk.width);
                out.set("height", &chunk.height);
                out.insert("data", cells(&chunk.payload));
                out.done()
            })
            .collect();
        json.insert("chunks", Json::Array(chunks));
    } else {
        let _ = layer;
        json.insert("data", cells(&data.payload));
    }
}

/// An object: a template instance states what it overrides, other objects their main values
/// always (`MapToVariantConverter::toVariant(const MapObject &)`).
fn object_json(object: &RawObject) -> Json {
    let instance = object.template.is_some();
    let mut json = Object::new();
    set_properties(&mut json, &object.properties);
    json.set("template", &object.template);
    json.set_unless("id", &object.id, 0.0);
    let stated_or = |value: &Option<Scalar>, default: Json| -> Option<Json> {
        match value {
            Some(value) => Some(json_of(value)),
            None if !instance => Some(default),
            None => None,
        }
    };
    if let Some(name) = stated_or(&object.name, Json::from("")) {
        json.insert("name", name);
    }
    let class_key = if object.class.is_some() {
        "class"
    } else {
        "type"
    };
    match object.class.as_ref().or(object.class_as_type.as_ref()) {
        Some(class) if !instance || !class.is_empty_text() => {
            json.insert(class_key, json_of(class))
        }
        None if !instance => json.insert(class_key, Json::from("")),
        _ => {}
    }
    json.set("gid", &object.gid);
    json.set("x", &object.x);
    json.set("y", &object.y);
    for (key, value, default) in [
        ("width", &object.width, Json::from(0)),
        ("height", &object.height, Json::from(0)),
        ("rotation", &object.rotation, Json::from(0)),
        ("opacity", &object.opacity, Json::from(1)),
        ("visible", &object.visible, Json::Bool(true)),
    ] {
        if let Some(value) = stated_or(value, default) {
            json.insert(key, value);
        }
    }
    match &object.shape {
        None => {}
        Some(RawShape::Ellipse) => json.insert("ellipse", Json::Bool(true)),
        Some(RawShape::Capsule) => json.insert("capsule", Json::Bool(true)),
        Some(RawShape::Point) => json.insert("point", Json::Bool(true)),
        Some(RawShape::Polygon(points)) => json.insert("polygon", points_json(points)),
        Some(RawShape::Polyline(points)) => json.insert("polyline", points_json(points)),
        Some(RawShape::Text(text)) => json.insert("text", text_json(text)),
    }
    json.done()
}

fn points_json(points: &[crate::raw::RawPoint]) -> Json {
    Json::Array(
        points
            .iter()
            .map(|point| {
                let mut out = Object::new();
                out.set("x", &point.x);
                out.set("y", &point.y);
                out.done()
            })
            .collect(),
    )
}

fn text_json(text: &RawText) -> Json {
    let mut json = Object::new();
    json.insert("text", Json::String(text.text.clone()));
    json.set_unless_text("fontfamily", &text.font_family, "sans-serif");
    json.set_unless("pixelsize", &text.pixel_size, 16.0);
    json.set_true("wrap", &text.wrap);
    json.set_unless_text("color", &text.color, "#000000");
    json.set_true("bold", &text.bold);
    json.set_true("italic", &text.italic);
    json.set_true("underline", &text.underline);
    json.set_true("strikeout", &text.strikeout);
    if text.kerning.as_ref().and_then(Scalar::to_bool) == Some(false) {
        json.insert("kerning", Json::Bool(false));
    }
    json.set_unless_text("halign", &text.halign, "left");
    json.set_unless_text("valign", &text.valign, "top");
    json.done()
}

/// A template file.
pub(crate) fn template_json(template: &RawTemplate) -> Json {
    let mut json = Object::new();
    json.insert("type", Json::from("template"));
    if let Some(tileset) = template.tilesets.first() {
        json.insert("tileset", tileset_json(tileset));
    }
    if let Some(object) = &template.object {
        json.insert("object", object_json(object));
    }
    json.done()
}

/// Sets `properties` when there are any.
fn set_properties(json: &mut Object, properties: &[RawProperty]) {
    if !properties.is_empty() {
        json.insert("properties", properties_json(properties, "propertytype"));
    }
}

/// Properties as Tiled writes them: `{"name", "type", "value", "propertytype"}` entries, class
/// values as objects of member values, list items typed. Projects' type definitions name the
/// custom type `propertyType`.
pub(crate) fn properties_json(properties: &[RawProperty], custom_type_key: &str) -> Json {
    Json::Array(
        properties
            .iter()
            .map(|property| {
                let mut entry = typed_json(property, custom_type_key);
                if let Json::Object(map) = &mut entry {
                    map.insert("name".to_owned(), Json::String(property.name.clone()));
                }
                entry
            })
            .collect(),
    )
}

/// A typed value: `{"type", "value", "propertytype"}`.
fn typed_json(property: &RawProperty, custom_type_key: &str) -> Json {
    let mut json = Object::new();
    json.insert(
        "type",
        Json::String(
            property
                .type_name
                .clone()
                .unwrap_or_else(|| "string".to_owned()),
        ),
    );
    json.insert("value", value_json(&property.value, custom_type_key));
    if let Some(custom_type) = &property.custom_type {
        json.insert(custom_type_key, Json::String(custom_type.clone()));
    }
    json.done()
}

/// A value as JSON stores it: class values as objects of member values, list items typed.
fn value_json(value: &RawValue, custom_type_key: &str) -> Json {
    match value {
        RawValue::Scalar(scalar) => json_of(scalar),
        RawValue::Members(members) => Json::Object(
            members
                .iter()
                .map(|member| {
                    (
                        member.name.clone(),
                        value_json(&member.value, custom_type_key),
                    )
                })
                .collect(),
        ),
        RawValue::Items(items) => Json::Array(
            items
                .iter()
                .map(|item| typed_json(item, custom_type_key))
                .collect(),
        ),
        RawValue::Absent => Json::Null,
    }
}

/// Whether an image names a file (JSON cannot hold embedded images).
fn has_source(image: &RawImage) -> bool {
    image.source.as_ref().is_some_and(|s| !s.is_empty_text())
}
