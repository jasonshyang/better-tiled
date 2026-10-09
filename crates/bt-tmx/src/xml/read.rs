//! XML text into the raw tree: elements and attributes as written, unknown ones reported.

use std::path::Path;

use roxmltree::{Document, Node};

use crate::error::Error;
use crate::numbers::parse_gid;
use crate::raw::{
    RawChunk, RawData, RawExport, RawFrame, RawGrid, RawImage, RawImageData, RawLayer,
    RawLayerKind, RawMap, RawObject, RawPayload, RawPoint, RawProperty, RawShape, RawSize,
    RawTemplate, RawTerrain, RawText, RawTile, RawTileset, RawTransformations, RawValue,
    RawWangColor, RawWangId, RawWangSet, RawWangTile, Scalar, WangColorRole,
};
use crate::warning::{WarningKind, Warnings};

/// Parses XML text; a `DOCTYPE` (written by Tiled before 1.0) is allowed.
pub(crate) fn parse_document<'i>(text: &'i str, path: &Path) -> Result<Document<'i>, Error> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    Document::parse_with_options(text, options).map_err(|source| Error::Xml {
        path: path.to_path_buf(),
        source,
    })
}

/// The attributes every layer element has.
const LAYER_ATTRIBUTES: [&str; 16] = [
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
];

/// Reads elements into the raw tree, recording what it does not know.
pub(crate) struct XmlReader<'w> {
    warnings: &'w mut Warnings,
}

