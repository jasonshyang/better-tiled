//! What the terrain tests share: terrain sets built in code, maps to paint on, and the check that
//! every pair of neighbouring tiles agrees where they touch.

#![allow(dead_code, reason = "each test file uses some of these")]

use std::sync::Arc;

use bt_core::{
    Cell, Color, LayerId, Map, TileId, Tileset, TilesetSlot, Transformations, WangColor, WangId,
    WangSet, WangSetType,
};
use bt_terrain::{Needed, Terrain};
use bt_test_data::fixtures::{Atlas, MapBuilder};
use glam::{IVec2, UVec2};

pub fn slot() -> TilesetSlot {
    TilesetSlot::new(0).unwrap()
}

/// A terrain of a set.
pub fn terrain_color(name: &str) -> WangColor {
    WangColor {
        name: name.to_owned(),
        class: String::new(),
        color: Color::rgb(0, 0, 0),
        tile: None,
        probability: 1.0,
        properties: Default::default(),
    }
}

/// A tileset of 16×16 tiles with one terrain set of `kind` and `names.len()` terrains, tile `i`
/// labelled `labels[i]`.
pub fn tileset(kind: WangSetType, names: &[&str], labels: &[[u8; 8]]) -> Tileset {
    let mut tileset = (*Atlas::builder()
        .name("terrain")
        .grid(16, 16)
        .build_embedded()
        .unwrap()
        .tileset)
        .clone();
    let mut set = WangSet {
        name: "Ground".to_owned(),
        class: String::new(),
        kind,
        tile: None,
        colors: names.iter().map(|n| terrain_color(n)).collect(),
        tiles: Default::default(),
        properties: Default::default(),
    };
    for (i, lanes) in labels.iter().enumerate() {
        set.tiles
            .insert(TileId(i as u32), WangId::from_lanes(*lanes));
    }
    tileset.wang_sets.push(set);
    tileset.transformations = Transformations::empty();
    tileset
}

/// Corner lanes (top-right, bottom-right, bottom-left, top-left) as a label.
pub fn corners(c: [u8; 4]) -> [u8; 8] {
    [0, c[0], 0, c[1], 0, c[2], 0, c[3]]
}

/// Edge lanes (top, right, bottom, left) as a label.
pub fn edges(e: [u8; 4]) -> [u8; 8] {
    [e[0], 0, e[1], 0, e[2], 0, e[3], 0]
}

/// Every combination of `colors` terrains on the four lanes, in counting order.
pub fn every_combination(colors: u8) -> Vec<[u8; 4]> {
    let mut all = Vec::new();
    for a in 1..=colors {
        for b in 1..=colors {
            for c in 1..=colors {
                for d in 1..=colors {
                    all.push([a, b, c, d]);
                }
            }
        }
    }
    all
}

/// A complete two-terrain corner set ("Sand", "Grass"): tile `i` is the `i`-th combination.
pub fn sand_and_grass() -> Tileset {
    let labels: Vec<[u8; 8]> = every_combination(2).into_iter().map(corners).collect();
    tileset(WangSetType::Corner, &["Sand", "Grass"], &labels)
}

/// The tile of a corner set made by [`every_combination`] for `c`.
pub fn combination_tile(colors: u8, c: [u8; 4]) -> TileId {
    let index = every_combination(colors)
        .iter()
        .position(|x| *x == c)
        .unwrap();
    TileId(index as u32)
}

/// A map of `size` cells with `tileset` in slot 0 and one tile layer, "ground" (id 1), every cell
/// `fill` (or empty).
pub fn map_with(tileset: Tileset, size: UVec2, fill: Option<TileId>) -> Map {
    let row = |_| match fill {
        Some(tile) => tile.0.to_string(),
        None => ".".to_owned(),
    };
    let grid: Vec<String> = (0..size.y)
        .map(|_| (0..size.x).map(row).collect::<Vec<_>>().join(" "))
        .collect();
    MapBuilder::new(UVec2::splat(16), size)
        .tileset(Arc::new(tileset))
        .tile_layer("ground", &grid.join("\n"))
        .build()
        .unwrap()
}

pub const GROUND: LayerId = LayerId(1);

