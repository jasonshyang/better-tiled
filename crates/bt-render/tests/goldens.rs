//! Golden pictures: small scenes drawn and compared with committed PNGs. `BT_UPDATE_GOLDENS=1`
//! rewrites them from the current output; a failing comparison writes the picture and a
//! difference picture into `BT_GOLDEN_DIFF_DIR` when it is set.

#![cfg(test)]

mod common;

use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{
    BlendMode, Cell, Color, EmbeddedImage, FileRef, FillMode, Frame, HAlign, ImageLayer, ImageRef,
    ImageSource, Layer, LayerId, LayerKind, MapObject, ObjectId, ObjectLayer, Orientation,
    RenderOrder, Shape, StaggerAxis, StaggerIndex, TextData, TileId, TileRenderSize, TilesetSlot,
    VAlign,
};
use bt_render::{
    Dash, GridStyle, LayerVisibility, ObjectStyle, Picture, PreviewTile, Render, TextItem,
    TextStyle, VectorItem, grid_vectors,
};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use bt_utils::DRect;
use common::device;
use glam::{DVec2, IVec2, UVec2};

/// How closely a picture must match its golden.
#[derive(Copy, Clone)]
struct Match {
    /// The most a premultiplied channel may differ.
    tolerance: u8,
    /// The share of pixels allowed to differ by more.
    share: f64,
}

/// Sprites only: exact.
const EXACT: Match = Match {
    tolerance: 0,
    share: 0.0,
};

/// Alpha blending may round a channel one step differently across GPU backends.
const BLENDED: Match = Match {
    tolerance: 1,
    share: 0.0,
};

/// Anti-aliased lines and text, which GPUs rasterise slightly differently.
const SMOOTH: Match = Match {
    tolerance: 6,
    share: 0.01,
};

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(format!("{name}.png"))
}

fn check(name: &str, picture: &Picture, within: Match) {
    let path = golden_path(name);
    if std::env::var_os("BT_UPDATE_GOLDENS").is_some_and(|v| !v.is_empty() && v != "0") {
        image::save_buffer(
            &path,
            &picture.rgba,
            picture.width,
            picture.height,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
        return;
    }
    let golden = image::open(&path)
        .unwrap_or_else(|error| {
            panic!(
                "{}: {error}; run with BT_UPDATE_GOLDENS=1 to write it",
                path.display()
            )
        })
        .to_rgba8();
    let expected = Picture {
        width: golden.width(),
        height: golden.height(),
        rgba: golden.into_raw(),
    };
    assert_eq!(
        (picture.width, picture.height),
        (expected.width, expected.height),
        "{name}: size"
    );
    let share = common::differing_share(picture, &expected, within.tolerance);
    if share > within.share {
        if let Some(dir) = std::env::var_os("BT_GOLDEN_DIFF_DIR") {
            let dir = PathBuf::from(dir);
            common::keep(picture, &format!("{name}-actual"));
            let diff: Vec<u8> = picture
                .rgba
                .chunks_exact(4)
                .zip(expected.rgba.chunks_exact(4))
                .flat_map(|(a, e)| {
                    if a == e {
                        [e[0] / 3, e[1] / 3, e[2] / 3, 255]
                    } else {
                        [255, 0, 0, 255]
                    }
                })
                .collect();
            image::save_buffer(
                dir.join(format!("{name}-diff.png")),
                &diff,
                picture.width,
                picture.height,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        }
        panic!(
            "{name}: {:.3} % of pixels differ from the golden",
            share * 100.0
        );
    }
}

fn slot(index: usize) -> TilesetSlot {
    TilesetSlot::new(index).unwrap()
}

fn atlas() -> Atlas {
    Atlas::builder().build_embedded().unwrap()
}

/// PNG bytes of an image `size` pixels drawn by `paint`.
fn png(size: UVec2, paint: impl Fn(u32, u32) -> [u8; 4]) -> EmbeddedImage {
    let image = image::RgbaImage::from_fn(size.x, size.y, |x, y| image::Rgba(paint(x, y)));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    EmbeddedImage {
        format: "png".to_owned(),
        bytes: Arc::from(bytes.into_inner()),
    }
}

fn image_ref(image: EmbeddedImage, size: UVec2) -> ImageRef {
    ImageRef {
        source: ImageSource::Embedded(image),
        declared_size: size,
        transparent: None,
    }
}

/// A gradient with alpha ramps, for tints and blend modes.
fn gradient(size: UVec2) -> EmbeddedImage {
    png(size, |x, y| {
        let r = (x * 255 / size.x.max(1)) as u8;
        let g = (y * 255 / size.y.max(1)) as u8;
        [
            r,
            g,
            255 - r / 2,
            if (x / 8 + y / 8) % 3 == 0 { 128 } else { 255 },
        ]
    })
}

fn render(map: bt_core::Map) -> Render {
    Render::of(Arc::new(map))
}

#[test]
fn flips_on_square_grids() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder().tile_size(16, 24).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(24, 24), UVec2::new(8, 1))
        .tileset(atlas.tileset.clone())
        .tile_layer("flips", "1 1h 1v 1hv 1d 1dh 1dv 1dhv")
        .build()
        .unwrap();
    check("flips-square", &render(map).run(device).unwrap(), EXACT);
}