impl<'w> XmlReader<'w> {
    /// A reader recording into `warnings`.
    pub(crate) fn new(warnings: &'w mut Warnings) -> XmlReader<'w> {
        XmlReader { warnings }
    }

    /// Records the attributes of `node` that are not in `known`.
    fn check_attributes(&mut self, node: Node, known: &[&str]) {
        for attribute in node.attributes() {
            if !known.contains(&attribute.name()) {
                self.warnings.record(
                    WarningKind::UnknownKey,
                    format!(
                        "unknown attribute {}=\"{}\" on <{}>; it is not kept",
                        attribute.name(),
                        attribute.value(),
                        node.tag_name().name()
                    ),
                );
            }
        }
    }

    /// Records an element that its parent does not have.
    fn record_unknown_element(&mut self, parent: Node, child: Node) {
        self.warnings.record(
            WarningKind::UnknownKey,
            format!(
                "unknown element <{}> in <{}>; it is not kept",
                child.tag_name().name(),
                parent.tag_name().name()
            ),
        );
    }

    /// The root `<map>`.
    pub(crate) fn read_map(&mut self, node: Node) -> RawMap {
        self.check_attributes(
            node,
            &[
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
            ],
        );
        let mut map = RawMap {
            version: attr(node, "version"),
            tiled_version: attr(node, "tiledversion"),
            class: attr(node, "class"),
            orientation: attr(node, "orientation"),
            render_order: attr(node, "renderorder"),
            compression_level: attr(node, "compressionlevel"),
            width: attr(node, "width"),
            height: attr(node, "height"),
            tile_width: attr(node, "tilewidth"),
            tile_height: attr(node, "tileheight"),
            infinite: attr(node, "infinite"),
            hex_side_length: attr(node, "hexsidelength"),
            stagger_axis: attr(node, "staggeraxis"),
            stagger_index: attr(node, "staggerindex"),
            skew_x: attr(node, "skewx"),
            skew_y: attr(node, "skewy"),
            parallax_origin_x: attr(node, "parallaxoriginx"),
            parallax_origin_y: attr(node, "parallaxoriginy"),
            background_color: attr(node, "backgroundcolor"),
            next_layer_id: attr(node, "nextlayerid"),
            next_object_id: attr(node, "nextobjectid"),
            ..RawMap::default()
        };
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "properties" => map.properties.extend(self.read_properties(child)),
                "tileset" => map.tilesets.push(self.read_tileset(child)),
                "editorsettings" => {
                    self.check_attributes(child, &[]);
                    for setting in child.children().filter(Node::is_element) {
                        match setting.tag_name().name() {
                            "chunksize" => {
                                self.check_attributes(setting, &["width", "height"]);
                                map.chunk_size = Some(RawSize {
                                    width: attr(setting, "width"),
                                    height: attr(setting, "height"),
                                });
                            }
                            "export" => map.export = Some(self.read_export(setting)),
                            _ => self.record_unknown_element(child, setting),
                        }
                    }
                }
                _ => match self.read_layer(child) {
                    Some(layer) => map.layers.push(layer),
                    None => self.record_unknown_element(node, child),
                },
            }
        }
        map
    }

    fn read_export(&mut self, node: Node) -> RawExport {
        self.check_attributes(node, &["target", "format"]);
        RawExport {
            target: attr(node, "target"),
            format: attr(node, "format"),
        }
    }

    /// A `<tileset>`: a reference, an embedded tileset or a tileset file's root.
    pub(crate) fn read_tileset(&mut self, node: Node) -> RawTileset {
        self.check_attributes(
            node,
            &[
                "firstgid",
                "source",
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
            ],
        );
        let mut tileset = RawTileset {
            first_gid: attr(node, "firstgid"),
            source: attr(node, "source"),
            version: attr(node, "version"),
            tiled_version: attr(node, "tiledversion"),
            name: attr(node, "name"),
            class: attr(node, "class"),
            tile_width: attr(node, "tilewidth"),
            tile_height: attr(node, "tileheight"),
            spacing: attr(node, "spacing"),
            margin: attr(node, "margin"),
            tile_count: attr(node, "tilecount"),
            columns: attr(node, "columns"),
            background_color: attr(node, "backgroundcolor"),
            object_alignment: attr(node, "objectalignment"),
            tile_render_size: attr(node, "tilerendersize"),
            fill_mode: attr(node, "fillmode"),
            ..RawTileset::default()
        };
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "editorsettings" => {
                    self.check_attributes(child, &[]);
                    for setting in child.children().filter(Node::is_element) {
                        match setting.tag_name().name() {
                            "export" => tileset.export = Some(self.read_export(setting)),
                            _ => self.record_unknown_element(child, setting),
                        }
                    }
                }
                "tileoffset" => {
                    self.check_attributes(child, &["x", "y"]);
                    tileset.tile_offset = Some(RawPoint {
                        x: attr(child, "x"),
                        y: attr(child, "y"),
                    });
                }
                "grid" => {
                    self.check_attributes(child, &["orientation", "width", "height"]);
                    tileset.grid = Some(RawGrid {
                        orientation: attr(child, "orientation"),
                        width: attr(child, "width"),
                        height: attr(child, "height"),
                    });
                }
                "transformations" => {
                    self.check_attributes(
                        child,
                        &["hflip", "vflip", "rotate", "preferuntransformed"],
                    );
                    tileset.transformations = Some(RawTransformations {
                        hflip: attr(child, "hflip"),
                        vflip: attr(child, "vflip"),
                        rotate: attr(child, "rotate"),
                        prefer_untransformed: attr(child, "preferuntransformed"),
                    });
                }
                "properties" => tileset.properties.extend(self.read_properties(child)),
                "image" => tileset.image = Some(self.read_image(child)),
                "tile" => tileset.tiles.push(self.read_tile(child)),
                "terraintypes" => {
                    self.check_attributes(child, &[]);
                    for terrain in child.children().filter(Node::is_element) {
                        if terrain.has_tag_name("terrain") {
                            tileset.terrains.push(self.read_terrain(terrain));
                        } else {
                            self.record_unknown_element(child, terrain);
                        }
                    }
                }
                "wangsets" => {
                    self.check_attributes(child, &[]);
                    for set in child.children().filter(Node::is_element) {
                        if set.has_tag_name("wangset") {
                            tileset.wang_sets.push(self.read_wang_set(set));
                        } else {
                            self.record_unknown_element(child, set);
                        }
                    }
                }
                _ => self.record_unknown_element(node, child),
            }
        }
        tileset
    }

    fn read_image(&mut self, node: Node) -> RawImage {
        self.check_attributes(node, &["source", "trans", "width", "height", "format"]);
        let mut image = RawImage {
            source: attr(node, "source"),
            trans: attr(node, "trans"),
            width: attr(node, "width"),
            height: attr(node, "height"),
            format: attr(node, "format"),
            data: None,
        };
        for child in node.children().filter(Node::is_element) {
            if child.has_tag_name("data") {
                self.check_attributes(child, &["encoding"]);
                image.data = Some(RawImageData {
                    encoding: attr(child, "encoding"),
                    text: direct_text(child),
                });
            } else {
                self.record_unknown_element(node, child);
            }
        }
        image
    }

    fn read_tile(&mut self, node: Node) -> RawTile {
        self.check_attributes(
            node,
            &[
                "id",
                "x",
                "y",
                "width",
                "height",
                "class",
                "type",
                "terrain",
                "probability",
            ],
        );
        let mut tile = RawTile {
            id: attr(node, "id"),
            class: attr(node, "class"),
            class_as_type: attr(node, "type"),
            probability: attr(node, "probability"),
            x: attr(node, "x"),
            y: attr(node, "y"),
            width: attr(node, "width"),
            height: attr(node, "height"),
            terrain: node.attribute("terrain").map(|text| {
                text.split(',')
                    .map(|corner| (!corner.trim().is_empty()).then(|| Scalar::text(corner)))
                    .collect()
            }),
            ..RawTile::default()
        };
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "properties" => tile.properties.extend(self.read_properties(child)),
                "image" => tile.image = Some(self.read_image(child)),
                "objectgroup" => tile.collision = self.read_layer(child),
                "animation" => {
                    self.check_attributes(child, &[]);
                    let mut frames = Vec::new();
                    for frame in child.children().filter(Node::is_element) {
                        if frame.has_tag_name("frame") {
                            self.check_attributes(frame, &["tileid", "duration"]);
                            frames.push(RawFrame {
                                tile_id: attr(frame, "tileid"),
                                duration: attr(frame, "duration"),
                            });
                        } else {
                            self.record_unknown_element(child, frame);
                        }
                    }
                    tile.animation = Some(frames);
                }
                _ => self.record_unknown_element(node, child),
            }
        }
        tile
    }

    fn read_terrain(&mut self, node: Node) -> RawTerrain {
        self.check_attributes(node, &["name", "tile"]);
        let mut terrain = RawTerrain {
            name: attr(node, "name"),
            tile: attr(node, "tile"),
            properties: Vec::new(),
        };
        for child in node.children().filter(Node::is_element) {
            if child.has_tag_name("properties") {
                terrain.properties.extend(self.read_properties(child));
            } else {
                self.record_unknown_element(node, child);
            }
        }
        terrain
    }

    fn read_wang_set(&mut self, node: Node) -> RawWangSet {
        self.check_attributes(node, &["name", "class", "type", "tile"]);
        let mut set = RawWangSet {
            name: attr(node, "name"),
            class: attr(node, "class"),
            kind: attr(node, "type"),
            tile: attr(node, "tile"),
            ..RawWangSet::default()
        };
        for child in node.children().filter(Node::is_element) {
            let role = match child.tag_name().name() {
                "properties" => {
                    set.properties.extend(self.read_properties(child));
                    continue;
                }
                "wangtile" => {
                    // Tiled 1.1 to 1.4 wrote flips the terrain filler could use.
                    self.check_attributes(child, &["tileid", "wangid", "hflip", "vflip", "dflip"]);
                    set.tiles.push(RawWangTile {
                        tile_id: attr(child, "tileid"),
                        wang_id: child
                            .attribute("wangid")
                            .map(|text| RawWangId::Text(text.to_owned())),
                    });
                    continue;
                }
                "wangcolor" => WangColorRole::Any,
                "wangcornercolor" => WangColorRole::Corner,
                "wangedgecolor" => WangColorRole::Edge,
                _ => {
                    self.record_unknown_element(node, child);
                    continue;
                }
            };
            self.check_attributes(child, &["name", "class", "color", "tile", "probability"]);
            let mut color = RawWangColor {
                role,
                name: attr(child, "name"),
                class: attr(child, "class"),
                color: attr(child, "color"),
                tile: attr(child, "tile"),
                probability: attr(child, "probability"),
                properties: Vec::new(),
            };
            for grandchild in child.children().filter(Node::is_element) {
                if grandchild.has_tag_name("properties") {
                    color.properties.extend(self.read_properties(grandchild));
                } else {
                    self.record_unknown_element(child, grandchild);
                }
            }
            set.colors.push(color);
        }
        set
    }

    /// A layer element; `None` for an element that is not one.
    fn read_layer(&mut self, node: Node) -> Option<RawLayer> {
        let kind = match node.tag_name().name() {
            "layer" => RawLayerKind::Tile,
            "objectgroup" => RawLayerKind::Object,
            "imagelayer" => RawLayerKind::Image,
            "group" => RawLayerKind::Group,
            _ => return None,
        };
        let extra: &[&str] = match kind {
            RawLayerKind::Object => &["color", "draworder"],
            RawLayerKind::Image => &["repeatx", "repeaty"],
            RawLayerKind::Tile | RawLayerKind::Group => &[],
        };
        let known: Vec<&str> = LAYER_ATTRIBUTES.iter().chain(extra).copied().collect();
        self.check_attributes(node, &known);
        let mut layer = RawLayer {
            kind,
            id: attr(node, "id"),
            name: attr(node, "name"),
            class: attr(node, "class"),
            x: attr(node, "x"),
            y: attr(node, "y"),
            width: attr(node, "width"),
            height: attr(node, "height"),
            visible: attr(node, "visible"),
            locked: attr(node, "locked"),
            opacity: attr(node, "opacity"),
            tint_color: attr(node, "tintcolor"),
            offset_x: attr(node, "offsetx"),
            offset_y: attr(node, "offsety"),
            parallax_x: attr(node, "parallaxx"),
            parallax_y: attr(node, "parallaxy"),
            mode: attr(node, "mode"),
            color: attr(node, "color"),
            draw_order: attr(node, "draworder"),
            repeat_x: attr(node, "repeatx"),
            repeat_y: attr(node, "repeaty"),
            ..RawLayer::default()
        };
        for child in node.children().filter(Node::is_element) {
            match (kind, child.tag_name().name()) {
                (_, "properties") => layer.properties.extend(self.read_properties(child)),
                (RawLayerKind::Tile, "data") => layer.data = Some(self.read_data(child)),
                (RawLayerKind::Object, "object") => layer.objects.push(self.read_object(child)),
                (RawLayerKind::Image, "image") => layer.image = Some(self.read_image(child)),
                (RawLayerKind::Group, _) => match self.read_layer(child) {
                    Some(inner) => layer.layers.push(inner),
                    None => self.record_unknown_element(node, child),
                },
                _ => self.record_unknown_element(node, child),
            }
        }
        Some(layer)
    }

    fn read_data(&mut self, node: Node) -> RawData {
        self.check_attributes(node, &["encoding", "compression"]);
        let mut data = RawData {
            encoding: attr(node, "encoding"),
            compression: attr(node, "compression"),
            payload: self.read_payload(node),
            chunks: Vec::new(),
            chunk_bounds: None,
        };
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "tile" => {}
                "chunk" => {
                    self.check_attributes(child, &["x", "y", "width", "height"]);
                    data.chunks.push(RawChunk {
                        x: attr(child, "x"),
                        y: attr(child, "y"),
                        width: attr(child, "width"),
                        height: attr(child, "height"),
                        payload: self.read_payload(child),
                    });
                }
                _ => self.record_unknown_element(node, child),
            }
        }
        data
    }

    /// The cells of a `<data>` or `<chunk>`: its `<tile>` elements, or its text.
    fn read_payload(&mut self, node: Node) -> RawPayload {
        let tiles: Vec<Node> = node
            .children()
            .filter(|child| child.has_tag_name("tile"))
            .collect();
        if !tiles.is_empty() {
            let mut gids = Vec::with_capacity(tiles.len());
            for tile in tiles {
                self.check_attributes(tile, &["gid"]);
                let gid = match tile.attribute("gid") {
                    None => 0,
                    Some(text) => parse_gid(text).unwrap_or_else(|| {
                        self.warnings.record(
                            WarningKind::CorruptLayerData,
                            format!("<tile gid=\"{text}\"> is not a tile id; the cell is empty"),
                        );
                        0
                    }),
                };
                gids.push(gid);
            }
            return RawPayload::Gids(gids);
        }
        let text = direct_text(node);
        if text.trim().is_empty() {
            RawPayload::Empty
        } else {
            RawPayload::Text(text)
        }
    }

    /// An `<object>`.
    pub(crate) fn read_object(&mut self, node: Node) -> RawObject {
        self.check_attributes(
            node,
            &[
                "id", "name", "class", "type", "gid", "x", "y", "width", "height", "rotation",
                "opacity", "visible", "template",
            ],
        );
        let mut object = RawObject {
            id: attr(node, "id"),
            name: attr(node, "name"),
            class: attr(node, "class"),
            class_as_type: attr(node, "type"),
            gid: attr(node, "gid"),
            x: attr(node, "x"),
            y: attr(node, "y"),
            width: attr(node, "width"),
            height: attr(node, "height"),
            rotation: attr(node, "rotation"),
            opacity: attr(node, "opacity"),
            visible: attr(node, "visible"),
            template: attr(node, "template"),
            ..RawObject::default()
        };
        for child in node.children().filter(Node::is_element) {
            let shape = match child.tag_name().name() {
                "properties" => {
                    object.properties.extend(self.read_properties(child));
                    continue;
                }
                "ellipse" => RawShape::Ellipse,
                "point" => RawShape::Point,
                "capsule" => RawShape::Capsule,
                "polygon" => RawShape::Polygon(self.read_points(child)),
                "polyline" => RawShape::Polyline(self.read_points(child)),
                "text" => RawShape::Text(Box::new(self.read_text(child))),
                _ => {
                    self.record_unknown_element(node, child);
                    continue;
                }
            };
            if matches!(
                shape,
                RawShape::Ellipse | RawShape::Point | RawShape::Capsule
            ) {
                self.check_attributes(child, &[]);
            }
            object.shape = Some(shape);
        }
        object
    }

    /// The `points` of a `<polygon>` or `<polyline>`: `x,y` pairs separated by spaces.
    fn read_points(&mut self, node: Node) -> Vec<RawPoint> {
        self.check_attributes(node, &["points"]);
        let mut points = Vec::new();
        for pair in node.attribute("points").unwrap_or("").split_whitespace() {
            match pair.split_once(',') {
                Some((x, y)) => points.push(RawPoint {
                    x: Some(Scalar::text(x)),
                    y: Some(Scalar::text(y)),
                }),
                None => self.warnings.record(
                    WarningKind::InvalidValue,
                    format!(
                        "the point “{pair}” of a <{}> is not x,y; it was left out",
                        node.tag_name().name()
                    ),
                ),
            }
        }
        points
    }

    fn read_text(&mut self, node: Node) -> RawText {
        self.check_attributes(
            node,
            &[
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
            font_family: attr(node, "fontfamily"),
            pixel_size: attr(node, "pixelsize"),
            wrap: attr(node, "wrap"),
            color: attr(node, "color"),
            bold: attr(node, "bold"),
            italic: attr(node, "italic"),
            underline: attr(node, "underline"),
            strikeout: attr(node, "strikeout"),
            kerning: attr(node, "kerning"),
            halign: attr(node, "halign"),
            valign: attr(node, "valign"),
            text: direct_text(node),
        }
    }

    /// The root `<template>`.
    pub(crate) fn read_template(&mut self, node: Node) -> RawTemplate {
        self.check_attributes(node, &[]);
        let mut template = RawTemplate::default();
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "tileset" => template.tilesets.push(self.read_tileset(child)),
                "object" => template.object = Some(self.read_object(child)),
                _ => self.record_unknown_element(node, child),
            }
        }
        template
    }

    /// A `<properties>` element's properties, in file order.
    pub(crate) fn read_properties(&mut self, node: Node) -> Vec<RawProperty> {
        self.check_attributes(node, &[]);
        let mut properties = Vec::new();
        for child in node.children().filter(Node::is_element) {
            if child.has_tag_name("property") {
                properties.push(self.read_property(child, true));
            } else {
                self.record_unknown_element(node, child);
            }
        }
        properties
    }

    /// A `<property>` or a list `<item>`.
    fn read_property(&mut self, node: Node, named: bool) -> RawProperty {
        let known: &[&str] = if named {
            &["name", "type", "propertytype", "value"]
        } else {
            &["type", "propertytype", "value"]
        };
        self.check_attributes(node, known);
        let type_name = node.attribute("type").map(str::to_owned);
        let mut members = None;
        let mut items = Vec::new();
        for child in node.children().filter(Node::is_element) {
            match child.tag_name().name() {
                "properties" => members = Some(self.read_properties(child)),
                "item" => items.push(self.read_property(child, false)),
                _ => self.record_unknown_element(node, child),
            }
        }
        let value = if type_name.as_deref() == Some("list") || !items.is_empty() {
            RawValue::Items(items)
        } else if let Some(members) = members {
            RawValue::Members(members)
        } else {
            // The `value` attribute, else the element's text when it is not only whitespace
            // (a string with line breaks), as Tiled reads it.
            let text = direct_text(node);
            match node.attribute("value") {
                Some(value) if !value.is_empty() => RawValue::Scalar(Scalar::text(value)),
                _ if !text.trim().is_empty() => RawValue::Scalar(Scalar::Text(text)),
                Some(value) => RawValue::Scalar(Scalar::text(value)),
                None => RawValue::Scalar(Scalar::text("")),
            }
        };
        RawProperty {
            name: if named {
                node.attribute("name").unwrap_or("").to_owned()
            } else {
                String::new()
            },
            type_name,
            custom_type: node.attribute("propertytype").map(str::to_owned),
            value,
        }
    }
}

