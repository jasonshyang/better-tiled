//! Random rule maps, maps and edits for the fuzz tests: rule maps of up to three rules mixing
//! every option, special tile and layer kind AutoMap reads, and maps (finite or infinite) of
//! tiles those rules name, each with the text that reproduces it in a failure.

use std::sync::Arc;

use bt_core::{Cell, Layer, LayerId, LayerKind, Map, MapObject, ObjectLayer, Property, Value};
use bt_test_data::fixtures::grid_layer;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use glam::{DVec2, IVec2, UVec2};

use super::{atlas, rule_map, target_map};

pub fn below(rng: &mut Rng, n: u64) -> u64 {
    rng.below(n).unwrap_or(0)
}

pub fn chance(rng: &mut Rng, percent: u64) -> bool {
    below(rng, 100) < percent
}

pub fn pick<'a>(rng: &mut Rng, choices: &[&'a str]) -> &'a str {
    choices[below(rng, choices.len() as u64) as usize]
}

/// The layers of a random rule map, in the order they are added.
pub const RULE_LAYERS: [&str; 8] = [
    "input_set",
    "input_aux",
    "inputnot_set",
    "inputB_set",
    "output_set",
    "output_out",
    "outputX_set",
    "outputY_set",
];

/// A rule map's size: three rule blocks of 3×3 cells a column apart, and a border.
pub const RULE_SIZE: UVec2 = UVec2::new(13, 5);

/// A random rule map, and how to read it in a failure.
pub struct RandomRuleMap {
    pub map: Map,
    pub text: String,
    /// The options it has that region runs must handle, by name.
    pub features: Vec<&'static str>,
}

fn random_rule_options(rng: &mut Rng) -> Vec<(&'static str, Value)> {
    let mut options = Vec::new();
    if chance(rng, 15) {
        options.push(("NoOverlappingOutput", Value::Bool(true)));
    }
    if chance(rng, 15) {
        options.push(("ModX", Value::Int(2 + below(rng, 2) as i64)));
        options.push(("OffsetX", Value::Int(below(rng, 3) as i64)));
    }
    if chance(rng, 10) {
        options.push(("ModY", Value::Int(2)));
        options.push(("OffsetY", Value::Int(1)));
    }
    if chance(rng, 15) {
        options.push(("Probability", Value::Float(0.6)));
    }
    options
}

pub fn random_rule_map(rng: &mut Rng) -> RandomRuleMap {
    let (w, h) = (RULE_SIZE.x as usize, RULE_SIZE.y as usize);
    let mut cells: Vec<Vec<Vec<&str>>> = vec![vec![vec!["."; w]; h]; RULE_LAYERS.len()];
    let rules = 1 + below(rng, 3) as usize;
    for r in 0..rules {
        let x0 = 1 + 4 * r;
        for _ in 0..1 + below(rng, 3) {
            let (x, y) = (x0 + below(rng, 3) as usize, 1 + below(rng, 3) as usize);
            let layer = match below(rng, 10) {
                0..=5 => 0,
                6 => 1,
                7 | 8 => 2,
                _ => 3,
            };
            let tile = pick(
                rng,
                &["1", "2", "3", "4", "1", "2", "E", "N", "O", "X", "I", "2h"],
            );
            cells[layer][y][x] = tile;
        }
        for _ in 0..1 + below(rng, 3) {
            let (x, y) = (x0 + below(rng, 3) as usize, 1 + below(rng, 3) as usize);
            let layer = 4 + below(rng, 4) as usize;
            let tile = pick(rng, &["1", "2", "3", "5", "6", "E", "I", "4v"]);
            cells[layer][y][x] = tile;
        }
    }
    let mut options: Vec<(&'static str, Value)> = random_rule_options(rng);
    if chance(rng, 30) {
        options.push(("MatchInOrder", Value::Bool(chance(rng, 80))));
    }
    match below(rng, 20) {
        0 | 1 => options.push(("MatchOutsideMap", Value::Bool(true))),
        2 | 3 => options.push(("OverflowBorder", Value::Bool(true))),
        4 => options.push(("WrapBorder", Value::Bool(true))),
        _ => {}
    }
    if chance(rng, 10) {
        options.push(("DeleteTiles", Value::Bool(true)));
    }
    if chance(rng, 10) {
        options.push(("AutomappingRadius", Value::Int(1)));
    }
    let tiles = atlas();
    let mut builder = rule_map(&tiles, RULE_SIZE);
    let mut text = Vec::new();
    for (index, name) in RULE_LAYERS.iter().enumerate() {
        let grid: Vec<String> = cells[index].iter().map(|row| row.join(" ")).collect();
        let grid = grid.join("\n");
        let mut layer = Layer::new(
            LayerId(0),
            *name,
            LayerKind::Tile(grid_layer(&grid).unwrap()),
        );
        if index == 0 && chance(rng, 20) {
            layer
                .properties
                .insert("AutoEmpty".into(), Property::new(Value::Bool(true)));
            text.push(format!("{name} (AutoEmpty):\n{grid}"));
        } else if index == 6 && chance(rng, 20) {
            layer
                .properties
                .insert("Probability".into(), Property::new(Value::Float(3.0)));
            text.push(format!("{name} (Probability 3):\n{grid}"));
        } else {
            text.push(format!("{name}:\n{grid}"));
        }
        builder = builder.layer(layer);
    }
    let mut rectangles = Vec::new();
    for r in 0..rules {
        if !chance(rng, 15) {
            continue;
        }
        let mut values = random_rule_options(rng);
        if chance(rng, 20) {
            values.push(("Disabled", Value::Bool(true)));
        }
        if chance(rng, 30) {
            values.push(("IgnoreLock", Value::Bool(true)));
        }
        let mut rectangle = MapObject {
            id: bt_core::ObjectId(r as u32 + 1),
            pos: DVec2::new(((1 + 4 * r) * 16) as f64 - 2.0, 14.0),
            size: DVec2::new(52.0, 52.0),
            ..MapObject::default()
        };
        text.push(format!("rule_options around rule block {r}: {values:?}"));
        for (name, value) in values {
            rectangle
                .properties
                .insert(name.to_owned(), Property::new(value));
        }
        rectangles.push(rectangle);
    }
    if !rectangles.is_empty() {
        builder = builder.layer(Layer::new(
            LayerId(0),
            "rule_options",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(rectangles),
                ..ObjectLayer::default()
            }),
        ));
    }
    let mut map = builder.build().unwrap();
    text.push(format!("options: {options:?}"));
    let features = options
        .iter()
        .filter(|(_, value)| *value != Value::Bool(false))
        .map(|(name, _)| *name)
        .collect();
    for (name, value) in options {
        map.properties.insert(name.to_owned(), Property::new(value));
    }
    RandomRuleMap {
        map,
        text: text.join("\n"),
        features,
    }
}