#[test]
fn turns_on_hexagonal_grids() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder().marker(6, 3).build_embedded().unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 2))
        .orientation(Orientation::Hexagonal)
        .hex_side_length(8)
        .stagger(StaggerAxis::X, StaggerIndex::Odd)
        .tileset(atlas.tileset.clone())
        .tile_layer("turns", "2 2d 2x 2dx 2hvd 2hd\n2h 2v 2hx 2vx 2hvx 2dhvx")
        .build()
        .unwrap();
    check(
        "turns-hexagonal",
        &render(map).scale(2.0).run(device).unwrap(),
        SMOOTH,
    );
}

#[test]
fn render_sizes_fill_modes_and_offsets() {
    let Some(device) = device() else { return };
    let stretched = Atlas::builder()
        .name("stretched")
        .tile_size(32, 16)
        .render_size(TileRenderSize::Grid)
        .build_embedded()
        .unwrap();
    let fitted = Atlas::builder()
        .name("fitted")
        .tile_size(32, 16)
        .render_size(TileRenderSize::Grid)
        .fill_mode(FillMode::PreserveAspectFit)
        .build_embedded()
        .unwrap();
    let offset = Atlas::builder()
        .name("offset")
        .tile_size(8, 8)
        .tile_offset(IVec2::new(4, -2))
        .build_embedded()
        .unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 2))
        .tileset(stretched.tileset.clone())
        .tileset(fitted.tileset.clone())
        .tileset(offset.tileset.clone())
        .tile_layer("sizes", "1@0 1@1 1@2 1@0d\n2@1h 2@2v 3@0 3@1d")
        .build()
        .unwrap();
    check(
        "sizes-and-offsets",
        &render(map).scale(2.0).run(device).unwrap(),
        EXACT,
    );
}

#[test]
fn animations_at_a_fixed_time() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder()
        .animation(
            TileId(0),
            vec![
                Frame {
                    tile: TileId(1),
                    duration_ms: 100,
                },
                Frame {
                    tile: TileId(2),
                    duration_ms: 100,
                },
            ],
        )
        .animation(
            TileId(3),
            vec![
                Frame {
                    tile: TileId(4),
                    duration_ms: 30,
                },
                Frame {
                    tile: TileId(5),
                    duration_ms: 0,
                },
                Frame {
                    tile: TileId(6),
                    duration_ms: 70,
                },
            ],
        )
        .build_embedded()
        .unwrap();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 1))
        .tileset(atlas.tileset.clone())
        .tile_layer("water", "0 3 3h")
        .build()
        .unwrap();
    check(
        "animation-120ms",
        &render(map).time_ms(120).run(device).unwrap(),
        EXACT,
    );
}

#[test]
fn tints_follow_tileds_formula() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 3))
        .tileset(atlas.tileset.clone())
        .image_layer(
            "gradient",
            ImageLayer {
                image: Some(image_ref(gradient(UVec2::new(96, 48)), UVec2::new(96, 48))),
                ..ImageLayer::default()
            },
        )
        .tile_layer("red", "1 2 . . . .\n. . . . . .\n. . . . . .")
        .tile_layer("translucent blue", ". . 3 4 . .\n. . . . . .\n. . . . . .")
        .build()
        .unwrap();
    map.layers[0].tint = Some(Color::rgb(0, 255, 64));
    map.layers[0].offset = DVec2::new(0.0, 16.0);
    map.layers[1].tint = Some(Color::rgb(255, 0, 0));
    map.layers[2].tint = Some(Color::rgba(0, 0, 255, 128));
    check("tints", &render(map).run(device).unwrap(), EXACT);
}

