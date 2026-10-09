//! JSON text into the raw tree: keys as written, the shapes of Tiled 1.0 and 1.1 expressed in
//! today's terms, unknown keys reported.

use std::path::Path;

use serde_json::{Map as JsonObject, Value as Json};

use crate::error::Error;
use crate::raw::{
    RawChunk, RawData, RawExport, RawFrame, RawGrid, RawImage, RawLayer, RawLayerKind, RawMap,
    RawObject, RawPayload, RawPoint, RawProperty, RawShape, RawSize, RawTemplate, RawTerrain,
    RawText, RawTile, RawTileset, RawTransformations, RawValue, RawWangColor, RawWangId,
    RawWangSet, RawWangTile, Scalar, WangColorRole,
};
use crate::warning::{WarningKind, Warnings};

/// Parses JSON text (a byte order mark is allowed).
pub(crate) fn parse_document(text: &str, path: &Path) -> Result<Json, Error> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// The root object of a JSON file of the given kind; a `type` naming another kind is refused
/// (files before Tiled 1.0 have none).
fn root<'v>(
    value: &'v Json,
    path: &Path,
    kind: &str,
) -> Result<&'v JsonObject<String, Json>, Error> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::format(path, format!("not a Tiled {kind}: not a JSON object")))?;
    match object.get("type").and_then(Json::as_str) {
        None => Ok(object),
        Some(stated) if stated == kind => Ok(object),
        Some(other) => Err(Error::format(
            path,
            format!("not a Tiled {kind}: its type is “{other}”"),
        )),
    }
}

/// A map file.
pub(crate) fn read_map(
    value: &Json,
    path: &Path,
    warnings: &mut Warnings,
) -> Result<RawMap, Error> {
    let object = root(value, path, "map")?;
    Ok(JsonReader { warnings }.read_map(object))
}

/// A tileset file.
pub(crate) fn read_tileset_file(
    value: &Json,
    path: &Path,
    warnings: &mut Warnings,
) -> Result<RawTileset, Error> {
    let object = root(value, path, "tileset")?;
    Ok(JsonReader { warnings }.read_tileset(object))
}

/// A template file.
pub(crate) fn read_template(
    value: &Json,
    path: &Path,
    warnings: &mut Warnings,
) -> Result<RawTemplate, Error> {
    let object = root(value, path, "template")?;
    Ok(JsonReader { warnings }.read_template(object))
}

/// The keys every layer has.
const LAYER_KEYS: [&str; 18] = [
    "type",
    "id",
    "name",
    "class",
    "x",
    "y",
    "width",
    "height",
    "visible",
    "locked",
    "opacity",
    "tintcolor",
    "offsetx",
    "offsety",
    "parallaxx",
    "parallaxy",
    "mode",
    "properties",
];

/// Reads JSON objects into the raw tree, recording what it does not know.
pub(crate) struct JsonReader<'w> {
    /// Where problems go.
    pub(crate) warnings: &'w mut Warnings,
}

