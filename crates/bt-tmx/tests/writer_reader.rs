//! A map built in memory with every feature survives being written and read back, in XML and in
//! JSON, for every orientation and layer data format, finite and infinite.

#![cfg(test)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bt_core::{
    Alignment, BlendMode, Cell, ClassType, CollisionGroup, Color, CustomType, CustomTypes,
    DrawOrder, FileFormat, FileRef, FillMode, Flip, Frame, GridOrientation, HAlign, ImageLayer,
    ImageRef, Layer, LayerDataFormat, LayerId, LayerKind, Map, MapObject, ObjectId, ObjectLayer,
    ObjectOverrides, ObjectTemplate, Orientation, Properties, Property, RenderOrder, Shape,
    StaggerAxis, StaggerIndex, TextData, TileData, TileId, TileLayer, TileRenderSize, Tileset,
    TilesetGrid, TilesetImage, TilesetRef, TilesetSlot, Transformations, VAlign, Value, WangColor,
    WangId, WangSet, WangSetType,
};
use bt_test_data::Scratch;
use bt_tmx::{Format, Loader, WriteRequest};
use glam::{DVec2, IVec2, UVec2};

/// Five 32×32 tiles in a row: the image of the built-in rule tileset.
const IMAGE: &[u8] = include_bytes!("../src/automap-tiles.png");

fn slot(index: usize) -> TilesetSlot {
    TilesetSlot::new(index).unwrap()
}

fn every_kind_of_property() -> Properties {
    let mut members = Properties::new();
    members.insert("hp".into(), Property::new(Value::Int(3)));
    members.insert(
        "tint".into(),
        Property::new(Value::Color(Some(Color::rgba(1, 2, 3, 4)))),
    );
    let mut properties = Properties::new();
    for (name, value) in [
        ("bool", Value::Bool(true)),
        ("color", Value::Color(Some(Color::rgb(255, 128, 0)))),
        ("empty colour", Value::Color(None)),
        ("file", Value::File("../scripts/a.lua".into())),
        ("float", Value::Float(0.1)),
        ("int", Value::Int(-7)),
        ("lines", Value::String("one\ntwo".into())),
        ("object", Value::Object(ObjectId(2))),
        ("string", Value::String("x/é \"y\"".into())),
        (
            "list",
            Value::List(vec![
                Property::new(Value::Int(1)),
                Property::new(Value::String("a".into())),
                Property::new(Value::List(vec![Property::new(Value::Bool(false))])),
            ]),
        ),
    ] {
        properties.insert(name.into(), Property::new(value));
    }
    properties.insert(
        "class".into(),
        Property::typed(Value::Class(members), "Spawn"),
    );
    properties
}

/// An atlas tileset file of the rule tile image, with every tileset feature.
fn atlas(dir: &Path) -> Tileset {
    let image = ImageRef {
        transparent: Some(Color::rgb(255, 0, 255)),
        ..ImageRef::file(
            FileRef::new("tiles.png", dir.join("tiles.png")),
            UVec2::new(160, 32),
        )
    };
    let mut tiles = BTreeMap::new();
    tiles.insert(
        TileId(1),
        TileData {
            class: "Wall".into(),
            probability: 0.5,
            animation: vec![
                Frame {
                    tile: TileId(1),
                    duration_ms: 100,
                },
                Frame {
                    tile: TileId(2),
                    duration_ms: 150,
                },
            ],
            collision: Some(CollisionGroup {
                id: LayerId(2),
                layer: ObjectLayer {
                    color: None,
                    draw_order: DrawOrder::Index,
                    objects: Arc::new(vec![MapObject {
                        id: ObjectId(1),
                        pos: DVec2::new(1.5, 2.0),
                        size: DVec2::new(8.0, 4.0),
                        ..MapObject::default()
                    }]),
                },
            }),
            properties: every_kind_of_property(),
            ..TileData::default()
        },
    );
    let corner = WangSet {
        name: "ground".into(),
        class: String::new(),
        kind: WangSetType::Corner,
        tile: Some(TileId(0)),
        colors: vec![
            WangColor {
                name: "grass".into(),
                class: "Biome".into(),
                color: Color::rgb(0, 255, 0),
                tile: Some(TileId(2)),
                probability: 1.0,
                properties: Properties::new(),
            },
            WangColor {
                name: "water".into(),
                class: String::new(),
                color: Color::rgba(0, 0, 255, 128),
                tile: None,
                probability: 0.25,
                properties: every_kind_of_property(),
            },
        ],
        tiles: [(TileId(3), WangId::from_lanes([0, 1, 0, 2, 0, 1, 0, 2]))].into(),
        properties: Properties::new(),
    };
    Tileset {
        name: "rules".into(),
        class: "Atlas".into(),
        tile_width: 32,
        tile_height: 32,
        image: TilesetImage::Atlas {
            image,
            columns: 5,
            tile_count: 5,
        },
        tile_offset: IVec2::new(2, -3),
        object_alignment: Alignment::Bottom,
        tile_render_size: TileRenderSize::Grid,
        fill_mode: FillMode::PreserveAspectFit,
        grid: Some(TilesetGrid {
            orientation: GridOrientation::Isometric,
            size: UVec2::new(32, 16),
        }),
        background_color: Some(Color::rgb(10, 20, 30)),
        transformations: Transformations::HFLIP | Transformations::PREFER_UNTRANSFORMED,
        tiles,
        wang_sets: vec![corner],
        properties: every_kind_of_property(),
        next_tile_id: 5,
        file: FileFormat {
            version: "1.10".into(),
            tiled_version: "1.12.2".into(),
            ..FileFormat::default()
        },
        ..Tileset::default()
    }
}