#[test]
fn every_blend_mode() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let modes = [
        BlendMode::Add,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ];
    let mut builder = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 2))
        .tileset(atlas.tileset.clone())
        .background(Color::rgb(40, 40, 60))
        .image_layer(
            "backdrop",
            ImageLayer {
                image: Some(image_ref(
                    gradient(UVec2::new(192, 32)),
                    UVec2::new(192, 32),
                )),
                ..ImageLayer::default()
            },
        );
    for (i, _) in modes.iter().enumerate() {
        let row: Vec<String> = (0..12)
            .map(|x| {
                if x == i {
                    (i + 2).to_string()
                } else {
                    ".".to_owned()
                }
            })
            .collect();
        let grid = format!("{}\n{}", row.join(" "), row.join(" "));
        builder = builder.tile_layer(&format!("mode {i}"), &grid);
    }
    let mut map = builder.build().unwrap();
    for (i, mode) in modes.iter().enumerate() {
        map.layers[i + 1].blend_mode = *mode;
        map.layers[i + 1].opacity = if i % 2 == 0 { 1.0 } else { 0.7 };
    }
    let picture = render(map)
        .background(Color::rgb(40, 40, 60))
        .run(device)
        .unwrap();
    check(
        "blend-modes",
        &picture,
        Match {
            // Several offscreen composites accumulate rounding differences.
            tolerance: 3,
            share: 0.0,
        },
    );
}

#[test]
fn every_render_order_with_overlapping_tiles() {
    let Some(device) = device() else { return };
    let atlas = Atlas::builder()
        .tile_size(24, 20)
        .grid(4, 1)
        .build_embedded()
        .unwrap();
    for (name, order) in [
        ("right-down", RenderOrder::RightDown),
        ("right-up", RenderOrder::RightUp),
        ("left-down", RenderOrder::LeftDown),
        ("left-up", RenderOrder::LeftUp),
    ] {
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(5, 4))
            .render_order(order)
            .tileset(atlas.tileset.clone())
            .tile_layer("overlap", "0 1 2 3 0\n1 2 3 0 1\n2 3 0 1 2\n3 0 1 2 3")
            .build()
            .unwrap();
        check(
            &format!("render-order-{name}"),
            &render(map).run(device).unwrap(),
            EXACT,
        );
    }
}

#[test]
fn image_layers_repeat_from_their_origin() {
    let Some(device) = device() else { return };
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(6, 4))
        .image_layer(
            "both",
            ImageLayer {
                image: Some(image_ref(gradient(UVec2::new(20, 12)), UVec2::new(20, 12))),
                repeat_x: true,
                repeat_y: true,
            },
        )
        .image_layer(
            "across",
            ImageLayer {
                image: Some(image_ref(
                    png(UVec2::new(10, 6), |x, _| {
                        [255, 255, 255, if x < 5 { 255 } else { 90 }]
                    }),
                    UVec2::new(10, 6),
                )),
                repeat_x: true,
                repeat_y: false,
            },
        )
        .build()
        .unwrap();
    map.layers[0].offset = DVec2::new(5.0, 3.0);
    map.layers[1].offset = DVec2::new(-3.0, 40.0);
    check(
        "image-layers-repeat",
        &render(map).run(device).unwrap(),
        EXACT,
    );
}

#[test]
fn parallax_moves_layers_with_the_view() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let mut map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(10, 4))
        .tileset(atlas.tileset.clone())
        .tile_layer(
            "far",
            "1 1 1 1 1 1 1 1 1 1\n. . . . . . . . . .\n. . . . . . . . . .\n. . . . . . . . . .",
        )
        .tile_layer(
            "near",
            ". . . . . . . . . .\n2 . 2 . 2 . 2 . 2 .\n. . . . . . . . . .\n. . . . . . . . . .",
        )
        .tile_layer(
            "still",
            ". . . . . . . . . .\n. . . . . . . . . .\n3 3 3 3 3 3 3 3 3 3\n. . . . . . . . . .",
        )
        .build()
        .unwrap();
    map.layers[0].parallax = DVec2::new(0.5, 1.0);
    map.layers[1].parallax = DVec2::new(2.0, 1.0);
    let picture = render(map)
        .view(DVec2::new(40.0, 0.0), UVec2::new(80, 64))
        .parallax(true)
        .run(device)
        .unwrap();
    check("parallax", &picture, EXACT);
}