impl JsonReader<'_> {
    /// Records the keys of `object` that are not in `known`.
    pub(crate) fn check_keys(
        &mut self,
        object: &JsonObject<String, Json>,
        what: &str,
        known: &[&str],
    ) {
        for key in object.keys() {
            if !known.contains(&key.as_str()) {
                self.warnings.record(
                    WarningKind::UnknownKey,
                    format!("unknown key “{key}” in {what}; it is not kept"),
                );
            }
        }
    }

    /// The scalar under `key`; `null` is absent, and containers are reported.
    pub(crate) fn scalar(
        &mut self,
        object: &JsonObject<String, Json>,
        key: &str,
    ) -> Option<Scalar> {
        let value = object.get(key)?;
        let scalar = scalar_of(value);
        if scalar.is_none() && !value.is_null() {
            self.warnings.record(
                WarningKind::InvalidValue,
                format!(
                    "“{key}” holds {}, not a single value; it is ignored",
                    shown(value)
                ),
            );
        }
        scalar
    }

    /// The object under `key`, reporting another kind of value.
    pub(crate) fn object<'v>(
        &mut self,
        object: &'v JsonObject<String, Json>,
        key: &str,
    ) -> Option<&'v JsonObject<String, Json>> {
        let value = object.get(key).filter(|v| !v.is_null())?;
        let found = value.as_object();
        if found.is_none() {
            self.warnings.record(
                WarningKind::InvalidValue,
                format!(
                    "“{key}” holds {}, not an object; it is ignored",
                    shown(value)
                ),
            );
        }
        found
    }

    /// The array under `key`, reporting another kind of value.
    pub(crate) fn array<'v>(
        &mut self,
        object: &'v JsonObject<String, Json>,
        key: &str,
    ) -> &'v [Json] {
        match object.get(key) {
            None | Some(Json::Null) => &[],
            Some(Json::Array(items)) => items,
            Some(other) => {
                self.warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "“{key}” holds {}, not an array; it is ignored",
                        shown(other)
                    ),
                );
                &[]
            }
        }
    }

    /// The objects of the array under `key`, reporting items that are not objects.
    pub(crate) fn objects<'v>(
        &mut self,
        object: &'v JsonObject<String, Json>,
        key: &str,
    ) -> Vec<&'v JsonObject<String, Json>> {
        let mut out = Vec::new();
        for item in self.array(object, key) {
            match item.as_object() {
                Some(found) => out.push(found),
                None => self.warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "an item of “{key}” is {}, not an object; it is ignored",
                        shown(item)
                    ),
                ),
            }
        }
        out
    }

    /// The text under `key`; empty when absent.
    pub(crate) fn text_of(&mut self, object: &JsonObject<String, Json>, key: &str) -> String {
        self.scalar(object, key)
            .map(|v| v.to_text())
            .unwrap_or_default()
    }

    pub(crate) fn read_map(&mut self, object: &JsonObject<String, Json>) -> RawMap {
        self.check_keys(
            object,
            "the map",
            &[
                "type",
                "version",
                "tiledversion",
                "class",
                "orientation",
                "renderorder",
                "compressionlevel",
                "width",
                "height",
                "tilewidth",
                "tileheight",
                "infinite",
                "hexsidelength",
                "staggeraxis",
                "staggerindex",
                "skewx",
                "skewy",
                "parallaxoriginx",
                "parallaxoriginy",
                "backgroundcolor",
                "nextlayerid",
                "nextobjectid",
                "editorsettings",
                "properties",
                "propertytypes",
                "tilesets",
                "layers",
            ],
        );
        let mut map = RawMap {
            version: self.scalar(object, "version"),
            tiled_version: self.scalar(object, "tiledversion"),
            class: self.scalar(object, "class"),
            orientation: self.scalar(object, "orientation"),
            render_order: self.scalar(object, "renderorder"),
            compression_level: self.scalar(object, "compressionlevel"),
            width: self.scalar(object, "width"),
            height: self.scalar(object, "height"),
            tile_width: self.scalar(object, "tilewidth"),
            tile_height: self.scalar(object, "tileheight"),
            infinite: self.scalar(object, "infinite"),
            hex_side_length: self.scalar(object, "hexsidelength"),
            stagger_axis: self.scalar(object, "staggeraxis"),
            stagger_index: self.scalar(object, "staggerindex"),
            skew_x: self.scalar(object, "skewx"),
            skew_y: self.scalar(object, "skewy"),
            parallax_origin_x: self.scalar(object, "parallaxoriginx"),
            parallax_origin_y: self.scalar(object, "parallaxoriginy"),
            background_color: self.scalar(object, "backgroundcolor"),
            next_layer_id: self.scalar(object, "nextlayerid"),
            next_object_id: self.scalar(object, "nextobjectid"),
            properties: self.properties_of(object),
            ..RawMap::default()
        };
        if let Some(settings) = self.object(object, "editorsettings") {
            self.check_keys(
                settings,
                "the map's editor settings",
                &["chunksize", "export"],
            );
            if let Some(size) = self.object(settings, "chunksize") {
                self.check_keys(size, "the map's chunk size", &["width", "height"]);
                map.chunk_size = Some(RawSize {
                    width: self.scalar(size, "width"),
                    height: self.scalar(size, "height"),
                });
            }
            if let Some(export) = self.object(settings, "export") {
                map.export = Some(self.read_export(export));
            }
        }
        for tileset in self.objects(object, "tilesets") {
            map.tilesets.push(self.read_tileset(tileset));
        }
        for layer in self.objects(object, "layers") {
            if let Some(layer) = self.read_layer(layer) {
                map.layers.push(layer);
            }
        }
        map
    }

    fn read_export(&mut self, object: &JsonObject<String, Json>) -> RawExport {
        self.check_keys(object, "an export setting", &["target", "format"]);
        RawExport {
            target: self.scalar(object, "target"),
            format: self.scalar(object, "format"),
        }
    }

    /// A tileset: a map's or template's reference or embedded tileset, or a tileset file.
    pub(crate) fn read_tileset(&mut self, object: &JsonObject<String, Json>) -> RawTileset {
        self.check_keys(
            object,
            "a tileset",
            &[
                "firstgid",
                "source",
                "type",
                "version",
                "tiledversion",
                "name",
                "class",
                "tilewidth",
                "tileheight",
                "spacing",
                "margin",
                "tilecount",
                "columns",
                "backgroundcolor",
                "objectalignment",
                "tilerendersize",
                "fillmode",
                "editorsettings",
                "properties",
                "propertytypes",
                "tileoffset",
                "grid",
                "image",
                "imagewidth",
                "imageheight",
                "transparentcolor",
                "transformations",
                "tiles",
                "tileproperties",
                "tilepropertytypes",
                "terrains",
                "wangsets",
            ],
        );
        let mut tileset = RawTileset {
            first_gid: self.scalar(object, "firstgid"),
            source: self.scalar(object, "source"),
            version: self.scalar(object, "version"),
            tiled_version: self.scalar(object, "tiledversion"),
            name: self.scalar(object, "name"),
            class: self.scalar(object, "class"),
            tile_width: self.scalar(object, "tilewidth"),
            tile_height: self.scalar(object, "tileheight"),
            spacing: self.scalar(object, "spacing"),
            margin: self.scalar(object, "margin"),
            tile_count: self.scalar(object, "tilecount"),
            columns: self.scalar(object, "columns"),
            background_color: self.scalar(object, "backgroundcolor"),
            object_alignment: self.scalar(object, "objectalignment"),
            tile_render_size: self.scalar(object, "tilerendersize"),
            fill_mode: self.scalar(object, "fillmode"),
            properties: self.properties_of(object),
            ..RawTileset::default()
        };
        if let Some(settings) = self.object(object, "editorsettings") {
            self.check_keys(settings, "the tileset's editor settings", &["export"]);
            if let Some(export) = self.object(settings, "export") {
                tileset.export = Some(self.read_export(export));
            }
        }
        if let Some(offset) = self.object(object, "tileoffset") {
            self.check_keys(offset, "a tile offset", &["x", "y"]);
            tileset.tile_offset = Some(RawPoint {
                x: self.scalar(offset, "x"),
                y: self.scalar(offset, "y"),
            });
        }
        if let Some(grid) = self.object(object, "grid") {
            self.check_keys(grid, "a tileset grid", &["orientation", "width", "height"]);
            tileset.grid = Some(RawGrid {
                orientation: self.scalar(grid, "orientation"),
                width: self.scalar(grid, "width"),
                height: self.scalar(grid, "height"),
            });
        }
        if let Some(flags) = self.object(object, "transformations") {
            self.check_keys(
                flags,
                "a tileset's transformations",
                &["hflip", "vflip", "rotate", "preferuntransformed"],
            );
            tileset.transformations = Some(RawTransformations {
                hflip: self.scalar(flags, "hflip"),
                vflip: self.scalar(flags, "vflip"),
                rotate: self.scalar(flags, "rotate"),
                prefer_untransformed: self.scalar(flags, "preferuntransformed"),
            });
        }
        tileset.image = self.image_of(object, true);
        tileset.tiles = self.tiles_of(object);
        for terrain in self.objects(object, "terrains") {
            self.check_keys(
                terrain,
                "a terrain",
                &["name", "tile", "properties", "propertytypes"],
            );
            tileset.terrains.push(RawTerrain {
                name: self.scalar(terrain, "name"),
                tile: self.scalar(terrain, "tile"),
                properties: self.properties_of(terrain),
            });
        }
        for set in self.objects(object, "wangsets") {
            tileset.wang_sets.push(self.read_wang_set(set));
        }
        tileset
    }

    /// The image keys of a tileset, tile or image layer; `None` without `image`.
    fn image_of(
        &mut self,
        object: &JsonObject<String, Json>,
        with_trans: bool,
    ) -> Option<RawImage> {
        let source = self.scalar(object, "image")?;
        Some(RawImage {
            source: Some(source),
            trans: if with_trans {
                self.scalar(object, "transparentcolor")
            } else {
                None
            },
            width: self.scalar(object, "imagewidth"),
            height: self.scalar(object, "imageheight"),
            format: None,
            data: None,
        })
    }

    /// A tileset's tiles: today's list, or the object keyed by id of Tiled 1.0 and 1.1 (with
    /// `tileproperties`), which lists them by id.
    fn tiles_of(&mut self, object: &JsonObject<String, Json>) -> Vec<RawTile> {
        let mut tiles = Vec::new();
        match object.get("tiles") {
            Some(Json::Array(_)) => {
                for tile in self.objects(object, "tiles") {
                    tiles.push(self.read_tile(tile, None));
                }
            }
            Some(Json::Object(by_id)) => {
                self.warnings.record(
                    WarningKind::LegacyShape,
                    "the tiles are an object keyed by id (Tiled 1.0 and 1.1); they become a list",
                );
                for (id, tile) in by_id {
                    match tile.as_object() {
                        Some(tile) => tiles.push(self.read_tile(tile, Some(Scalar::text(id)))),
                        None => self.warnings.record(
                            WarningKind::InvalidValue,
                            format!(
                                "tile “{id}” is {}, not an object; it is ignored",
                                shown(tile)
                            ),
                        ),
                    }
                }
            }
            None | Some(Json::Null) => {}
            Some(other) => self.warnings.record(
                WarningKind::InvalidValue,
                format!("“tiles” holds {}, not a list; it is ignored", shown(other)),
            ),
        }
        if let Some(by_id) = self.object(object, "tileproperties") {
            self.warnings.record(
                WarningKind::LegacyShape,
                "tile properties are kept beside the tiles (Tiled 1.0 and 1.1); they move onto them",
            );
            let types = object.get("tilepropertytypes").and_then(Json::as_object);
            for (id, values) in by_id {
                let properties = legacy_properties(values, types.and_then(|t| t.get(id)));
                match tiles.iter_mut().find(|tile| {
                    tile.id.as_ref().map(Scalar::to_text).as_deref() == Some(id.as_str())
                }) {
                    Some(tile) => tile.properties.extend(properties),
                    None => tiles.push(RawTile {
                        id: Some(Scalar::text(id)),
                        properties,
                        ..RawTile::default()
                    }),
                }
            }
        }
        if !matches!(object.get("tiles"), Some(Json::Array(_))) {
            tiles.sort_by_key(|tile| tile.id.as_ref().and_then(Scalar::to_int).unwrap_or(-1));
        }
        tiles
    }

    fn read_tile(&mut self, object: &JsonObject<String, Json>, id: Option<Scalar>) -> RawTile {
        self.check_keys(
            object,
            "a tile",
            &[
                "id",
                "class",
                "type",
                "probability",
                "x",
                "y",
                "width",
                "height",
                "terrain",
                "properties",
                "propertytypes",
                "image",
                "imagewidth",
                "imageheight",
                "objectgroup",
                "animation",
            ],
        );
        let mut tile = RawTile {
            id: id.or_else(|| self.scalar(object, "id")),
            class: self.scalar(object, "class"),
            class_as_type: self.scalar(object, "type"),
            probability: self.scalar(object, "probability"),
            x: self.scalar(object, "x"),
            y: self.scalar(object, "y"),
            width: self.scalar(object, "width"),
            height: self.scalar(object, "height"),
            terrain: object
                .get("terrain")
                .and_then(Json::as_array)
                .map(|corners| corners.iter().map(scalar_of).collect()),
            properties: self.properties_of(object),
            image: self.image_of(object, false),
            ..RawTile::default()
        };
        if let Some(group) = self.object(object, "objectgroup") {
            tile.collision = self.read_layer(group);
        }
        if object.contains_key("animation") {
            let frames = self
                .objects(object, "animation")
                .into_iter()
                .map(|frame| {
                    self.check_keys(frame, "an animation frame", &["tileid", "duration"]);
                    RawFrame {
                        tile_id: self.scalar(frame, "tileid"),
                        duration: self.scalar(frame, "duration"),
                    }
                })
                .collect();
            tile.animation = Some(frames);
        }
        tile
    }

    fn read_wang_set(&mut self, object: &JsonObject<String, Json>) -> RawWangSet {
        self.check_keys(
            object,
            "a terrain set",
            &[
                "name",
                "class",
                "type",
                "tile",
                "properties",
                "propertytypes",
                "colors",
                "cornercolors",
                "edgecolors",
                "wangtiles",
            ],
        );
        let mut set = RawWangSet {
            name: self.scalar(object, "name"),
            class: self.scalar(object, "class"),
            kind: self.scalar(object, "type"),
            tile: self.scalar(object, "tile"),
            properties: self.properties_of(object),
            ..RawWangSet::default()
        };
        // Tiled reads `colors`, then `edgecolors`, then `cornercolors`.
        for (key, role) in [
            ("colors", WangColorRole::Any),
            ("edgecolors", WangColorRole::Edge),
            ("cornercolors", WangColorRole::Corner),
        ] {
            for color in self.objects(object, key) {
                self.check_keys(
                    color,
                    "a terrain",
                    &[
                        "name",
                        "class",
                        "color",
                        "tile",
                        "probability",
                        "properties",
                        "propertytypes",
                    ],
                );
                set.colors.push(RawWangColor {
                    role,
                    name: self.scalar(color, "name"),
                    class: self.scalar(color, "class"),
                    color: self.scalar(color, "color"),
                    tile: self.scalar(color, "tile"),
                    probability: self.scalar(color, "probability"),
                    properties: self.properties_of(color),
                });
            }
        }
        for tile in self.objects(object, "wangtiles") {
            self.check_keys(
                tile,
                "a terrain tile",
                &["tileid", "wangid", "hflip", "vflip", "dflip"],
            );
            let wang_id = match tile.get("wangid") {
                Some(Json::Array(lanes)) => Some(RawWangId::Lanes(
                    lanes.iter().filter_map(scalar_of).collect(),
                )),
                Some(Json::Number(number)) => number.as_i64().map(RawWangId::Packed),
                Some(Json::String(text)) => Some(RawWangId::Text(text.clone())),
                _ => None,
            };
            set.tiles.push(RawWangTile {
                tile_id: self.scalar(tile, "tileid"),
                wang_id,
            });
        }
        set
    }

    /// A layer; `None` (reported) for an unknown type.
    fn read_layer(&mut self, object: &JsonObject<String, Json>) -> Option<RawLayer> {
        let type_name = object.get("type").and_then(Json::as_str).unwrap_or("");
        let kind = match type_name {
            "tilelayer" => RawLayerKind::Tile,
            "objectgroup" => RawLayerKind::Object,
            "imagelayer" => RawLayerKind::Image,
            "group" => RawLayerKind::Group,
            other => {
                self.warnings.record(
                    WarningKind::UnknownKey,
                    format!("a layer of the unknown type “{other}” is not kept"),
                );
                return None;
            }
        };
        let extra: &[&str] = match kind {
            RawLayerKind::Tile => &[
                "startx",
                "starty",
                "encoding",
                "compression",
                "data",
                "chunks",
                "propertytypes",
            ],
            RawLayerKind::Object => &["color", "draworder", "objects", "propertytypes"],
            RawLayerKind::Image => &[
                "image",
                "imagewidth",
                "imageheight",
                "transparentcolor",
                "repeatx",
                "repeaty",
                "propertytypes",
            ],
            RawLayerKind::Group => &["layers", "propertytypes"],
        };
        let known: Vec<&str> = LAYER_KEYS.iter().chain(extra).copied().collect();
        self.check_keys(object, "a layer", &known);
        let mut layer = RawLayer {
            kind,
            id: self.scalar(object, "id"),
            name: self.scalar(object, "name"),
            class: self.scalar(object, "class"),
            x: self.scalar(object, "x"),
            y: self.scalar(object, "y"),
            width: self.scalar(object, "width"),
            height: self.scalar(object, "height"),
            visible: self.scalar(object, "visible"),
            locked: self.scalar(object, "locked"),
            opacity: self.scalar(object, "opacity"),
            tint_color: self.scalar(object, "tintcolor"),
            offset_x: self.scalar(object, "offsetx"),
            offset_y: self.scalar(object, "offsety"),
            parallax_x: self.scalar(object, "parallaxx"),
            parallax_y: self.scalar(object, "parallaxy"),
            mode: self.scalar(object, "mode"),
            properties: self.properties_of(object),
            ..RawLayer::default()
        };
        match kind {
            RawLayerKind::Tile => {
                layer.start_x = self.scalar(object, "startx");
                layer.start_y = self.scalar(object, "starty");
                let has_data = object.get("data").is_some_and(|d| !d.is_null());
                if has_data || object.contains_key("chunks") {
                    layer.data = Some(RawData {
                        // In JSON, data without an encoding is an array of gids: CSV.
                        encoding: self
                            .scalar(object, "encoding")
                            .or(Some(Scalar::text("csv"))),
                        compression: self.scalar(object, "compression"),
                        payload: self.payload_of(object.get("data")),
                        chunks: self
                            .objects(object, "chunks")
                            .into_iter()
                            .map(|chunk| {
                                self.check_keys(
                                    chunk,
                                    "a chunk",
                                    &["x", "y", "width", "height", "data"],
                                );
                                RawChunk {
                                    x: self.scalar(chunk, "x"),
                                    y: self.scalar(chunk, "y"),
                                    width: self.scalar(chunk, "width"),
                                    height: self.scalar(chunk, "height"),
                                    payload: self.payload_of(chunk.get("data")),
                                }
                            })
                            .collect(),
                        chunk_bounds: None,
                    });
                }
            }
            RawLayerKind::Object => {
                layer.color = self.scalar(object, "color");
                layer.draw_order = self.scalar(object, "draworder");
                for item in self.objects(object, "objects") {
                    layer.objects.push(self.read_object(item));
                }
            }
            RawLayerKind::Image => {
                layer.image = self.image_of(object, true);
                layer.repeat_x = self.scalar(object, "repeatx");
                layer.repeat_y = self.scalar(object, "repeaty");
            }
            RawLayerKind::Group => {
                for inner in self.objects(object, "layers") {
                    if let Some(inner) = self.read_layer(inner) {
                        layer.layers.push(inner);
                    }
                }
            }
        }
        Some(layer)
    }

    /// Tile data: an array of gids or a base64 string.
    fn payload_of(&mut self, data: Option<&Json>) -> RawPayload {
        match data {
            None | Some(Json::Null) => RawPayload::Empty,
            Some(Json::String(text)) => RawPayload::Text(text.clone()),
            Some(Json::Array(items)) => {
                let mut bad = 0usize;
                let gids = items
                    .iter()
                    .map(|item| {
                        scalar_of(item).and_then(|s| s.to_gid()).unwrap_or_else(|| {
                            bad += 1;
                            0
                        })
                    })
                    .collect();
                if bad > 0 {
                    self.warnings.record(
                        WarningKind::CorruptLayerData,
                        format!(
                            "{bad} items of layer data are not tile ids; their cells are empty"
                        ),
                    );
                }
                RawPayload::Gids(gids)
            }
            Some(other) => {
                self.warnings.record(
                    WarningKind::CorruptLayerData,
                    format!(
                        "layer data is {}, not an array or a string; it is ignored",
                        shown(other)
                    ),
                );
                RawPayload::Empty
            }
        }
    }

    /// An object.
    pub(crate) fn read_object(&mut self, object: &JsonObject<String, Json>) -> RawObject {
        self.check_keys(
            object,
            "an object",
            &[
                "id",
                "name",
                "class",
                "type",
                "gid",
                "x",
                "y",
                "width",
                "height",
                "rotation",
                "opacity",
                "visible",
                "template",
                "properties",
                "propertytypes",
                "ellipse",
                "point",
                "capsule",
                "polygon",
                "polyline",
                "text",
            ],
        );
        let mut raw = RawObject {
            id: self.scalar(object, "id"),
            name: self.scalar(object, "name"),
            class: self.scalar(object, "class"),
            class_as_type: self.scalar(object, "type"),
            gid: self.scalar(object, "gid"),
            x: self.scalar(object, "x"),
            y: self.scalar(object, "y"),
            width: self.scalar(object, "width"),
            height: self.scalar(object, "height"),
            rotation: self.scalar(object, "rotation"),
            opacity: self.scalar(object, "opacity"),
            visible: self.scalar(object, "visible"),
            template: self.scalar(object, "template"),
            properties: self.properties_of(object),
            shape: None,
        };
        let flag = |reader: &mut Self, key: &str| {
            reader.scalar(object, key).and_then(|v| v.to_bool()) == Some(true)
        };
        // In Tiled's order, the last one found wins.
        if object.get("polygon").is_some_and(Json::is_array) {
            raw.shape = Some(RawShape::Polygon(self.points_of(object, "polygon")));
        }
        if object.get("polyline").is_some_and(Json::is_array) {
            raw.shape = Some(RawShape::Polyline(self.points_of(object, "polyline")));
        }
        if flag(self, "ellipse") {
            raw.shape = Some(RawShape::Ellipse);
        }
        if flag(self, "capsule") {
            raw.shape = Some(RawShape::Capsule);
        }
        if flag(self, "point") {
            raw.shape = Some(RawShape::Point);
        }
        if let Some(text) = object.get("text").and_then(Json::as_object) {
            raw.shape = Some(RawShape::Text(Box::new(self.read_text(text))));
        }
        raw
    }

    fn points_of(&mut self, object: &JsonObject<String, Json>, key: &str) -> Vec<RawPoint> {
        self.objects(object, key)
            .into_iter()
            .map(|point| {
                self.check_keys(point, "a point", &["x", "y"]);
                RawPoint {
                    x: self.scalar(point, "x"),
                    y: self.scalar(point, "y"),
                }
            })
            .collect()
    }

    fn read_text(&mut self, object: &JsonObject<String, Json>) -> RawText {
        self.check_keys(
            object,
            "a text",
            &[
                "text",
                "fontfamily",
                "pixelsize",
                "wrap",
                "color",
                "bold",
                "italic",
                "underline",
                "strikeout",
                "kerning",
                "halign",
                "valign",
            ],
        );
        RawText {
            font_family: self.scalar(object, "fontfamily"),
            pixel_size: self.scalar(object, "pixelsize"),
            wrap: self.scalar(object, "wrap"),
            color: self.scalar(object, "color"),
            bold: self.scalar(object, "bold"),
            italic: self.scalar(object, "italic"),
            underline: self.scalar(object, "underline"),
            strikeout: self.scalar(object, "strikeout"),
            kerning: self.scalar(object, "kerning"),
            halign: self.scalar(object, "halign"),
            valign: self.scalar(object, "valign"),
            text: self
                .scalar(object, "text")
                .map(|t| t.to_text())
                .unwrap_or_default(),
        }
    }

    fn read_template(&mut self, object: &JsonObject<String, Json>) -> RawTemplate {
        self.check_keys(object, "the template", &["type", "tileset", "object"]);
        let mut template = RawTemplate::default();
        if let Some(tileset) = self.object(object, "tileset") {
            template.tilesets.push(self.read_tileset(tileset));
        }
        if let Some(found) = self.object(object, "object") {
            template.object = Some(self.read_object(found));
        }
        template
    }

    /// The `properties` of an object: today's list of typed entries, or the name-to-value object
    /// of Tiled 1.0 and 1.1 with its `propertytypes`.
    pub(crate) fn properties_of(&mut self, object: &JsonObject<String, Json>) -> Vec<RawProperty> {
        self.properties_with_key(object, "propertytype")
    }

    /// The same, with the custom type under `custom_type_key` (`propertyType` in a project's
    /// type definitions).
    pub(crate) fn properties_with_key(
        &mut self,
        object: &JsonObject<String, Json>,
        custom_type_key: &str,
    ) -> Vec<RawProperty> {
        match object.get("properties") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(entries)) => {
                let mut out = Vec::with_capacity(entries.len());
                for entry in entries {
                    match entry.as_object() {
                        Some(entry) => {
                            self.check_keys(
                                entry,
                                "a property",
                                &["name", "type", "propertytype", "propertyType", "value"],
                            );
                            out.push(typed_property(entry, custom_type_key));
                        }
                        None => self.warnings.record(
                            WarningKind::InvalidValue,
                            format!(
                                "a property is {}, not an object; it is ignored",
                                shown(entry)
                            ),
                        ),
                    }
                }
                out
            }
            Some(values @ Json::Object(_)) => {
                self.warnings.record(
                    WarningKind::LegacyShape,
                    "properties are an object of values (Tiled 1.0 and 1.1); they become a list",
                );
                legacy_properties(values, object.get("propertytypes"))
            }
            Some(other) => {
                self.warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "“properties” holds {}, not a list; it is ignored",
                        shown(other)
                    ),
                );
                Vec::new()
            }
        }
    }
}

