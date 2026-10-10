//! The solver on terrain sets built in code: the brush's corners, edges and whole tiles, fills
//! with and without a chosen terrain, previews that are the paint, choices that depend only on
//! the seed and the cell, finite edges and infinite layers, symmetry, and the needs of cells no
//! tile fits.

#![cfg(test)]

mod common;

use bt_core::{Cell, Lane, TileId, WangSetType};
use bt_terrain::{
    BrushTarget, Cause, FillRequest, PaintRequest, Terrain, TerrainColor, TerrainError, fill,
    paint, preview_fill, preview_paint,
};
use glam::{IVec2, UVec2};

use common::*;

const SAND: TerrainColor = TerrainColor(1);
const GRASS: TerrainColor = TerrainColor(2);

fn all_sand_map(size: u32) -> (bt_core::Map, Terrain) {
    let map = map_with(
        sand_and_grass(),
        UVec2::splat(size),
        Some(combination_tile(2, [1, 1, 1, 1])),
    );
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    (map, terrain)
}

#[test]
fn painting_a_corner_turns_the_four_tiles_around_it() {
    let (mut map, terrain) = all_sand_map(6);
    let request = PaintRequest::new(GROUND, &terrain, GRASS)
        .targets(vec![BrushTarget::Corner(IVec2::new(3, 3))]);
    let report = paint(&mut map, &request).unwrap();
    assert!(report.invalid.is_empty(), "{report:?}");
    // The vertex at the top-left of (3, 3) is grass for the four tiles that meet there.
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(3, 3)),
        corners([1, 1, 1, 2])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(2, 3)),
        corners([2, 1, 1, 1])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(3, 2)),
        corners([1, 1, 2, 1])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(2, 2)),
        corners([1, 2, 1, 1])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(0, 0)),
        corners([1, 1, 1, 1])
    );
    assert_eq!(
        report.changed,
        Some(bt_utils::IRect::from_min_size(
            IVec2::new(2, 2),
            IVec2::splat(2)
        ))
    );
    assert_consistent(&map, &terrain, &[]);
}

#[test]
fn a_preview_is_the_paint_and_a_seed_decides_the_same_way_twice() {
    // Two plain sand tiles, so choices are random.
    let mut tileset = sand_and_grass();
    let set = &mut tileset.wang_sets[0];
    set.tiles.insert(TileId(100), plain(1));
    let map = map_with(tileset, UVec2::splat(8), None);
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    let cells: Vec<IVec2> = bt_utils::IRect::from_min_size(IVec2::ZERO, IVec2::splat(8))
        .points()
        .collect();
    let fill_with = |seed: u64| {
        FillRequest::new(GROUND, &terrain, SAND)
            .cells(cells.clone())
            .seed(seed)
    };
    let first = preview_fill(&map, &fill_with(7)).unwrap();
    assert_eq!(first, preview_fill(&map, &fill_with(7)).unwrap());
    let mut filled = map.clone();
    fill(&mut filled, &fill_with(7)).unwrap();
    for placed in &first.cells {
        assert_eq!(cell_at(&filled, placed.at), placed.cell);
    }
    // Another seed picks the other plain tile somewhere.
    let tiles = |seed| -> Vec<Cell> {
        preview_fill(&map, &fill_with(seed))
            .unwrap()
            .cells
            .iter()
            .map(|c| c.cell)
            .collect()
    };
    assert!(
        (8..16).any(|seed| tiles(seed) != tiles(7)),
        "seeds change the picks"
    );
}

/// A corner label with every corner the same terrain.
fn plain(color: u8) -> bt_core::WangId {
    bt_core::WangId::from_lanes(corners([color; 4]))
}