#[test]
fn highlight_fades_below_and_dims_above() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 2))
        .tileset(atlas.tileset.clone())
        .tile_layer("below", "1 1 1 1\n1 1 1 1")
        .tile_layer("current", ". 2 2 .\n. 2 2 .")
        .tile_layer("above", ". . 3 3\n. . 3 3")
        .build()
        .unwrap();
    let picture = render(map)
        .layers(LayerVisibility::default().highlight(vec![LayerId(2)], Color::rgb(20, 20, 40)))
        .run(device)
        .unwrap();
    check("highlight", &picture, BLENDED);
}

#[test]
fn previews_draw_above_their_layer() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 2))
        .tileset(atlas.tileset.clone())
        .tile_layer("ground", "1 1 1 1\n1 1 1 1")
        .tile_layer("top", ". . . 4\n. . . 4")
        .build()
        .unwrap();
    let previews: Vec<PreviewTile> = (0..4)
        .map(|x| PreviewTile {
            at: IVec2::new(x, x % 2),
            cell: Cell::new(slot(0), TileId(7)),
            opacity: 0.6,
        })
        .collect();
    let picture = render(map)
        .previews(LayerId(1), Arc::from(previews))
        .run(device)
        .unwrap();
    check("previews", &picture, BLENDED);
}

#[test]
fn what_cannot_be_drawn_gets_tileds_marker() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let mut missing = bt_core::Tileset {
        name: "missing".to_owned(),
        tile_width: 16,
        tile_height: 16,
        image: bt_core::TilesetImage::Atlas {
            image: ImageRef::file(
                FileRef::new("nowhere.png", "/nowhere/at/all/nowhere.png"),
                UVec2::new(64, 64),
            ),
            columns: 4,
            tile_count: 16,
        },
        ..bt_core::Tileset::default()
    };
    missing.next_tile_id = 16;
    let object = MapObject {
        id: ObjectId(1),
        pos: DVec2::new(48.0, 32.0),
        size: DVec2::new(16.0, 16.0),
        tile: Some(Cell::new(slot(5), TileId(0))),
        ..MapObject::default()
    };
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 2))
        .tileset(atlas.tileset.clone())
        .tileset(Arc::new(missing))
        .tile_layer("ground", "1 99 2@1 .\n. . . .")
        .object_layer("things", vec![object])
        .build()
        .unwrap();
    let picture = render(map).scale(2.0).run(device).unwrap();
    check("markers", &picture, SMOOTH);
}

#[test]
fn vector_items_and_text() {
    let Some(device) = device() else { return };
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
        .build()
        .unwrap();
    let geometry = map.geometry();
    let visible = DRect::from_min_size(DVec2::ZERO, DVec2::new(192.0, 128.0));
    let mut items = grid_vectors(
        &geometry,
        visible,
        1.0,
        GridStyle::new(Color::BLACK, 1.0).major(IVec2::new(4, 4)),
    );
    items.push(
        VectorItem::polygon(vec![
            DVec2::new(10.0, 10.0),
            DVec2::new(70.0, 20.0),
            DVec2::new(40.0, 60.0),
            DVec2::new(30.0, 30.0),
        ])
        .fill(Color::rgba(255, 128, 0, 100))
        .stroke(Color::rgb(200, 80, 0), 2.0),
    );
    items.push(
        VectorItem::polyline(vec![
            DVec2::new(80.0, 10.0),
            DVec2::new(150.0, 30.0),
            DVec2::new(110.0, 60.0),
        ])
        .stroke(Color::rgb(0, 0, 200), 1.0)
        .dashed(Dash {
            on: 4.0,
            off: 3.0,
            phase: 0.0,
        }),
    );
    items.push(
        VectorItem::ellipse(DRect::from_min_size(
            DVec2::new(150.0, 70.0),
            DVec2::new(36.0, 24.0),
        ))
        .stroke(Color::rgb(0, 128, 0), 3.0),
    );
    items.push(
        VectorItem::rect(DRect::from_min_size(
            DVec2::new(10.0, 75.0),
            DVec2::new(30.0, 20.0),
        ))
        .world_stroke(Color::rgb(128, 0, 128), 4.0),
    );
    items.push(VectorItem::text(
        TextItem::new("Centred, wrapped text in a box", DVec2::new(50.0, 70.0))
            .size(12.0)
            .boxed(DVec2::new(90.0, 50.0))
            .align(HAlign::Center, VAlign::Center)
            .wrap(true)
            .color(Color::rgb(20, 20, 20)),
    ));
    items.push(VectorItem::text(
        TextItem::new("Styled", DVec2::new(150.0, 10.0))
            .size(14.0)
            .style(TextStyle {
                bold: true,
                italic: true,
                underline: true,
                strikeout: true,
            })
            .color(Color::rgb(160, 0, 0)),
    ));
    items.push(VectorItem::text(
        TextItem::new("label", DVec2::new(170.0, 120.0))
            .screen_size(11.0)
            .align(HAlign::Right, VAlign::Bottom)
            .color(Color::rgb(0, 90, 160)),
    ));
    let picture = render(map)
        .background(Color::rgb(240, 240, 235))
        .vectors(Arc::from(items))
        .scale(2.0)
        .run(device)
        .unwrap();
    check("vectors-and-text", &picture, SMOOTH);
}