/// A collection embedded in the map, its tiles in a custom order.
fn collection(dir: &Path) -> Tileset {
    let mut tiles = BTreeMap::new();
    for id in [0, 4] {
        tiles.insert(
            TileId(id),
            TileData {
                image: Some(ImageRef::file(
                    FileRef::new("tiles.png", dir.join("tiles.png")),
                    UVec2::new(160, 32),
                )),
                rect: (id == 4).then_some(bt_utils::IRect {
                    x: 32,
                    y: 0,
                    w: 32,
                    h: 32,
                }),
                ..TileData::default()
            },
        );
    }
    let mut tileset = Tileset {
        name: "props".into(),
        tile_width: 160,
        tile_height: 32,
        image: TilesetImage::Collection,
        tiles,
        next_tile_id: 5,
        ..Tileset::default()
    };
    tileset.set_tile_order(vec![TileId(4), TileId(0)]);
    tileset
}

fn feature_map(
    dir: &Path,
    atlas: &Arc<Tileset>,
    template: &ObjectTemplate,
    orientation: Orientation,
    format: LayerDataFormat,
    infinite: bool,
) -> Map {
    let atlas_cell = |tile: u32| Cell::new(slot(0), TileId(tile));
    let mut tiles = if infinite {
        TileLayer::infinite(UVec2::new(6, 4))
    } else {
        TileLayer::finite(UVec2::new(6, 4))
    };
    tiles.set_cell(IVec2::new(0, 0), atlas_cell(1)).unwrap();
    tiles
        .set_cell(IVec2::new(5, 3), atlas_cell(2).with_flip(Flip::H | Flip::D))
        .unwrap();
    tiles
        .set_cell(
            IVec2::new(2, 1),
            Cell::new(slot(1), TileId(4)).with_flip(Flip::V | Flip::HEX120),
        )
        .unwrap();
    if infinite {
        tiles.set_cell(IVec2::new(-20, -33), atlas_cell(3)).unwrap();
    }
    let mut ground = Layer::new(LayerId(1), "ground", LayerKind::Tile(tiles));
    ground.properties = every_kind_of_property();
    ground.tint = Some(Color::rgba(255, 255, 255, 200));
    ground.blend_mode = BlendMode::Multiply;

    let template_object = template.object.clone().unwrap();
    let objects = vec![
        MapObject {
            id: ObjectId(1),
            name: "box".into(),
            class: "Trigger".into(),
            pos: DVec2::new(12.5, 7.25),
            size: DVec2::new(30.0, 20.0),
            rotation: 45.0,
            visible: false,
            opacity: 0.75,
            properties: every_kind_of_property(),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(2),
            shape: Shape::Ellipse,
            size: DVec2::new(10.0, 5.0),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(3),
            shape: Shape::Point,
            pos: DVec2::new(-4.0, 9.0),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(4),
            shape: Shape::Capsule,
            size: DVec2::new(10.0, 30.0),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(5),
            shape: Shape::Polygon(vec![
                DVec2::ZERO,
                DVec2::new(10.0, 0.0),
                DVec2::new(5.5, -8.25),
            ]),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(6),
            shape: Shape::Polyline(vec![DVec2::ZERO, DVec2::new(3.0, 4.0)]),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(7),
            size: DVec2::new(100.0, 40.0),
            shape: Shape::Text(TextData {
                text: "Hello <world> & \"you\"\nsecond line".into(),
                font_family: "Serif".into(),
                pixel_size: 12,
                wrap: true,
                color: Color::rgba(1, 2, 3, 200),
                bold: true,
                italic: true,
                underline: true,
                strikeout: true,
                kerning: false,
                halign: HAlign::Justify,
                valign: VAlign::Bottom,
            }),
            ..MapObject::default()
        },
        MapObject {
            id: ObjectId(8),
            pos: DVec2::new(64.0, 64.0),
            size: DVec2::new(32.0, 32.0),
            tile: Some(atlas_cell(2).with_flip(Flip::V)),
            ..MapObject::default()
        },
        // A template instance: the template's values, with the name and rotation its own and a
        // property added.
        MapObject {
            id: ObjectId(9),
            name: "own name".into(),
            pos: DVec2::new(100.0, 50.0),
            rotation: 90.0,
            template: Some(FileRef::new("chest.tx", dir.join("chest.tx"))),
            overrides: ObjectOverrides::NAME | ObjectOverrides::ROTATION,
            properties: {
                let mut properties = template_object.properties.clone();
                properties.insert("own".into(), Property::new(Value::Int(1)));
                properties
            },
            tile: template_object.tile,
            class: template_object.class.clone(),
            size: template_object.size,
            ..MapObject::default()
        },
    ];
    let things = Layer::new(
        LayerId(2),
        "things",
        LayerKind::Object(ObjectLayer {
            color: Some(Color::rgb(160, 0, 0)),
            draw_order: DrawOrder::Index,
            objects: Arc::new(objects),
        }),
    );
    let mut picture = Layer::new(
        LayerId(4),
        "picture",
        LayerKind::Image(ImageLayer {
            image: Some(ImageRef {
                transparent: Some(Color::rgb(0, 0, 0)),
                ..ImageRef::file(
                    FileRef::new("tiles.png", dir.join("tiles.png")),
                    UVec2::new(160, 32),
                )
            }),
            repeat_x: true,
            repeat_y: false,
        }),
    );
    picture.offset = DVec2::new(3.5, -2.0);
    picture.parallax = DVec2::new(0.5, 1.25);
    picture.locked = true;
    let mut group = Layer::new(LayerId(3), "group", LayerKind::Group(vec![things, picture]));
    group.opacity = 0.5;
    group.visible = false;
    group.class = "Region".into();
    let (hex_side_length, stagger_axis) = match orientation {
        Orientation::Hexagonal => (8, StaggerAxis::X),
        _ => (0, StaggerAxis::Y),
    };
    Map {
        orientation,
        render_order: RenderOrder::LeftUp,
        width: 6,
        height: 4,
        tile_width: 32,
        tile_height: 16,
        infinite,
        hex_side_length,
        stagger_axis,
        // Only staggered and hexagonal maps store how they are staggered.
        stagger_index: if matches!(orientation, Orientation::Staggered | Orientation::Hexagonal) {
            StaggerIndex::Even
        } else {
            StaggerIndex::Odd
        },
        skew: if orientation == Orientation::Oblique {
            IVec2::new(4, -2)
        } else {
            IVec2::ZERO
        },
        parallax_origin: DVec2::new(16.0, 8.0),
        background_color: Some(Color::rgba(40, 50, 60, 70)),
        class: "Level".into(),
        properties: every_kind_of_property(),
        tilesets: vec![
            TilesetRef {
                file: Some(FileRef::new("atlas.tsx", dir.join("atlas.tsx"))),
                first_gid: Some(1),
                tileset: Arc::clone(atlas),
            },
            TilesetRef {
                file: None,
                first_gid: Some(6),
                tileset: Arc::new(collection(dir)),
            },
        ],
        layers: vec![ground, group],
        next_layer_id: 5,
        next_object_id: 10,
        file: FileFormat {
            version: "1.10".into(),
            tiled_version: "1.12.2".into(),
            layer_data: format,
            compression_level: (format == LayerDataFormat::Base64Zlib).then_some(9),
            output_chunk_size: if infinite {
                UVec2::new(8, 8)
            } else {
                UVec2::splat(16)
            },
            ..FileFormat::default()
        },
    }
}

