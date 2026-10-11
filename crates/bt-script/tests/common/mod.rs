//! What the runner's tests share: a level on synthetic tilesets (a ground atlas with a grass and
//! water terrain set, a collection of sprites drawn with known feet), a way to run a script on
//! it, and ways to read the result.

#![allow(dead_code, reason = "each test file uses some of these")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{
    Cell, FileRef, ImageRef, Lane, LayerId, Map, MapObject, TileData, TileId, Tileset,
    TilesetImage, TilesetSlot, WangSetType,
};
use bt_image::DecodedImage;
use bt_script::{
    EditResult, Feet, Names, RunError, RunInput, parse_script, run_script, standing_cell,
};
use bt_test_data::Scratch;
use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
use glam::{DVec2, IVec2, UVec2};

/// A level and the folder it lives in.
pub struct Level {
    pub scratch: Scratch,
    pub map: Map,
    pub path: PathBuf,
}

/// The ground tileset's slot.
pub fn ground_slot() -> TilesetSlot {
    TilesetSlot::new(0).unwrap()
}

/// A tile of the ground tileset.
pub fn ground(id: u32) -> Cell {
    Cell::new(ground_slot(), TileId(id))
}

/// The ground tile with these terrains at its corners (top-right, bottom-right, bottom-left,
/// top-left; 1 grass, 2 water).
pub fn corners(c: [u8; 4]) -> Cell {
    ground(combination_tile(2, c).unwrap().0)
}

/// The ground atlas: 32 tiles, the first 16 labelled with every combination of grass and water
/// at their corners (the set "Land"), the next 15 with ice and nothing (the set "Frost", whose
/// ice borders nothing), and tile 31 plain.
pub fn ground_tileset() -> Tileset {
    let land = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Land")
        .terrains(&["Grass", "Water"])
        .every_combination();
    let mut frost = TerrainSetBuilder::new(WangSetType::Corner)
        .name("Frost")
        .terrains(&["Ice"]);
    let mut id = 16;
    for a in 0..2u8 {
        for b in 0..2u8 {
            for c in 0..2u8 {
                for d in 0..2u8 {
                    if [a, b, c, d] != [0, 0, 0, 0] {
                        frost = frost.label(id, bt_test_data::fixtures::corners([a, b, c, d]));
                        id += 1;
                    }
                }
            }
        }
    }
    let atlas = Atlas::builder()
        .name("ground")
        .grid(8, 4)
        .build_embedded()
        .unwrap();
    frost.added_to(land.added_to(Tileset::clone(&atlas.tileset)))
}

/// A picture: transparent but for the rectangles given, each `x, y, w, h, alpha`.
pub fn picture(width: u32, height: u32, rects: &[[u32; 5]]) -> DecodedImage {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for [x0, y0, w, h, alpha] in rects {
        for y in *y0..y0 + h {
            for x in *x0..x0 + w {
                let at = ((y * width + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[60, 140, 60, *alpha as u8]);
            }
        }
    }
    DecodedImage {
        width,
        height,
        rgba,
    }
}

/// The sprites: `tree` (16×32, its trunk's last row 29, two pixels wide at x 7–8: its foot is
/// the middle of the image's bottom row but two pixels up), `rock` (16×16, solid to the bottom
/// edge: its foot is the middle of its bottom edge) and `shadow` (16×8, the same).
pub fn sprite_tileset(dir: &Path) -> Tileset {
    let images = [
        (
            "tree.png",
            picture(16, 32, &[[2, 2, 12, 18, 255], [7, 20, 2, 10, 255]]),
        ),
        ("rock.png", picture(16, 16, &[[2, 6, 12, 10, 255]])),
        ("shadow.png", picture(16, 8, &[[0, 0, 16, 8, 255]])),
    ];
    let mut tiles = BTreeMap::new();
    for (id, (file, image)) in (0u32..).zip(images) {
        let path = dir.join(file);
        bt_image::write_png(&path, &image).unwrap();
        tiles.insert(
            TileId(id),
            TileData {
                image: Some(ImageRef::file(
                    FileRef::new(file, path),
                    UVec2::new(image.width, image.height),
                )),
                ..TileData::default()
            },
        );
    }
    Tileset {
        name: "props".into(),
        tile_width: 16,
        tile_height: 32,
        image: TilesetImage::Collection,
        tiles,
        next_tile_id: 3,
        ..Tileset::default()
    }
}

/// A 12×8 level of 16-pixel cells: `ground` (#1) all grass, `ice` (#2) empty, `props` (#3)
/// and `design` (#4) object layers holding `objects`.
pub fn level_with(name: &str, props: Vec<MapObject>, design: Vec<MapObject>) -> Level {
    let scratch = Scratch::new(name).unwrap();
    let sprites = sprite_tileset(scratch.path());
    let row = ["0"; 12].join(" ");
    let grid = vec![row; 8].join("\n");
    let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
        .tileset(Arc::new(ground_tileset()))
        .tileset(Arc::new(sprites))
        .tile_layer("ground", &grid)
        .tile_layer("ice", "")
        .object_layer("props", props)
        .object_layer("design", design)
        .build()
        .unwrap();
    let path = scratch.join("level.tmx");
    Level { scratch, map, path }
}

/// The level with no objects.
pub fn level(name: &str) -> Level {
    level_with(name, Vec::new(), Vec::new())
}

/// Runs `script` on `map` (at `path`) with `seed`.
pub fn run_on(map: &Map, path: &Path, script: &str, seed: u64) -> Result<EditResult, RunError> {
    let script = parse_script(script).unwrap();
    let input = RunInput::builder(Arc::new(map.clone()), path)
        .seed(seed)
        .build();
    run_script(&script, &input, &mut bt_tmx::Loader::default())
}

/// Runs `script` on the level, seed 1.
pub fn run(level: &Level, script: &str) -> Result<EditResult, RunError> {
    run_on(&level.map, &level.path, script, 1)
}

/// Runs `script` on the level and expects it to finish.
pub fn ran(level: &Level, script: &str) -> EditResult {
    match run(level, script) {
        Ok(result) => result,
        Err(error) => panic!("{error}"),
    }
}

/// The layer called `name`.
pub fn layer_id(map: &Map, name: &str) -> LayerId {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name {
            found = Some(layer.id);
        }
    });
    found.unwrap_or_else(|| panic!("no layer {name}"))
}