#[test]
fn objects_as_tiled_draws_them() {
    let Some(device) = device() else { return };
    let atlas = atlas();
    let object = |id: u32, shape: Shape, pos: (f64, f64), size: (f64, f64)| MapObject {
        id: ObjectId(id),
        class: if id.is_multiple_of(2) {
            "door".to_owned()
        } else {
            String::new()
        },
        shape,
        pos: DVec2::new(pos.0, pos.1),
        size: DVec2::new(size.0, size.1),
        ..MapObject::default()
    };
    let mut objects = vec![
        object(1, Shape::Rectangle, (10.0, 10.0), (40.0, 20.0)),
        object(2, Shape::Ellipse, (60.0, 10.0), (30.0, 30.0)),
        object(3, Shape::Point, (120.0, 40.0), (0.0, 0.0)),
        object(
            4,
            Shape::Polygon(vec![
                DVec2::ZERO,
                DVec2::new(30.0, 5.0),
                DVec2::new(10.0, 30.0),
            ]),
            (140.0, 10.0),
            (0.0, 0.0),
        ),
        object(
            5,
            Shape::Polyline(vec![
                DVec2::ZERO,
                DVec2::new(20.0, 20.0),
                DVec2::new(40.0, 0.0),
            ]),
            (10.0, 60.0),
            (0.0, 0.0),
        ),
        object(6, Shape::Capsule, (60.0, 60.0), (50.0, 20.0)),
        object(
            7,
            Shape::Text(TextData {
                text: "Hello, objects".to_owned(),
                pixel_size: 12,
                wrap: true,
                color: Color::rgb(30, 30, 30),
                ..TextData::default()
            }),
            (120.0, 60.0),
            (60.0, 30.0),
        ),
        object(9, Shape::Rectangle, (160.0, 100.0), (20.0, 10.0)),
        object(11, Shape::Rectangle, (90.0, 100.0), (0.0, 0.0)),
    ];
    objects[7].rotation = 30.0;
    objects.push(MapObject {
        id: ObjectId(13),
        pos: DVec2::new(30.0, 120.0),
        size: DVec2::new(16.0, 16.0),
        tile: Some(Cell::new(slot(0), TileId(5))),
        rotation: 45.0,
        ..MapObject::default()
    });
    let layer = Layer {
        kind: LayerKind::Object(ObjectLayer {
            color: Some(Color::rgb(0, 120, 220)),
            objects: Arc::new(objects),
            ..ObjectLayer::default()
        }),
        ..Layer::new(LayerId(0), "objects", LayerKind::Group(Vec::new()))
    };
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
        .tileset(atlas.tileset.clone())
        .layer(layer)
        .build()
        .unwrap();
    let colors = std::collections::BTreeMap::from([("door".to_owned(), Color::rgb(220, 60, 60))]);
    let picture = render(map)
        .background(Color::rgb(235, 235, 230))
        .objects(ObjectStyle::default().class_colors(Arc::new(colors)))
        .scale(2.0)
        .run(device)
        .unwrap();
    check("objects", &picture, SMOOTH);
}
