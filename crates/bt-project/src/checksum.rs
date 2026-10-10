//! Checksums of layers as saved in a map: the change detectors that tell whether a painted
//! entry still applies to the map's layer. Each is FNV-1a (`bt_utils::hash`) over a canonical
//! byte stream, written as sixteen hex digits. A checksum detects changes; it is not a security
//! measure.
//!
//! The stream names tilesets and templates by their keys (`crate::paths::tileset_key`), never by
//! their place in the map, so the order of the map's tilesets changes no checksum. Numbers are
//! little-endian, texts are their length and then their bytes, and reals are their decimal form
//! with six significant digits: the precision Tiled's XML keeps, so a layer checksummed in memory
//! before a save and the same layer read back from the file, in either format, agree.
//!
//! The stream is part of the file format: changing it makes every painted entry already saved
//! stop applying, which the pinned values in the tests guard against.

use std::hash::Hasher;

use bt_core::{Cell, Color, HAlign, Map, MapObject, Properties, Property, Shape, TextData};
use bt_core::{TileLayer, VAlign, Value};
use bt_utils::hash::Fnv64;

use crate::paths::{reference_key, tileset_key};

/// The checksum of `layer`, a tile layer of `map`: its size and its non-empty cells in row
/// order (rows from the top, each from the left), each as its position, its tileset's key, its
/// tile id and its flips.
pub fn tile_layer(map: &Map, layer: &TileLayer) -> String {
    let keys = tileset_keys(map);
    let mut stream = Canonical::new("tile layer");
    stream.u32(layer.size().x);
    stream.u32(layer.size().y);
    let mut cells: Vec<_> = layer.cells().collect();
    cells.sort_by_key(|placed| (placed.at.y, placed.at.x));
    for placed in cells {
        stream.i32(placed.at.x);
        stream.i32(placed.at.y);
        stream.cell(&keys, placed.cell);
    }
    stream.finish()
}

/// The checksum of `objects`, the objects of an object layer of `map`, in their order: each
/// one's id, name, class, position, size, rotation, visibility, opacity, tile (by its tileset's
/// key), shape (with a polygon's points and a text's text and settings), template, the fields it
/// sets apart from its template, and properties: everything editing in Tiled can change.
pub fn objects(map: &Map, objects: &[MapObject]) -> String {
    let keys = tileset_keys(map);
    let mut stream = Canonical::new("objects");
    for object in objects {
        stream.object(&keys, object);
    }
    stream.finish()
}

/// The keys of the map's tilesets, by slot.
fn tileset_keys(map: &Map) -> Vec<String> {
    map.tilesets.iter().map(tileset_key).collect()
}

/// The canonical byte stream of a checksum, hashed as it is written.
struct Canonical {
    hash: Fnv64,
}

impl Canonical {
    /// A stream that starts with what it is a checksum of, so two kinds never share one.
    fn new(kind: &str) -> Canonical {
        let mut stream = Canonical { hash: Fnv64::new() };
        stream.text(kind);
        stream
    }

    /// The checksum: the hash as sixteen lowercase hex digits.
    fn finish(self) -> String {
        format!("{:016x}", self.hash.finish())
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.hash.write(bytes);
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }

    /// How many things follow.
    fn count(&mut self, count: usize) {
        // usize is at most 64 bits on every platform Rust supports.
        self.bytes(&(count as u64).to_le_bytes());
    }

    fn text(&mut self, text: &str) {
        self.count(text.len());
        self.bytes(text.as_bytes());
    }

    fn optional_text(&mut self, text: Option<&str>) {
        match text {
            None => self.u8(0),
            Some(text) => {
                self.u8(1);
                self.text(text);
            }
        }
    }

    /// A real as its decimal form with six significant digits (`-0` as `0`), as Tiled's XML
    /// writes it, so rounding on the way to the file changes nothing.
    fn real(&mut self, value: f64) {
        let value = if value == 0.0 { 0.0 } else { value };
        self.text(&format!("{value:.5e}"));
    }

    fn color(&mut self, color: Color) {
        self.bytes(&[color.r, color.g, color.b, color.a]);
    }