/// The terrains of a tile layer as text, a row a line: `g` grass, `w` water, `+` an edge (no
/// terrain covers more than half of the tile), `.` empty, `?` a tile with no terrain.
pub fn terrain_rows(map: &Map, layer: &str) -> Vec<String> {
    let names = Names::of(map);
    let tiles = map
        .layer(layer_id(map, layer))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    (0..map.height as i32)
        .map(|y| {
            (0..map.width as i32)
                .map(|x| {
                    let cell = tiles.cell(IVec2::new(x, y));
                    if cell.is_empty() {
                        return '.';
                    }
                    match names.terrain_covering(map, cell) {
                        Some(t) => t.name.chars().next().unwrap_or('?'),
                        None if names.tile_label(map, cell).ends_with("edges") => '+',
                        None => '?',
                    }
                })
                .collect()
        })
        .collect()
}

/// Where each terrain tile of a layer disagrees with its right or lower neighbour at the
/// corners they share: what `check` calls a seam.
pub fn seams(map: &Map, layer: &str) -> Vec<IVec2> {
    let tiles = map
        .layer(layer_id(map, layer))
        .unwrap()
        .as_tile_layer()
        .unwrap();
    let lanes = |cell: Cell| -> Option<[u8; 8]> {
        let tileset = map.tileset_of(cell)?;
        tileset
            .wang_sets
            .iter()
            .find_map(|set| set.tiles.get(&cell.tile()))
            .map(|id| id.lanes())
    };
    let mut found = Vec::new();
    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let here = IVec2::new(x, y);
            let Some(a) = lanes(tiles.cell(here)) else {
                continue;
            };
            let right = IVec2::new(x + 1, y);
            if let Some(b) = lanes(tiles.cell(right))
                && (a[Lane::TopRight.index()] != b[Lane::TopLeft.index()]
                    || a[Lane::BottomRight.index()] != b[Lane::BottomLeft.index()])
            {
                found.push(here);
            }
            let below = IVec2::new(x, y + 1);
            if let Some(b) = lanes(tiles.cell(below))
                && (a[Lane::BottomLeft.index()] != b[Lane::TopLeft.index()]
                    || a[Lane::BottomRight.index()] != b[Lane::TopRight.index()])
            {
                found.push(here);
            }
        }
    }
    found
}

/// The objects of the layer called `name`.
pub fn objects(map: &Map, name: &str) -> Vec<MapObject> {
    map.layer(layer_id(map, name))
        .unwrap()
        .as_object_layer()
        .unwrap()
        .objects
        .to_vec()
}

/// Where an object stands, in tile units.
pub fn stands(map: &Map, object: &MapObject) -> DVec2 {
    Feet::new().standing_point(map, object).unwrap()
}

/// The cell an object stands in.
pub fn standing_in(map: &Map, object: &MapObject) -> IVec2 {
    standing_cell(stands(map, object))
}

/// Whether two points are the same, give or take the noise of turning positions around.
pub fn close(a: DVec2, b: DVec2) -> bool {
    (a - b).length() < 1e-6
}

/// The text of every note of a result, `line N: …`.
pub fn notes(result: &EditResult) -> Vec<String> {
    result
        .outcome
        .notes
        .iter()
        .map(|note| format!("line {}: {}", note.line, note.text))
        .collect()
}