/// A grid of `w`×`h` cells, each one of `tiles` with `percent` chance.
pub fn random_cells(rng: &mut Rng, w: i32, h: i32, percent: u64, tiles: &[&str]) -> String {
    (0..h)
        .map(|_| {
            (0..w)
                .map(|_| {
                    if chance(rng, percent) {
                        pick(rng, tiles)
                    } else {
                        "."
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A random map, where its content is, and how to read it in a failure.
pub struct RandomMap {
    pub map: Map,
    pub infinite: bool,
    /// Where the content is, and edits go.
    pub content: IRect,
    /// Where results are compared: the map, or a wide area around an infinite map's content.
    pub compared: IRect,
    pub text: String,
}

pub fn random_map(rng: &mut Rng) -> RandomMap {
    let tiles = atlas();
    let infinite = chance(rng, 15);
    let (w, h) = if infinite { (14, 10) } else { (12, 9) };
    let set = random_cells(rng, w, h, 55, &["1", "2", "3", "4", "2h"]);
    let aux = random_cells(rng, w, h, 20, &["1", "2"]);
    let out = chance(rng, 50).then(|| random_cells(rng, w, h, 10, &["5", "6"]));
    let locked = out.is_some() && chance(rng, 20);
    let origin = if infinite {
        IVec2::new(-7, -5)
    } else {
        IVec2::ZERO
    };
    let mut builder = target_map(&tiles, UVec2::new(w as u32, h as u32));
    if infinite {
        builder = builder.infinite();
    }
    builder = builder
        .tile_layer_at("set", origin, &set)
        .tile_layer_at("aux", origin, &aux);
    let mut text = format!("infinite: {infinite}\nset:\n{set}\naux:\n{aux}");
    if let Some(out) = &out {
        builder = builder.tile_layer_at("out", origin, out);
        text.push_str(&format!("\nout (locked: {locked}):\n{out}"));
    }
    let mut map = builder.build().unwrap();
    if locked && let Some(layer) = map.layers.iter_mut().find(|l| l.name == "out") {
        layer.locked = true;
    }
    let content = IRect::from_min_size(origin, IVec2::new(w, h));
    let compared = if infinite {
        IRect::from_min_size(IVec2::splat(-40), IVec2::splat(80))
    } else {
        content
    };
    RandomMap {
        map,
        infinite,
        content,
        compared,
        text,
    }
}

/// A random edit of the map's content: a rectangle repainted on `set` or `aux`.
pub struct Edit {
    pub rect: IRect,
    pub text: String,
}

pub fn random_edit(rng: &mut Rng, map: &mut Map, content: IRect) -> Edit {
    let (w, h) = (1 + below(rng, 4) as i32, 1 + below(rng, 3) as i32);
    let rect = IRect {
        x: content.x + below(rng, (content.w - w + 1) as u64) as i32,
        y: content.y + below(rng, (content.h - h + 1) as u64) as i32,
        w,
        h,
    };
    let mut text = format!("edit {rect:?}:");
    for p in rect.points() {
        let layer = if chance(rng, 80) { "set" } else { "aux" };
        let tile = [None, Some(1), Some(2), Some(3), Some(4)][below(rng, 5) as usize];
        let cell = tile.map_or(Cell::EMPTY, |id| {
            Cell::new(bt_core::TilesetSlot::new(0).unwrap(), bt_core::TileId(id))
        });
        if let Some(tiles) = map
            .layers
            .iter_mut()
            .find(|l| l.name == layer)
            .and_then(Layer::as_tile_layer_mut)
        {
            tiles.set_cell(p, cell).unwrap();
        }
        text.push_str(&format!(" {layer}({}, {})={tile:?}", p.x, p.y));
    }
    Edit { rect, text }
}

/// The cell at `p` of the tile layer named `name`, empty when there is none.
pub fn cell_at(map: &Map, name: &str, p: IVec2) -> Cell {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none()
            && layer.name == name
            && let Some(tiles) = layer.as_tile_layer()
        {
            found = Some(tiles.cell(p));
        }
    });
    found.unwrap_or(Cell::EMPTY)
}

/// The names of the tile layers of the maps.
pub fn tile_layer_names(maps: &[&Map]) -> Vec<String> {
    let mut names = Vec::new();
    for map in maps {
        map.for_each_layer(|layer, _| {
            if layer.as_tile_layer().is_some() && !names.contains(&layer.name) {
                names.push(layer.name.clone());
            }
        });
    }
    names
}