#[test]
fn a_fill_with_a_terrain_is_that_terrain_inside_and_connects_at_its_border() {
    let (map, terrain) = all_sand_map(8);
    let cells: Vec<IVec2> = bt_utils::IRect::from_min_size(IVec2::splat(2), IVec2::splat(4))
        .points()
        .collect();
    let mut filled = map.clone();
    let report = fill(
        &mut filled,
        &FillRequest::new(GROUND, &terrain, GRASS)
            .cells(cells)
            .seed(3),
    )
    .unwrap();
    assert!(report.invalid.is_empty(), "{report:?}");
    // The middle is grass; the ring meets the sand around it.
    for p in [IVec2::new(3, 3), IVec2::new(4, 4), IVec2::new(3, 4)] {
        assert_eq!(lanes_at(&filled, &terrain, p), corners([2, 2, 2, 2]), "{p}");
    }
    assert_eq!(
        lanes_at(&filled, &terrain, IVec2::new(2, 2)),
        corners([1, 2, 1, 1])
    );
    assert_consistent(&filled, &terrain, &[]);
}

#[test]
fn a_fill_without_a_terrain_is_any_terrain_that_connects() {
    let (map, terrain) = all_sand_map(8);
    let cells: Vec<IVec2> = bt_utils::IRect::from_min_size(IVec2::splat(1), IVec2::splat(6))
        .points()
        .collect();
    for seed in 0..8 {
        let mut filled = map.clone();
        let report = fill(
            &mut filled,
            &FillRequest::new(GROUND, &terrain, TerrainColor::NONE)
                .cells(cells.clone())
                .seed(seed),
        )
        .unwrap();
        assert!(report.invalid.is_empty(), "seed {seed}: {report:?}");
        assert_consistent(&filled, &terrain, &[]);
    }
}

#[test]
fn a_fill_that_refuses_empty_cells_gives_every_cell_a_tile() {
    // Over an empty layer nothing is wished for, and every tile fits as well as an empty cell.
    let map = map_with(sand_and_grass(), UVec2::splat(4), None);
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    let cells: Vec<IVec2> = bt_utils::IRect::from_min_size(IVec2::ZERO, IVec2::splat(4))
        .points()
        .collect();
    let without_empty = preview_fill(
        &map,
        &FillRequest::new(GROUND, &terrain, TerrainColor::NONE)
            .cells(cells)
            .allow_empty(false),
    )
    .unwrap();
    assert_eq!(without_empty.cells.len(), 16);
    assert!(without_empty.cells.iter().all(|c| !c.cell.is_empty()));
}

#[test]
fn a_cell_no_tile_fits_stays_and_says_what_it_needed() {
    // Plain sand and plain grass, and no tile where they meet.
    let labels = [corners([1, 1, 1, 1]), corners([2, 2, 2, 2])];
    let mut map = map_with(
        tileset(WangSetType::Corner, &["Sand", "Grass"], &labels),
        UVec2::splat(5),
        Some(TileId(0)),
    );
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    let before = map.clone();
    let report = paint(
        &mut map,
        &PaintRequest::new(GROUND, &terrain, GRASS)
            .targets(vec![BrushTarget::Corner(IVec2::new(2, 2))]),
    )
    .unwrap();
    // None of the four tiles at the vertex can have grass at it beside sand elsewhere.
    assert_eq!(
        report.invalid,
        [
            IVec2::new(1, 1),
            IVec2::new(2, 1),
            IVec2::new(1, 2),
            IVec2::new(2, 2)
        ]
    );
    assert_eq!(map, before, "every cell is left as it was");
    assert_eq!(report.changed, None);
    let need = &report.needed[0];
    assert_eq!(need.cell, IVec2::new(1, 1));
    assert_eq!(need.cause, Cause::NoTile);
    // The grass it must have, and the sand it wants where no tile leads from grass.
    assert_eq!(
        need.lanes,
        [None, Some(1), None, Some(2), None, None, None, None]
    );
    assert_needs_are_missing(&terrain, &report.needed);
}

#[test]
fn a_best_tile_of_probability_zero_is_never_placed() {
    let labels = [corners([1, 1, 1, 1]), corners([2, 2, 2, 2])];
    let mut tileset = tileset(WangSetType::Corner, &["Sand", "Grass"], &labels);
    tileset.tiles.insert(
        TileId(1),
        bt_core::TileData {
            probability: 0.0,
            ..Default::default()
        },
    );
    let map = map_with(tileset, UVec2::splat(3), Some(TileId(0)));
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    let preview = preview_paint(
        &map,
        &PaintRequest::new(GROUND, &terrain, GRASS).targets(vec![BrushTarget::Tile(IVec2::ONE)]),
    )
    .unwrap();
    let need = preview
        .needed
        .iter()
        .find(|n| n.cell == IVec2::ONE)
        .unwrap();
    assert_eq!(need.cause, Cause::ZeroProbability);
    assert_eq!(
        need.lanes,
        [None, Some(2), None, Some(2), None, Some(2), None, Some(2)]
    );
    assert_needs_are_missing(&terrain, &preview.needed);
}