/// Where two values' debug forms first differ, with what leads up to it.
fn first_difference(expected: &impl std::fmt::Debug, read: &impl std::fmt::Debug) -> String {
    let (expected, read) = (format!("{expected:#?}"), format!("{read:#?}"));
    let (a, b): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), read.lines().collect());
    let line = a
        .iter()
        .zip(&b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    format!(
        "{}\n  expected: {}\n  read:     {}",
        a[line.saturating_sub(8)..line].join("\n"),
        a.get(line).unwrap_or(&"<end>"),
        b.get(line).unwrap_or(&"<end>")
    )
}

#[test]
fn every_feature_survives_both_syntaxes() {
    let scratch = Scratch::new("bt-tmx-writer-reader").unwrap();
    std::fs::write(scratch.join("tiles.png"), IMAGE).unwrap();
    let atlas = atlas(scratch.path());
    WriteRequest::for_tileset(&atlas, &scratch.join("atlas.tsx"))
        .write()
        .unwrap();
    let template = ObjectTemplate {
        object: Some(MapObject {
            name: "chest".into(),
            class: "Chest".into(),
            size: DVec2::new(32.0, 32.0),
            tile: Some(Cell::new(slot(0), TileId(4))),
            properties: [("gold".to_owned(), Property::new(Value::Int(10)))].into(),
            ..MapObject::default()
        }),
        tilesets: vec![TilesetRef {
            file: Some(FileRef::new("atlas.tsx", scratch.join("atlas.tsx"))),
            first_gid: Some(1),
            tileset: Arc::new(atlas.clone()),
        }],
        file: FileFormat::default(),
    };
    WriteRequest::for_template(&template, &scratch.join("chest.tx"))
        .write()
        .unwrap();

    // JSON class values do not say their members' types; the project's classes do.
    let mut types = CustomTypes::new();
    types.add(CustomType::class(
        "Spawn",
        ClassType {
            members: [
                ("hp".to_owned(), Property::new(Value::Int(0))),
                ("tint".to_owned(), Property::new(Value::Color(None))),
            ]
            .into(),
            ..ClassType::default()
        },
    ));
    let mut loader = Loader::builder().custom_types(Arc::new(types)).build();
    let read_atlas = loader.load_tileset(&scratch.join("atlas.tsx")).unwrap();
    assert!(read_atlas.warnings.is_empty(), "{:?}", read_atlas.warnings);
    assert_eq!(*read_atlas.value, atlas, "the atlas file reads back");
    let read_template = loader
        .load_template(&scratch.join("chest.tx"))
        .unwrap()
        .value;
    assert_eq!(*read_template, template, "the template reads back");

    let orientations = [
        Orientation::Orthogonal,
        Orientation::Isometric,
        Orientation::Staggered,
        Orientation::Hexagonal,
        Orientation::Oblique,
    ];
    let formats = [
        LayerDataFormat::Csv,
        LayerDataFormat::Base64,
        LayerDataFormat::Base64Zlib,
        LayerDataFormat::Base64Gzip,
        LayerDataFormat::Base64Zstd,
    ];
    let mut cases = 0;
    for orientation in orientations {
        for format in formats {
            for infinite in [false, true] {
                for (syntax, extension) in [(Format::Xml, "tmx"), (Format::Json, "tmj")] {
                    let map = feature_map(
                        scratch.path(),
                        &read_atlas.value,
                        &template,
                        orientation,
                        format,
                        infinite,
                    );
                    let path = scratch.join(format!("map.{extension}"));
                    let warnings = WriteRequest::for_map(&map, &path)
                        .format(syntax)
                        .loader(&loader)
                        .write()
                        .unwrap();
                    assert!(warnings.is_empty(), "{warnings:?}");
                    let read = loader.load_map(&path).unwrap();
                    assert!(
                        read.warnings.is_empty(),
                        "{orientation:?} {format:?} {syntax:?}: {:?}",
                        read.warnings
                    );
                    let mut expected = map;
                    if infinite && syntax == Format::Json {
                        // Tiled's JSON states an infinite layer's chunk bounds as its size.
                        let LayerKind::Tile(tiles) = &mut expected.layers[0].kind else {
                            panic!("the first layer is the tile layer");
                        };
                        let bounds = tiles.chunk_bounds().unwrap();
                        let mut resized = TileLayer::infinite(bounds.size().as_uvec2());
                        for placed in tiles.cells() {
                            resized.set_cell(placed.at, placed.cell).unwrap();
                        }
                        *tiles = resized;
                    }
                    assert!(
                        read.value == expected,
                        "{orientation:?} {format:?} infinite {infinite} {syntax:?}: the map reads back differently:\n{}",
                        first_difference(&expected, &read.value)
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 100);
}
