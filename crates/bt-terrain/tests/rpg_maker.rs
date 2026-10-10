//! RPG Maker A2 autotiles, on sheets drawn pixel by pixel here (no RPG Maker sheet is in the
//! repository or the packs): an autotile expands to the tiles of a blob drawn from the Blob 7×7
//! layout and to the labels of Tiled's own blob set, and painting with the result gives what
//! painting with Tiled's set gives. Other kinds of sheets warn.

#![cfg(test)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Lane, Map, Tileset, WangSetType};
use bt_image::DecodedImage;
use bt_terrain::layouts::LAYOUTS;
use bt_terrain::rpg_maker::{RpgMakerError, SheetWarning, expand, sheet_warning, tileset};
use bt_terrain::{BrushTarget, FillRequest, PaintRequest, Terrain, TerrainColor, fill, paint};
use bt_test_data::Scratch;
use bt_test_data::fixtures::MapBuilder;
use glam::{IVec2, UVec2};

use common::pictures::*;
use common::{GROUND, slot};

/// The tile size of the drawn sheets: quarters of 6 pixels, a blob's 3×3 lanes of 4.
const TILE: u32 = 12;

/// The autotile's terrain.
const GRASS: [u8; 4] = [80, 160, 70, 255];

/// Draws an A2 autotile with its top-left tile at `column`, `row` (in autotiles) as RPG Maker
/// lays one out: the palette tile, the four inner corners (the terrain but for a notch at each
/// corner), and a 2×2-tile island whose outer band of 4 pixels is see-through.
fn draw_autotile(sheet: &mut DecodedImage, column: u32, row: u32) {
    let (x0, y0) = (column * 2, row * 3);
    fill_tile(sheet, x0, y0, TILE, GRASS, |_, _| true);
    let inner = 4..TILE - 4;
    let notch = |x: u32, y: u32| !inner.contains(&x) && !inner.contains(&y);
    fill_tile(sheet, x0 + 1, y0, TILE, GRASS, |x, y| !notch(x, y));
    let island = 4..2 * TILE - 4;
    for (dx, dy) in [(0, 1), (1, 1), (0, 2), (1, 2)] {
        fill_tile(sheet, x0 + dx, y0 + dy, TILE, GRASS, |x, y| {
            island.contains(&(dx * TILE + x)) && island.contains(&((dy - 1) * TILE + y))
        });
    }
}

/// A sheet of `columns`×`rows` autotiles, those listed drawn, written as `name` into `scratch`.
fn sheet(scratch: &Scratch, name: &str, columns: u32, rows: u32, drawn: &[[u32; 2]]) -> PathBuf {
    let mut picture = blank(columns * 2, rows * 3, TILE);
    for &[column, row] in drawn {
        draw_autotile(&mut picture, column, row);
    }
    let path = scratch.join(name);
    bt_image::write_png(&path, &picture).unwrap();
    path
}

/// The blob of the Blob 7×7 layout drawn lane by lane: what the expansion should be.
fn blob_picture() -> DecodedImage {
    let blob = LAYOUTS.iter().find(|l| l.name == "Blob 7×7").unwrap();
    let mut picture = blank(7, 7, TILE);
    draw_layout(&mut picture, blob, UVec2::ZERO, TILE, GRASS, None);
    picture
}

#[test]
fn an_autotile_expands_to_the_blob_drawn_from_its_quarters() {
    let scratch = Scratch::new("bt-terrain-rpg-maker-expand").unwrap();
    let path = sheet(&scratch, "Floors_A2.png", 1, 1, &[[0, 0]]);
    let expansion = expand(&path, TILE).unwrap();
    assert_eq!(expansion.path(), scratch.join("Floors_A2 blob.png"));
    assert_eq!(expansion.blocks(), [UVec2::ZERO]);
    assert_eq!(expansion.tile(), TILE);
    let expected = blob_picture();
    let image = expansion.image();
    assert_eq!(
        (image.width, image.height),
        (expected.width, expected.height)
    );
    for y in 0..image.height {
        for x in 0..image.width {
            assert_eq!(
                get(image, x, y),
                get(&expected, x, y),
                "pixel {x},{y}, in tile {},{}",
                x / TILE,
                y / TILE
            );
        }
    }
    let written = bt_image::decode_file(expansion.path(), None).unwrap();
    assert_eq!(&written, image, "the file holds the composed image");
}