    /// A cell as its tileset's key, its tile id and its flips. A cell whose slot names no
    /// tileset of the map has no key.
    fn cell(&mut self, keys: &[String], cell: Cell) {
        let key = cell.slot().and_then(|slot| keys.get(slot.index()));
        self.optional_text(key.map(String::as_str));
        self.u32(cell.tile().0);
        self.u8(cell.flip().bits());
    }

    fn object(&mut self, keys: &[String], object: &MapObject) {
        self.u32(object.id.0);
        self.text(&object.name);
        self.text(&object.class);
        self.real(object.pos.x);
        self.real(object.pos.y);
        self.real(object.size.x);
        self.real(object.size.y);
        self.real(object.rotation);
        self.bool(object.visible);
        self.real(object.opacity);
        match object.tile {
            None => self.u8(0),
            Some(cell) => {
                self.u8(1);
                self.cell(keys, cell);
            }
        }
        self.shape(&object.shape);
        self.optional_text(object.template.as_ref().map(reference_key).as_deref());
        self.u16(object.overrides.bits());
        self.properties(&object.properties);
    }

    fn shape(&mut self, shape: &Shape) {
        match shape {
            Shape::Rectangle => self.u8(0),
            Shape::Ellipse => self.u8(1),
            Shape::Point => self.u8(2),
            Shape::Capsule => self.u8(3),
            Shape::Polygon(points) | Shape::Polyline(points) => {
                let closed = matches!(shape, Shape::Polygon(_));
                self.u8(if closed { 4 } else { 5 });
                self.count(points.len());
                for point in points {
                    self.real(point.x);
                    self.real(point.y);
                }
            }
            Shape::Text(text) => {
                self.u8(6);
                self.text_data(text);
            }
        }
    }

    fn text_data(&mut self, text: &TextData) {
        self.text(&text.text);
        self.text(&text.font_family);
        self.u32(text.pixel_size);
        self.bool(text.wrap);
        self.color(text.color);
        self.bool(text.bold);
        self.bool(text.italic);
        self.bool(text.underline);
        self.bool(text.strikeout);
        self.bool(text.kerning);
        self.u8(match text.halign {
            HAlign::Left => 0,
            HAlign::Center => 1,
            HAlign::Right => 2,
            HAlign::Justify => 3,
        });
        self.u8(match text.valign {
            VAlign::Top => 0,
            VAlign::Center => 1,
            VAlign::Bottom => 2,
        });
    }

    fn properties(&mut self, properties: &Properties) {
        self.count(properties.len());
        for (name, property) in properties {
            self.text(name);
            self.property(property);
        }
    }