pub fn cell_at(map: &Map, p: IVec2) -> Cell {
    map.layer(GROUND)
        .and_then(|l| l.as_tile_layer())
        .unwrap()
        .cell(p)
}

/// The terrains on each lane of the cell at `p`.
pub fn lanes_at(map: &Map, terrain: &Terrain, p: IVec2) -> [u8; 8] {
    terrain.wang_of_cell(cell_at(map, p)).lanes()
}

/// On a square grid, every two neighbouring cells with tiles of the set agree where they touch,
/// except next to the cells in `invalid`. Panics naming the first disagreement.
pub fn assert_consistent(map: &Map, terrain: &Terrain, invalid: &[IVec2]) {
    let layer = map.layer(GROUND).and_then(|l| l.as_tile_layer()).unwrap();
    let mask: [bool; 8] = match terrain.kind() {
        WangSetType::Corner => [false, true, false, true, false, true, false, true],
        WangSetType::Edge => [true, false, true, false, true, false, true, false],
        WangSetType::Mixed => [true; 8],
    };
    for placed in layer.cells() {
        let p = placed.at;
        if invalid.contains(&p) || placed.cell.slot() != Some(terrain.slot()) {
            continue;
        }
        let a = terrain.wang_of_cell(placed.cell).lanes();
        // To the right: my right edge and corners against its left side.
        let pairs = [
            (IVec2::new(1, 0), [(2, 6), (1, 7), (3, 5)]),
            (IVec2::new(0, 1), [(4, 0), (5, 7), (3, 1)]),
        ];
        for (offset, touching) in pairs {
            let q = p + offset;
            let other = layer.cell(q);
            if invalid.contains(&q) || other.slot() != Some(terrain.slot()) {
                continue;
            }
            let b = terrain.wang_of_cell(other).lanes();
            for (mine, theirs) in touching {
                if mask[mine] {
                    assert_eq!(
                        a[mine], b[theirs],
                        "{p} lane {mine} against {q} lane {theirs}: {a:?} {b:?}"
                    );
                }
            }
        }
        let diagonal = p + IVec2::ONE;
        let other = layer.cell(diagonal);
        if mask[3] && !invalid.contains(&diagonal) && other.slot() == Some(terrain.slot()) {
            let b = terrain.wang_of_cell(other).lanes();
            assert_eq!(a[3], b[7], "{p} against {diagonal}");
        }
    }
}

/// Every need names a pattern the set has no tile for (no placeable tile, for a zero
/// probability).
pub fn assert_needs_are_missing(terrain: &Terrain, needed: &[Needed]) {
    for need in needed {
        let placeable = need.cause == bt_terrain::Cause::ZeroProbability;
        assert!(
            !terrain.has_tile_with(&need.lanes, placeable),
            "the set has a tile for {need:?}"
        );
    }
}

/// Every two neighbouring cells a preview places agree where they touch (square grids).
pub fn assert_preview_consistent(preview: &bt_terrain::Preview, terrain: &Terrain, what: &str) {
    let placed: std::collections::HashMap<IVec2, Cell> =
        preview.cells.iter().map(|c| (c.at, c.cell)).collect();
    let mask: [bool; 8] = match terrain.kind() {
        WangSetType::Corner => [false, true, false, true, false, true, false, true],
        WangSetType::Edge => [true, false, true, false, true, false, true, false],
        WangSetType::Mixed => [true; 8],
    };
    for (&p, &cell) in &placed {
        let a = terrain.wang_of_cell(cell).lanes();
        for (offset, touching) in [
            (IVec2::new(1, 0), [(2, 6), (1, 7), (3, 5)]),
            (IVec2::new(0, 1), [(4, 0), (5, 7), (3, 1)]),
        ] {
            let Some(&other) = placed.get(&(p + offset)) else {
                continue;
            };
            let b = terrain.wang_of_cell(other).lanes();
            for (mine, theirs) in touching {
                if mask[mine] {
                    assert_eq!(
                        a[mine],
                        b[theirs],
                        "{what}: {p} lane {mine} against {} lane {theirs}",
                        p + offset
                    );
                }
            }
        }
    }
}
