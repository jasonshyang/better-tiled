//! What the integration tests share: finding Tiled's cases and the packs, reading maps, building
//! rule sets, and comparing tile layers the way Tiled's own harness does.

#![allow(dead_code, reason = "each test file uses part of the helpers")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::RuleSet;
use bt_core::{Cell, Flip, Layer, LayerKind, Map, TileId, TileLayer, Tileset};
use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
use bt_tmx::Loader;
use glam::{IVec2, UVec2};

/// The folder of Tiled's automapping case `name`, or `None` when the reference is missing and
/// the skip variable is set.
pub fn tiled_case(name: &str) -> Option<PathBuf> {
    bt_test_data::test_data(&format!("reference/tiled/tests/automapping/{name}"))
}

/// The folder of Tiled's automapping cases.
pub fn tiled_cases_dir() -> Option<PathBuf> {
    bt_test_data::test_data("reference/tiled/tests/automapping")
}

/// Reads a map, failing the test when it cannot be read.
pub fn load_map(loader: &mut Loader, path: &Path) -> Map {
    match loader.load_map(path) {
        Ok(loaded) => loaded.value,
        Err(error) => panic!("{} cannot be read: {error}", path.display()),
    }
}

/// A rule set of the rule maps at `paths`, read through `loader`.
pub fn rule_set(loader: &mut Loader, paths: &[PathBuf]) -> RuleSet {
    let mut builder = RuleSet::builder().custom_types(Arc::clone(loader.custom_types()));
    for path in paths {
        builder = builder.rule_map(path.clone(), Arc::new(load_map(loader, path)));
    }
    builder.build()
}

/// The drawn atlas maps and rule maps built in tests share: 16×16 tiles in 4 columns and 4 rows.
pub fn atlas() -> Arc<Tileset> {
    Atlas::builder().build_embedded().unwrap().tileset
}

/// A rule map of `size` with `atlas` in slot 0 and the rule tiles in slot 1, so grids name
/// atlas tiles by number and rule tiles by letter.
pub fn rule_map(atlas: &Arc<Tileset>, size: UVec2) -> MapBuilder {
    MapBuilder::new(UVec2::splat(16), size)
        .tileset(Arc::clone(atlas))
        .tileset(rule_tiles().unwrap())
}

/// A map of `size` with `atlas` in slot 0.
pub fn target_map(atlas: &Arc<Tileset>, size: UVec2) -> MapBuilder {
    MapBuilder::new(UVec2::splat(16), size).tileset(Arc::clone(atlas))
}

/// A rule set of rule maps built in memory, named `/rules/0.tmx`, `/rules/1.tmx`…, in order.
pub fn rules_of(maps: Vec<Map>) -> RuleSet {
    let mut builder = RuleSet::builder();
    for (i, map) in maps.into_iter().enumerate() {
        builder = builder.rule_map(format!("/rules/{i}.tmx"), Arc::new(map));
    }
    builder.build()
}

/// The first tile layer named `name`.
pub fn tiles<'m>(map: &'m Map, name: &str) -> &'m TileLayer {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name {
            found = layer.as_tile_layer();
        }
    });
    found.unwrap_or_else(|| panic!("the map has no tile layer {name}"))
}

/// A tile layer of `map` as an ASCII grid of the map's size (the fixtures' notation: `.` for
/// empty, the tile id, `@slot` unless the slot is 0, and the flips).
pub fn grid_of(map: &Map, name: &str) -> String {
    let layer = tiles(map, name);
    let mut rows = Vec::new();
    for y in 0..i32::try_from(map.height).unwrap() {
        let mut row = Vec::new();
        for x in 0..i32::try_from(map.width).unwrap() {
            let cell = layer.cell(IVec2::new(x, y));
            row.push(cell_token(cell));
        }
        rows.push(row.join(" "));
    }
    rows.join("\n")
}

/// A cell in the fixtures' grid notation.
pub fn cell_token(cell: Cell) -> String {
    let Some(slot) = cell.slot() else {
        return ".".to_owned();
    };
    let mut token = cell.tile().0.to_string();
    if slot.index() != 0 {
        token.push_str(&format!("@{}", slot.index()));
    }
    for (flag, letter) in [
        (Flip::H, 'h'),
        (Flip::V, 'v'),
        (Flip::D, 'd'),
        (Flip::HEX120, 'x'),
    ] {
        if cell.flip().contains(flag) {
            token.push(letter);
        }
    }
    token
}

/// A grid written in a raw string, its lines trimmed, as `grid_of` gives it.
pub fn grid(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A cell named by what it shows rather than by its slot: its tileset (by value, or by file),
/// tile and flips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownCell {
    pub tileset: String,
    pub tile: TileId,
    pub flip: Flip,
}

/// What `cell` of `map` shows; `None` when it is empty.
pub fn shown(map: &Map, cell: Cell) -> Option<ShownCell> {
    let slot = cell.slot()?;
    let reference = map.tilesets.get(slot.index())?;
    let tileset = match &reference.file {
        Some(file) => file.path.display().to_string(),
        None => format!("{:p}", Arc::as_ptr(&reference.tileset)),
    };
    Some(ShownCell {
        tileset,
        tile: cell.tile(),
        flip: cell.flip(),
    })
}

fn top_level_layers(map: &Map) -> Vec<&Layer> {
    map.layers.iter().collect()
}

/// Asserts what Tiled's `test_automapping.cpp` asserts: the same number of layers, of the same
/// types and names, and for tile layers the same cells (by what they show), cell by cell.
pub fn assert_same_tile_layers(case: &str, actual: &Map, expected: &Map) {
    let actual_layers = top_level_layers(actual);
    let expected_layers = top_level_layers(expected);
    let names = |layers: &[&Layer]| -> Vec<String> {
        layers
            .iter()
            .map(|l| format!("{} ({})", l.name, l.kind_name()))
            .collect()
    };
    assert_eq!(
        names(&actual_layers),
        names(&expected_layers),
        "{case}: the layers differ"
    );
    for (a, e) in actual_layers.iter().zip(&expected_layers) {
        let (LayerKind::Tile(a_tiles), LayerKind::Tile(e_tiles)) = (&a.kind, &e.kind) else {
            continue;
        };
        let mut differences = Vec::new();
        let mut positions: Vec<glam::IVec2> = a_tiles
            .cells()
            .chain(e_tiles.cells())
            .map(|c| c.at)
            .collect();
        positions.sort_by_key(|p| (p.y, p.x));
        positions.dedup();
        for p in positions {
            let seen = shown(actual, a_tiles.cell(p));
            let wanted = shown(expected, e_tiles.cell(p));
            if seen != wanted {
                differences.push(format!(
                    "({}, {}): {:?} where Tiled has {:?}",
                    p.x,
                    p.y,
                    seen.map(|s| s.tile.0),
                    wanted.map(|s| s.tile.0)
                ));
            }
        }
        assert!(
            differences.is_empty(),
            "{case}: layer “{}” differs from Tiled's result:\n{}",
            a.name,
            differences.join("\n")
        );
    }
}