#[test]
fn edge_sets_paint_the_edge_and_its_twin() {
    let labels: Vec<[u8; 8]> = every_combination(2).into_iter().map(edges).collect();
    let mut map = map_with(
        tileset(WangSetType::Edge, &["Path", "Grass"], &labels),
        UVec2::splat(4),
        Some(TileId(15)),
    );
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    // All grass; a path across the edge between (1, 1) and (2, 1).
    let report = paint(
        &mut map,
        &PaintRequest::new(GROUND, &terrain, TerrainColor(1)).targets(vec![BrushTarget::Edge {
            cell: IVec2::new(1, 1),
            side: Lane::Right,
        }]),
    )
    .unwrap();
    assert!(report.invalid.is_empty());
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(1, 1)),
        edges([2, 1, 2, 2])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(2, 1)),
        edges([2, 2, 2, 1])
    );
    assert_consistent(&map, &terrain, &[]);
    let corner =
        PaintRequest::new(GROUND, &terrain, TerrainColor(1)).targets(vec![BrushTarget::Edge {
            cell: IVec2::ONE,
            side: Lane::TopLeft,
        }]);
    assert_eq!(
        preview_paint(&map, &corner),
        Err(TerrainError::NotAnEdge(Lane::TopLeft))
    );
}

#[test]
fn infinite_layers_paint_anywhere_and_finite_ones_stop_at_their_edge() {
    let (mut finite, terrain) = all_sand_map(4);
    let report = paint(
        &mut finite,
        &PaintRequest::new(GROUND, &terrain, GRASS).targets(vec![BrushTarget::Corner(IVec2::ZERO)]),
    )
    .unwrap();
    assert!(report.invalid.is_empty());
    assert_eq!(
        lanes_at(&finite, &terrain, IVec2::ZERO),
        corners([1, 1, 1, 2])
    );

    // Sand far from the origin of an infinite layer (on an empty layer, the neighbours would
    // want "no terrain" beside the grass, which this set has no tile for).
    let sand = ["0 0 0 0 0 0 0"; 7].join("\n");
    let mut infinite = bt_test_data::fixtures::MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
        .tileset(std::sync::Arc::new(sand_and_grass()))
        .infinite()
        .tile_layer_at("ground", IVec2::new(-43, 22), &sand)
        .build()
        .unwrap();
    let terrain = Terrain::new(&infinite, slot(), 0).unwrap();
    let far = IVec2::new(-40, 25);
    let report = paint(
        &mut infinite,
        &PaintRequest::new(GROUND, &terrain, GRASS).targets(vec![BrushTarget::Tile(far)]),
    )
    .unwrap();
    assert!(report.invalid.is_empty(), "{report:?}");
    assert_eq!(lanes_at(&infinite, &terrain, far), corners([2, 2, 2, 2]));
    assert_eq!(
        lanes_at(&infinite, &terrain, far - IVec2::ONE),
        corners([1, 2, 1, 1])
    );
}

#[test]
fn symmetry_paints_the_half_turn_too_on_finite_maps_only() {
    let (mut map, terrain) = all_sand_map(6);
    let request = PaintRequest::new(GROUND, &terrain, GRASS)
        .targets(vec![BrushTarget::Tile(IVec2::new(1, 1))])
        .mirror(true);
    paint(&mut map, &request).unwrap();
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(1, 1)),
        corners([2, 2, 2, 2])
    );
    assert_eq!(
        lanes_at(&map, &terrain, IVec2::new(4, 4)),
        corners([2, 2, 2, 2])
    );
    assert_consistent(&map, &terrain, &[]);

    let infinite = bt_test_data::fixtures::MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
        .tileset(std::sync::Arc::new(sand_and_grass()))
        .infinite()
        .tile_layer("ground", "")
        .build()
        .unwrap();
    let terrain = Terrain::new(&infinite, slot(), 0).unwrap();
    assert_eq!(
        preview_paint(
            &infinite,
            &PaintRequest::new(GROUND, &terrain, GRASS).mirror(true)
        ),
        Err(TerrainError::MirrorOnInfiniteMap)
    );
}