/// A typed property entry or list item (`{"name", "type", "propertytype", "value"}`).
pub(crate) fn typed_property(
    entry: &JsonObject<String, Json>,
    custom_type_key: &str,
) -> RawProperty {
    let text = |key: &str| entry.get(key).and_then(Json::as_str).map(str::to_owned);
    RawProperty {
        name: text("name").unwrap_or_default(),
        type_name: text("type"),
        custom_type: text(custom_type_key).or_else(|| text("propertytype")),
        value: entry
            .get("value")
            .map_or(RawValue::Absent, |v| raw_value(v, custom_type_key)),
    }
}

/// The properties of Tiled 1.0 and 1.1: names to values, with their types beside them.
fn legacy_properties(values: &Json, types: Option<&Json>) -> Vec<RawProperty> {
    let Some(values) = values.as_object() else {
        return Vec::new();
    };
    let types = types.and_then(Json::as_object);
    values
        .iter()
        .map(|(name, value)| RawProperty {
            name: name.clone(),
            type_name: types
                .and_then(|t| t.get(name))
                .and_then(Json::as_str)
                .map(str::to_owned),
            custom_type: None,
            value: raw_value(value, "propertytype"),
        })
        .collect()
}

/// A property value as written: arrays are typed list items, objects untyped class members.
fn raw_value(value: &Json, custom_type_key: &str) -> RawValue {
    match value {
        Json::Null => RawValue::Absent,
        Json::Array(items) => RawValue::Items(
            items
                .iter()
                .map(|item| match item.as_object() {
                    Some(entry) if entry.contains_key("type") || entry.contains_key("value") => {
                        typed_property(entry, custom_type_key)
                    }
                    _ => RawProperty {
                        value: raw_value(item, custom_type_key),
                        ..RawProperty::default()
                    },
                })
                .collect(),
        ),
        Json::Object(members) => RawValue::Members(
            members
                .iter()
                .map(|(name, member)| RawProperty {
                    name: name.clone(),
                    value: raw_value(member, custom_type_key),
                    ..RawProperty::default()
                })
                .collect(),
        ),
        scalar => scalar_of(scalar).map_or(RawValue::Absent, RawValue::Scalar),
    }
}