/// An attribute as written.
fn attr(node: Node, name: &str) -> Option<Scalar> {
    node.attribute(name).map(Scalar::text)
}

/// The text of an element itself (text and CDATA, not its descendants').
fn direct_text(node: Node) -> String {
    node.children()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (RawMap, Vec<String>) {
        let mut warnings = Warnings::new(Path::new("/m.tmx"));
        let document = parse_document(text, Path::new("/m.tmx")).unwrap();
        let map = XmlReader::new(&mut warnings).read_map(document.root_element());
        let messages = warnings
            .into_list()
            .into_iter()
            .map(|w| w.message)
            .collect();
        (map, messages)
    }

    #[test]
    fn elements_become_raw_values() {
        let (map, warnings) = read(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE map SYSTEM "http://mapeditor.org/dtd/1.0/map.dtd">
<map version="1.10" orientation="orthogonal" width="2" height="1" tilewidth="8" tileheight="8" glow="1">
 <properties>
  <property name="note">two
lines</property>
  <property name="n" type="int" value="3"/>
  <property name="l" type="list"><item type="int" value="1"/><item value="x"/></property>
  <property name="c" type="class" propertytype="Spawn"><properties><property name="hp" type="int" value="5"/></properties></property>
 </properties>
 <tileset firstgid="1" source="t.tsx"/>
 <layer id="1" name="ground" width="2" height="1">
  <data encoding="csv">
1,2
</data>
 </layer>
 <objectgroup id="2" name="things" draworder="index">
  <object id="1" x="1.5" y="2"><polygon points="0,0 4,0 nope"/></object>
 </objectgroup>
 <unknown/>
</map>"#,
        );
        assert_eq!(map.version, Some(Scalar::text("1.10")));
        assert_eq!(map.properties.len(), 4);
        assert_eq!(
            map.properties[0].value,
            RawValue::Scalar(Scalar::text("two\nlines"))
        );
        let RawValue::Items(items) = &map.properties[2].value else {
            panic!()
        };
        assert_eq!((items.len(), items[1].type_name.as_deref()), (2, None));
        let RawValue::Members(members) = &map.properties[3].value else {
            panic!()
        };
        assert_eq!(members[0].name, "hp");
        assert_eq!(map.tilesets[0].source, Some(Scalar::text("t.tsx")));
        let data = map.layers[0].data.as_ref().unwrap();
        assert_eq!(data.payload, RawPayload::Text("\n1,2\n".into()));
        let RawShape::Polygon(points) = map.layers[1].objects[0].shape.as_ref().unwrap() else {
            panic!()
        };
        assert_eq!(points.len(), 2);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].contains("glow"));
        assert!(warnings[1].contains("nope"));
        assert!(warnings[2].contains("<unknown>"));
    }

    #[test]
    fn xml_tile_elements_are_gids() {
        let (map, warnings) = read(
            r#"<map><layer><data><tile gid="3"/><tile/><tile gid="-2147483647"/><tile gid="x"/></data></layer></map>"#,
        );
        assert_eq!(
            map.layers[0].data.as_ref().unwrap().payload,
            RawPayload::Gids(vec![3, 0, 0x8000_0001, 0])
        );
        assert_eq!(warnings.len(), 1);
    }
}