#[test]
fn requests_name_what_they_cannot_find() {
    let (map, terrain) = all_sand_map(2);
    let request = PaintRequest::new(bt_core::LayerId(9), &terrain, GRASS);
    assert_eq!(
        preview_paint(&map, &request),
        Err(TerrainError::NoSuchLayer(bt_core::LayerId(9)))
    );
    assert_eq!(
        preview_paint(&map, &PaintRequest::new(GROUND, &terrain, TerrainColor(3))),
        Err(TerrainError::NoSuchColor { color: 3, count: 2 })
    );
}

/// Sand–grass and sand–water tiles, but none where grass meets water.
fn three_terrains() -> bt_core::Tileset {
    let mut labels: Vec<[u8; 8]> = every_combination(2).into_iter().map(corners).collect();
    for c in every_combination(2) {
        let water = c.map(|x| if x == 2 { 3 } else { 1 });
        if water != [1, 1, 1, 1] {
            labels.push(corners(water));
        }
    }
    tileset(WangSetType::Corner, &["Sand", "Grass", "Water"], &labels)
}

#[test]
fn a_stroke_says_what_it_pushed_back_and_the_tile_that_would_have_kept_it() {
    let tileset = three_terrains();
    let water = TileId(every_combination(2).len() as u32 + 14);
    let map = map_with(tileset, UVec2::splat(8), Some(water));
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    assert_eq!(lanes_at(&map, &terrain, IVec2::ZERO), corners([3, 3, 3, 3]));
    let request = PaintRequest::new(GROUND, &terrain, GRASS)
        .targets(vec![BrushTarget::Corner(IVec2::splat(4))]);
    let preview = preview_paint(&map, &request).unwrap();
    assert!(preview.invalid.is_empty(), "{preview:?}");
    let layer = map.layers[0].as_tile_layer().unwrap();
    let pushed = bt_terrain::pushed_back(&terrain, layer, &preview, GRASS);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].terrain, TerrainColor(3));
    let name = |c: TerrainColor| ["nothing", "Sand", "Grass", "Water"][usize::from(c.0)].to_owned();
    let text = bt_terrain::pushed_back_text(&terrain, &name, GRASS, &pushed).unwrap();
    assert!(
        text.starts_with("painting Grass pushed back Water at "),
        "{text}"
    );
    assert!(
        // Each of the four painted tiles lacks its own; ties go by the pattern's order.
        text.ends_with(": “Ground” has no tile with Grass at the top-right corner and Water at the other three corners"),
        "{text}"
    );
}

#[test]
fn a_stroke_over_another_sets_tiles_says_so() {
    // Two sets on one tileset: "Ground" (sand and grass) and "Paths" on tiles of their own.
    let mut tileset = sand_and_grass();
    let mut paths = tileset.wang_sets[0].clone();
    paths.name = "Paths".to_owned();
    paths.tiles = [(TileId(200), plain(1))].into_iter().collect();
    tileset.wang_sets.push(paths);
    let mut map = map_with(
        tileset,
        UVec2::splat(4),
        Some(combination_tile(2, [1, 1, 1, 1])),
    );
    map.layers[0]
        .as_tile_layer_mut()
        .unwrap()
        .set_cell(IVec2::ONE, Cell::new(slot(), TileId(200)))
        .unwrap();
    let terrain = Terrain::new(&map, slot(), 0).unwrap();
    let preview = preview_paint(
        &map,
        &PaintRequest::new(GROUND, &terrain, SAND).targets(vec![BrushTarget::Tile(IVec2::ONE)]),
    )
    .unwrap();
    let layer = map.layers[0].as_tile_layer().unwrap();
    let replaced = bt_terrain::replaced_sets(&map, &terrain, layer, &preview);
    assert_eq!(replaced.len(), 1);
    assert_eq!(replaced[0].set_name, "Paths");
    assert_eq!(replaced[0].cells, [IVec2::ONE]);
}