    fn property(&mut self, property: &Property) {
        self.optional_text(property.custom_type.as_deref());
        match &property.value {
            Value::String(text) => {
                self.u8(0);
                self.text(text);
            }
            Value::Int(value) => {
                self.u8(1);
                self.i64(*value);
            }
            Value::Float(value) => {
                self.u8(2);
                self.real(*value);
            }
            Value::Bool(value) => {
                self.u8(3);
                self.bool(*value);
            }
            Value::Color(color) => {
                self.u8(4);
                match color {
                    None => self.u8(0),
                    Some(color) => {
                        self.u8(1);
                        self.color(*color);
                    }
                }
            }
            Value::File(path) => {
                self.u8(5);
                self.text(path);
            }
            Value::Object(id) => {
                self.u8(6);
                self.u32(id.0);
            }
            Value::Class(members) => {
                self.u8(7);
                self.properties(members);
            }
            Value::List(items) => {
                self.u8(8);
                self.count(items.len());
                for item in items {
                    self.property(item);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{
        FileRef, Flip, ObjectId, ObjectOverrides, TileId, Tileset, TilesetRef, TilesetSlot,
    };
    use bt_test_data::fixtures::MapBuilder;
    use glam::{DVec2, UVec2};

    use super::*;

    fn tileset(name: &str) -> Arc<Tileset> {
        Arc::new(Tileset {
            name: name.to_owned(),
            tile_width: 16,
            tile_height: 16,
            ..Tileset::default()
        })
    }

    /// `ground.tsx` in a file of its own and `Decor` embedded, in the order given.
    fn tilesets(file_first: bool) -> Vec<TilesetRef> {
        let file = TilesetRef {
            file: Some(FileRef::new(
                "../tiles/ground.tsx",
                "/game/tiles/ground.tsx",
            )),
            first_gid: None,
            tileset: tileset("ground"),
        };
        let embedded = TilesetRef::embedded(tileset("Decor"));
        if file_first {
            vec![file, embedded]
        } else {
            vec![embedded, file]
        }
    }

    /// A map with the two tilesets in the order given and one tile layer drawn from `grid`.
    fn map(file_first: bool, grid: &str) -> Map {
        let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 3))
            .tile_layer("ground", grid)
            .build()
            .unwrap();
        map.tilesets = tilesets(file_first);
        map
    }

    fn layer_checksum(map: &Map) -> String {
        tile_layer(map, map.layers[0].as_tile_layer().unwrap())
    }

    // The same cells: the file's tileset is slot 0 in the first, slot 1 in the second.
    const FILE_FIRST: &str = "1 2@1h . .\n. . . .\n. . 5d 7@1";
    const DECOR_FIRST: &str = "1@1 2h . .\n. . . .\n. . 5@1d 7";

    #[test]
    fn the_order_of_the_maps_tilesets_does_not_change_a_tile_layers_checksum() {
        let first = layer_checksum(&map(true, FILE_FIRST));
        assert_eq!(first, layer_checksum(&map(false, DECOR_FIRST)));
        assert_eq!(first.len(), 16);
        assert!(
            first
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }

    #[test]
    fn what_a_tile_layer_holds_changes_its_checksum() {
        let original = layer_checksum(&map(true, FILE_FIRST));
        for (change, grid) in [
            ("a tile moved", ". 2@1h 1 .\n. . . .\n. . 5d 7@1"),
            ("another tile", "3 2@1h . .\n. . . .\n. . 5d 7@1"),
            ("another flip", "1 2@1v . .\n. . . .\n. . 5d 7@1"),
            ("another tileset", "1@1 2@1h . .\n. . . .\n. . 5d 7@1"),
            ("a tile erased", "1 2@1h . .\n. . . .\n. . 5d ."),
            ("a tile added", "1 2@1h . .\n. 9 . .\n. . 5d 7@1"),
        ] {
            assert_ne!(layer_checksum(&map(true, grid)), original, "{change}");
        }
        let mut resized = map(true, FILE_FIRST);
        resized.layers[0] = MapBuilder::new(UVec2::new(16, 16), UVec2::new(5, 3))
            .tile_layer("ground", FILE_FIRST)
            .build()
            .unwrap()
            .layers
            .remove(0);
        assert_ne!(layer_checksum(&resized), original, "another size");
    }

    #[test]
    fn cells_are_taken_in_row_order_whatever_chunks_they_are_kept_in() {
        // A layer keeps its cells in 16×16 chunks, row of chunks by row of chunks: the cell at
        // (20, 0) is in the chunk after those holding (-3, 1) and (0, 1), but its row comes first.
        let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
            .infinite()
            .tile_layer_at("far", glam::IVec2::new(-3, 1), "4 . . 1@1")
            .build()
            .unwrap();
        map.tilesets = tilesets(true);
        let tile = |slot: usize, id: u32| Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id));
        let far = map.layers[0].as_tile_layer().unwrap().clone();
        let mut layer = far.clone();
        layer
            .set_cell(glam::IVec2::new(20, 0), tile(1, 9).with_flip(Flip::V))
            .unwrap();
        let chunk_order: Vec<glam::IVec2> = layer.cells().map(|placed| placed.at).collect();
        assert_eq!(
            chunk_order,
            [(-3, 1), (0, 1), (20, 0)].map(|(x, y)| glam::IVec2::new(x, y)),
            "the layer's own order"
        );
        let mut expected = Canonical::new("tile layer");
        expected.u32(4);
        expected.u32(4);
        let keys = tileset_keys(&map);
        for (x, y, cell) in [
            (20, 0, tile(1, 9).with_flip(Flip::V)),
            (-3, 1, tile(0, 4)),
            (0, 1, tile(1, 1)),
        ] {
            expected.i32(x);
            expected.i32(y);
            expected.cell(&keys, cell);
        }
        assert_eq!(tile_layer(&map, &layer), expected.finish());
        assert_ne!(tile_layer(&map, &layer), tile_layer(&map, &far));
    }

    /// An object with every field set.
    fn full_object() -> MapObject {
        let mut properties = Properties::new();
        properties.insert("speed".to_owned(), Property::new(Value::Float(1.5)));
        properties.insert(
            "spawn".to_owned(),
            Property::typed(
                Value::Class(Properties::from([(
                    "hp".to_owned(),
                    Property::new(Value::Int(5)),
                )])),
                "Spawn",
            ),
        );
        properties.insert(
            "path".to_owned(),
            Property::new(Value::List(vec![
                Property::new(Value::Object(ObjectId(3))),
                Property::new(Value::File("../a.txt".to_owned())),
                Property::new(Value::Color(Some(Color::rgba(1, 2, 3, 4)))),
                Property::new(Value::Bool(true)),
                Property::new(Value::String("x".to_owned())),
            ])),
        );
        MapObject {
            id: ObjectId(7),
            name: "door".to_owned(),
            class: "Door".to_owned(),
            pos: DVec2::new(146.96875, 32.0),
            size: DVec2::new(16.0, 24.0),
            rotation: 90.0,
            visible: true,
            opacity: 0.5,
            shape: Shape::Polygon(vec![DVec2::new(0.0, 0.0), DVec2::new(8.0, 4.0)]),
            tile: Some(
                bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(3)).with_flip(Flip::H),
            ),
            template: Some(FileRef::new("door.tx", "/game/maps/door.tx")),
            overrides: ObjectOverrides::NAME,
            properties,
        }
    }