/// A JSON scalar as a raw value; `None` for `null`, arrays and objects.
fn scalar_of(value: &Json) -> Option<Scalar> {
    match value {
        Json::String(text) => Some(Scalar::Text(text.clone())),
        Json::Bool(value) => Some(Scalar::Bool(*value)),
        Json::Number(number) => Some(match number.as_i64() {
            Some(int) => Scalar::Int(int),
            None => Scalar::Real(number.as_f64().unwrap_or(0.0)),
        }),
        Json::Null | Json::Array(_) | Json::Object(_) => None,
    }
}

/// A value shown briefly in a message.
fn shown(value: &Json) -> String {
    let text = value.to_string();
    if text.chars().count() > 40 {
        format!("{}…", text.chars().take(40).collect::<String>())
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (RawMap, Vec<String>) {
        let mut warnings = Warnings::new(Path::new("/m.tmj"));
        let value = parse_document(text, Path::new("/m.tmj")).unwrap();
        let map = read_map(&value, Path::new("/m.tmj"), &mut warnings).unwrap();
        (
            map,
            warnings
                .into_list()
                .into_iter()
                .map(|w| w.message)
                .collect(),
        )
    }

    #[test]
    fn json_maps_become_raw_values() {
        let (map, warnings) = read(
            r#"{"type":"map","version":1.2,"width":2,"height":1,"glow":1,
                "properties":[{"name":"c","type":"class","propertytype":"Spawn","value":{"hp":5}}],
                "tilesets":[{"firstgid":1,"source":"t.tsj"}],
                "layers":[{"type":"tilelayer","id":1,"name":"g","width":2,"height":1,"data":[1,-2147483647]},
                          {"type":"objectgroup","objects":[{"id":3,"ellipse":true,"x":1.5}]},
                          {"type":"weird"}]}"#,
        );
        assert_eq!(map.version, Some(Scalar::Real(1.2)));
        let RawValue::Members(members) = &map.properties[0].value else {
            panic!()
        };
        assert_eq!(
            (members[0].name.as_str(), members[0].type_name.as_ref()),
            ("hp", None)
        );
        let data = map.layers[0].data.as_ref().unwrap();
        assert_eq!(data.encoding, Some(Scalar::text("csv")));
        assert_eq!(data.payload, RawPayload::Gids(vec![1, 0x8000_0001]));
        assert_eq!(map.layers[1].objects[0].shape, Some(RawShape::Ellipse));
        assert_eq!(map.layers.len(), 2);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn the_first_json_shapes_are_read() {
        let (map, warnings) = read(
            r#"{"tilesets":[{"firstgid":1,"name":"t","tilewidth":8,"tileheight":8,
                 "tiles":{"3":{"image":"a.png"},"1":{"terrain":[0,-1,0,0]}},
                 "tileproperties":{"1":{"hp":"3"},"7":{"x":true}},
                 "tilepropertytypes":{"1":{"hp":"int"}}}],
                "properties":{"speed":2},"propertytypes":{"speed":"float"}}"#,
        );
        let ids: Vec<String> = map.tilesets[0]
            .tiles
            .iter()
            .map(|t| t.id.as_ref().unwrap().to_text())
            .collect();
        assert_eq!(ids, ["1", "3", "7"]);
        assert_eq!(
            map.tilesets[0].tiles[0].properties[0].type_name.as_deref(),
            Some("int")
        );
        assert_eq!(map.properties[0].type_name.as_deref(), Some("float"));
        assert_eq!(warnings.len(), 3);
        let mut warnings = Warnings::new(Path::new("/m.tmj"));
        let wrong = parse_document(r#"{"type":"tileset"}"#, Path::new("/m.tmj")).unwrap();
        assert!(read_map(&wrong, Path::new("/m.tmj"), &mut warnings).is_err());
    }
}