#[test]
fn the_tileset_has_the_labels_of_tileds_blob_set() {
    let Some(blob) = bt_test_data::test_data("reference/tiled/tests/wangtiles/wangblob.tsx") else {
        return;
    };
    let theirs = bt_tmx::Loader::default().load_tileset(&blob).unwrap().value;
    let scratch = Scratch::new("bt-terrain-rpg-maker-labels").unwrap();
    let path = sheet(&scratch, "Floors_A2.png", 1, 1, &[[0, 0]]);
    let ours = tileset(&expand(&path, TILE).unwrap(), "Floors");
    assert_eq!(
        (ours.name.as_str(), ours.columns(), ours.tile_count()),
        ("Floors", 7, 49)
    );
    let [set] = ours.wang_sets.as_slice() else {
        panic!("one terrain set: {:?}", ours.wang_sets);
    };
    assert_eq!(
        (set.name.as_str(), set.kind),
        ("Floors", WangSetType::Mixed)
    );
    let names: Vec<&str> = set.colors.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Autotile 1"]);
    let color = set.colors[0].color;
    assert_eq!([color.r, color.g, color.b], [GRASS[0], GRASS[1], GRASS[2]]);
    let yellow = theirs
        .wang_sets
        .iter()
        .find(|s| s.name == "Blob (only Yellow)")
        .unwrap();
    assert_eq!(
        set.tiles, yellow.tiles,
        "Tiled's blob of one terrain over nothing"
    );
    assert_eq!(set.colors[0].tile, yellow.colors[0].tile, "its full tile");
}

/// A map of `size` cells with `tileset` and an empty layer.
fn map_with(tileset: Arc<Tileset>, size: u32) -> Map {
    MapBuilder::new(UVec2::splat(16), UVec2::splat(size))
        .tileset(tileset)
        .tile_layer("ground", "")
        .build()
        .unwrap()
}

/// Paints strokes and a fill with terrain 1 of set `set_index`, the same seeds each time.
fn painted(tileset: Arc<Tileset>, set_index: usize) -> Map {
    let mut map = map_with(tileset, 14);
    let terrain = Terrain::new(&map, slot(), set_index).unwrap();
    let one = TerrainColor(1);
    let strokes = [
        vec![
            BrushTarget::Tile(IVec2::new(2, 2)),
            BrushTarget::Tile(IVec2::new(3, 2)),
            BrushTarget::Tile(IVec2::new(3, 3)),
        ],
        vec![BrushTarget::Corner(IVec2::new(8, 3))],
        vec![BrushTarget::Edge {
            cell: IVec2::new(10, 9),
            side: Lane::Left,
        }],
        vec![BrushTarget::Tile(IVec2::new(5, 9))],
    ];
    for (seed, targets) in (1_u64..).zip(strokes) {
        let request = PaintRequest::new(GROUND, &terrain, one)
            .targets(targets)
            .seed(seed);
        paint(&mut map, &request).unwrap();
    }
    let cells = bt_utils::IRect {
        x: 9,
        y: 1,
        w: 4,
        h: 4,
    }
    .points()
    .collect();
    let request = FillRequest::new(GROUND, &terrain, one)
        .cells(cells)
        .seed(99);
    fill(&mut map, &request).unwrap();
    map
}

#[test]
fn painting_with_the_tileset_gives_what_tileds_blob_set_gives() {
    let Some(blob) = bt_test_data::test_data("reference/tiled/tests/wangtiles/wangblob.tsx") else {
        return;
    };
    let theirs = bt_tmx::Loader::default().load_tileset(&blob).unwrap().value;
    let yellow = theirs
        .wang_sets
        .iter()
        .position(|s| s.name == "Blob (only Yellow)")
        .unwrap();
    let scratch = Scratch::new("bt-terrain-rpg-maker-paint").unwrap();
    let path = sheet(&scratch, "Floors_A2.png", 1, 1, &[[0, 0]]);
    let ours = Arc::new(tileset(&expand(&path, TILE).unwrap(), "Floors"));
    let with_ours = painted(ours, 0);
    let with_theirs = painted(theirs, yellow);
    let layer = |map: &Map| {
        map.layer(GROUND)
            .and_then(|l| l.as_tile_layer())
            .unwrap()
            .clone()
    };
    let (ours, theirs) = (layer(&with_ours), layer(&with_theirs));
    let mut painted_cells = 0;
    for at in (bt_utils::IRect {
        x: 0,
        y: 0,
        w: 14,
        h: 14,
    })
    .points()
    {
        let (a, b) = (ours.cell(at), theirs.cell(at));
        assert_eq!((a.tile(), a.flip()), (b.tile(), b.flip()), "cell {at}");
        painted_cells += usize::from(!a.is_empty());
    }
    assert!(
        painted_cells > 20,
        "the strokes painted: {painted_cells} cells"
    );
}