    fn objects_checksum(objects_list: &[MapObject]) -> String {
        objects(&map(true, FILE_FIRST), objects_list)
    }

    #[test]
    fn each_object_field_tiled_can_edit_changes_the_objects_checksum() {
        type Edit = fn(&mut MapObject);
        let edits: Vec<(&str, Edit)> = vec![
            ("id", |o| o.id = ObjectId(8)),
            ("name", |o| o.name = "gate".to_owned()),
            ("class", |o| o.class = String::new()),
            ("x", |o| o.pos.x += 1.0),
            ("y", |o| o.pos.y -= 0.5),
            ("width", |o| o.size.x = 17.0),
            ("height", |o| o.size.y = 23.0),
            ("rotation", |o| o.rotation = 45.0),
            ("visibility", |o| o.visible = false),
            ("opacity", |o| o.opacity = 0.75),
            ("no tile", |o| o.tile = None),
            ("another tile", |o| {
                o.tile = Some(bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(4)))
            }),
            ("another tileset", |o| {
                o.tile = Some(
                    bt_core::Cell::new(TilesetSlot::new(1).unwrap(), TileId(3)).with_flip(Flip::H),
                )
            }),
            ("another flip", |o| {
                o.tile = o.tile.map(|c| c.with_flip(Flip::V))
            }),
            ("rectangle", |o| o.shape = Shape::Rectangle),
            ("ellipse", |o| o.shape = Shape::Ellipse),
            ("point", |o| o.shape = Shape::Point),
            ("capsule", |o| o.shape = Shape::Capsule),
            ("polyline", |o| {
                o.shape = Shape::Polyline(vec![DVec2::new(0.0, 0.0), DVec2::new(8.0, 4.0)])
            }),
            ("a point moved", |o| {
                o.shape = Shape::Polygon(vec![DVec2::new(0.0, 0.0), DVec2::new(8.0, 5.0)])
            }),
            ("a point added", |o| {
                o.shape = Shape::Polygon(vec![
                    DVec2::new(0.0, 0.0),
                    DVec2::new(8.0, 4.0),
                    DVec2::new(1.0, 1.0),
                ])
            }),
            ("text", |o| o.shape = Shape::Text(TextData::default())),
            ("no template", |o| o.template = None),
            ("another template", |o| {
                o.template = Some(FileRef::new("gate.tx", "/game/maps/gate.tx"))
            }),
            ("overrides", |o| {
                o.overrides = ObjectOverrides::NAME | ObjectOverrides::SIZE
            }),
            ("a property's value", |o| {
                o.properties
                    .insert("speed".to_owned(), Property::new(Value::Float(2.0)));
            }),
            ("a property's type", |o| {
                o.properties
                    .insert("speed".to_owned(), Property::new(Value::Int(1)));
            }),
            ("a property's custom type", |o| {
                o.properties.insert(
                    "speed".to_owned(),
                    Property::typed(Value::Float(1.5), "Speed"),
                );
            }),
            ("a property removed", |o| {
                o.properties.remove("speed");
            }),
            ("a property renamed", |o| {
                let speed = o.properties.remove("speed").unwrap();
                o.properties.insert("pace".to_owned(), speed);
            }),
            ("a class member", |o| {
                if let Some(Property {
                    value: Value::Class(members),
                    ..
                }) = o.properties.get_mut("spawn")
                {
                    members.insert("hp".to_owned(), Property::new(Value::Int(6)));
                }
            }),
            ("a list item", |o| {
                if let Some(Property {
                    value: Value::List(items),
                    ..
                }) = o.properties.get_mut("path")
                {
                    items[0] = Property::new(Value::Object(ObjectId(4)));
                }
            }),
            ("a list item's colour", |o| {
                if let Some(Property {
                    value: Value::List(items),
                    ..
                }) = o.properties.get_mut("path")
                {
                    items[2] = Property::new(Value::Color(None));
                }
            }),
        ];
        let original = objects_checksum(&[full_object()]);
        for (field, edit) in edits {
            let mut object = full_object();
            edit(&mut object);
            assert_ne!(objects_checksum(&[object]), original, "{field}");
        }

        type TextEdit = fn(&mut TextData);
        let text_edits: Vec<(&str, TextEdit)> = vec![
            ("the text", |t| t.text = "hello".to_owned()),
            ("the font", |t| t.font_family = "serif".to_owned()),
            ("the size", |t| t.pixel_size = 12),
            ("wrapping", |t| t.wrap = true),
            ("the colour", |t| t.color = Color::WHITE),
            ("bold", |t| t.bold = true),
            ("italic", |t| t.italic = true),
            ("underline", |t| t.underline = true),
            ("strikeout", |t| t.strikeout = true),
            ("kerning", |t| t.kerning = false),
            ("horizontal alignment", |t| t.halign = HAlign::Justify),
            ("vertical alignment", |t| t.valign = VAlign::Bottom),
        ];
        let with_text = |text: TextData| MapObject {
            shape: Shape::Text(text),
            ..full_object()
        };
        let plain_text = objects_checksum(&[with_text(TextData::default())]);
        for (field, edit) in text_edits {
            let mut text = TextData::default();
            edit(&mut text);
            assert_ne!(objects_checksum(&[with_text(text)]), plain_text, "{field}");
        }

        let second = MapObject {
            id: ObjectId(9),
            ..MapObject::default()
        };
        assert_ne!(
            objects_checksum(&[full_object(), second.clone()]),
            objects_checksum(&[second, full_object()]),
            "the objects' order"
        );
    }

    #[test]
    fn objects_checksums_survive_the_rounding_of_a_save_and_the_tilesets_order() {
        let original = objects_checksum(&[full_object()]);
        // Tiled's XML writes 146.96875 as 146.969: reading it back changes nothing.
        let mut rounded = full_object();
        rounded.pos.x = 146.969;
        assert_eq!(objects_checksum(&[rounded]), original);
        let mut negative_zero = full_object();
        negative_zero.shape = Shape::Polygon(vec![DVec2::new(-0.0, 0.0), DVec2::new(8.0, 4.0)]);
        assert_eq!(objects_checksum(&[negative_zero]), original);
        // The tile object's tileset is slot 0 here and slot 1 there.
        let mut reordered = full_object();
        reordered.tile = reordered.tile.map(|cell| {
            bt_core::Cell::new(TilesetSlot::new(1).unwrap(), cell.tile()).with_flip(cell.flip())
        });
        assert_eq!(objects(&map(false, DECOR_FIRST), &[reordered]), original);
    }

    /// The canonical stream is part of the file format: these values change only on purpose.
    /// The first two were also computed from the stream as the module describes it, outside
    /// Rust.
    #[test]
    fn the_canonical_stream_is_pinned() {
        assert_eq!(layer_checksum(&map(true, FILE_FIRST)), "ce0f8a023a8bf5ab");
        assert_eq!(objects_checksum(&[]), "46f7cff835ee795e");
        assert_eq!(objects_checksum(&[full_object()]), "fec892d72b0a56c7");
    }
}