#[test]
fn empty_autotiles_make_no_terrain() {
    let scratch = Scratch::new("bt-terrain-rpg-maker-empty").unwrap();
    // Three autotiles in a row, the middle one empty.
    let path = sheet(&scratch, "Floors_A2.png", 3, 1, &[[0, 0], [2, 0]]);
    let expansion = expand(&path, TILE).unwrap();
    assert_eq!(expansion.blocks(), [UVec2::ZERO, UVec2::new(14, 0)]);
    let ours = tileset(&expansion, "Floors");
    let set = &ours.wang_sets[0];
    let names: Vec<&str> = set.colors.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Autotile 1", "Autotile 2"]);
    assert_eq!(
        set.tiles.len(),
        2 * 46,
        "a blob's tiles but the plain ones, twice"
    );
    assert!(
        set.tiles.keys().all(|t| !(7..14).contains(&(t.0 % 21))),
        "nothing in the empty autotile's block"
    );
    assert!(
        set.tiles.iter().all(|(t, label)| label
            .lanes()
            .iter()
            .all(|&c| c == 0 || c == 1 + u8::from(t.0 % 21 >= 14))),
        "the third autotile is the second terrain"
    );
}

#[test]
fn expanding_never_writes_over_a_file() {
    let scratch = Scratch::new("bt-terrain-rpg-maker-names").unwrap();
    let path = sheet(&scratch, "Floors_A2.png", 1, 1, &[[0, 0]]);
    std::fs::write(scratch.join("Floors_A2 blob.png"), b"mine").unwrap();
    let first = expand(&path, TILE).unwrap();
    assert_eq!(first.path(), scratch.join("Floors_A2 blob 2.png"));
    let second = expand(&path, TILE).unwrap();
    assert_eq!(second.path(), scratch.join("Floors_A2 blob 3.png"));
    assert_eq!(
        std::fs::read(scratch.join("Floors_A2 blob.png")).unwrap(),
        b"mine"
    );
}

#[test]
fn sheets_that_cannot_be_expanded_say_why() {
    let scratch = Scratch::new("bt-terrain-rpg-maker-errors").unwrap();
    let path = sheet(&scratch, "Floors_A2.png", 1, 1, &[[0, 0]]);
    assert_eq!(expand(&path, 7), Err(RpgMakerError::TileSize(7)));
    assert_eq!(expand(&path, 0), Err(RpgMakerError::TileSize(0)));
    assert!(matches!(
        expand(&path, 48),
        Err(RpgMakerError::NoAutotile { tile: 48, .. })
    ));
    let empty = sheet(&scratch, "Empty_A2.png", 2, 1, &[]);
    assert_eq!(expand(&empty, TILE), Err(RpgMakerError::NothingDrawn));
    assert!(matches!(
        expand(Path::new(&scratch.join("missing.png")), TILE),
        Err(RpgMakerError::Image(bt_image::Error::Io { .. }))
    ));
}

#[test]
fn other_kinds_of_sheets_warn() {
    assert_eq!(sheet_warning("Outside_A2.png", 768, 576, 48), None);
    for (name, width, height, tile, warning) in [
        ("World_A1.png", 768, 576, 48, SheetWarning::A1),
        ("Outside_A3.png", 768, 384, 48, SheetWarning::A3),
        ("Inside_A4.png", 768, 720, 48, SheetWarning::A4),
        ("Dungeon_A5.png", 384, 768, 48, SheetWarning::A5),
        ("Outside_B.png", 768, 768, 48, SheetWarning::Plain),
        ("Inside_D.png", 768, 768, 48, SheetWarning::Plain),
        ("roofs.png", 768, 384, 48, SheetWarning::A3),
        (
            "Outside_A2.png",
            800,
            576,
            48,
            SheetWarning::Partial { tile: 48 },
        ),
    ] {
        assert_eq!(
            sheet_warning(name, width, height, tile),
            Some(warning),
            "{name}"
        );
        assert!(!warning.to_string().is_empty());
    }
}
