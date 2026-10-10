//! The selection tools' areas: flood fills, the magic wand, Select Same Tile, Bucket Fill's area
//! and inversion. Ported from Tiled's `TilePainter::fillRegion` and its callers
//! (`reference/tiled/src/tiled/tilepainter.cpp`, `magicwandtool.cpp`, `selectsametiletool.cpp`,
//! `bucketfilltool.cpp`).

use bt_core::{Cell, MapGeometry, TileLayer};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::region::TileRegion;

/// The cells connected to `start` for which `matches` holds, inside `bounds`: Tiled's scanline
/// fill. Cells connect as `MapGeometry::neighbors` connects them: four ways on square grids, six
/// on hexagonal and staggered grids. Empty when `start` is outside `bounds` or does not match.
pub fn flood_fill(
    layer: &TileLayer,
    start: IVec2,
    bounds: IRect,
    geometry: &MapGeometry,
    matches: impl Fn(Cell) -> bool,
) -> TileRegion {
    let mut region = TileRegion::new();
    if !bounds.contains(start) || !matches(layer.cell(start)) {
        return region;
    }
    let right_edge = bounds.x + bounds.w;
    let bottom_edge = bounds.y + bounds.h;
    let mut seeds = vec![start];
    while let Some(seed) = seeds.pop() {
        if region.contains(seed) {
            continue;
        }
        // The run through the seed. Its cells are not filled yet: a filled cell next to a
        // matching one would have taken the whole run with it.
        let y = seed.y;
        let mut left = seed.x;
        while left > bounds.x && matches(layer.cell(IVec2::new(left - 1, y))) {
            left -= 1;
        }
        let mut right = seed.x + 1;
        while right < right_edge && matches(layer.cell(IVec2::new(right, y))) {
            right += 1;
        }
        region.insert_rect(IRect {
            x: left,
            y,
            w: right - left,
            h: 1,
        });
        for row in [y - 1, y + 1] {
            if row < bounds.y || row >= bottom_edge {
                continue;
            }
            let Some(columns) = neighbour_columns(geometry, y, left, right - 1, row) else {
                continue;
            };
            let mut x = columns.first.max(bounds.x);
            let end = (columns.last + 1).min(right_edge);
            while x < end {
                let p = IVec2::new(x, row);
                if !region.contains(p) && matches(layer.cell(p)) {
                    // One seed per run: its fill takes the rest of the run.
                    seeds.push(p);
                    while x < end && matches(layer.cell(IVec2::new(x, row))) {
                        x += 1;
                    }
                } else {
                    x += 1;
                }
            }
        }
    }
    region
}

/// Columns `first..=last` of a row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Columns {
    first: i32,
    last: i32,
}

/// The columns of `row` holding a neighbour of a cell in columns `first..=last` of row `y`.
/// They are contiguous on every grid, so the run's two ends decide them.
fn neighbour_columns(
    geometry: &MapGeometry,
    y: i32,
    first: i32,
    last: i32,
    row: i32,
) -> Option<Columns> {
    let in_row = |x: i32| {
        geometry
            .neighbors(IVec2::new(x, y))
            .filter(move |n| n.y == row)
            .map(|n| n.x)
    };
    Some(Columns {
        first: in_row(first).min()?,
        last: in_row(last).max()?,
    })
}

/// The cells a finite layer can hold, or the chunks an infinite one has allocated (Tiled's
/// `TileLayer::bounds` and `rect`): where fills and the wand may spread when nothing else
/// bounds them.
fn layer_area(layer: &TileLayer) -> IRect {
    if layer.is_infinite() {
        layer.chunk_bounds().unwrap_or_default()
    } else {
        IRect::from_min_size(IVec2::ZERO, clamp_size(layer.size()))
    }
}

fn clamp_size(size: UVec2) -> IVec2 {
    size.min(UVec2::splat(i32::MAX.cast_unsigned())).as_ivec2()
}

/// The Magic Wand's selection: the cells connected to `start` holding what `start` holds (tile
/// and flips) or any of `extra` (the kinds the drag went over), within the layer's area.
pub fn magic_wand(
    layer: &TileLayer,
    start: IVec2,
    extra: &[Cell],
    geometry: &MapGeometry,
) -> TileRegion {
    let target = layer.cell(start);
    flood_fill(layer, start, layer_area(layer), geometry, |cell| {
        cell == target || extra.contains(&cell)
    })
}

/// Select Same Tile: every cell of the layer holding one of `cells`, tile and flips. Matching
/// the empty cell selects the empty cells of the layer's area (the map on a finite map, the
/// allocated chunks on an infinite one).
pub fn select_same_tile(layer: &TileLayer, cells: &[Cell]) -> TileRegion {
    let mut region: TileRegion = layer
        .cells()
        .filter(|placed| cells.contains(&placed.cell))
        .map(|placed| placed.at)
        .collect();
    if cells.iter().any(|cell| cell.is_empty()) {
        let filled: TileRegion = layer.cells().map(|placed| placed.at).collect();
        let empty = TileRegion::from_rect(layer_area(layer)).subtract(&filled);
        region = region.union(&empty);
    }
    region
}

/// Bucket Fill's area from `start`: the cells connected to it holding what it holds or any of
/// `extra`. On a finite map the flood spreads over the whole layer and is then clipped to the
/// selection; on an infinite map it spreads within the selection's bounding rectangle when
/// there is a selection (starting inside it) and is clipped to it, else within the allocated
/// chunks. An empty selection does not restrict.
pub fn fill_area(
    layer: &TileLayer,
    start: IVec2,
    extra: &[Cell],
    geometry: &MapGeometry,
    selection: &TileRegion,
) -> TileRegion {
    let bounds = if !layer.is_infinite() {
        layer_area(layer)
    } else if let Some(selected) = selection.bounds() {
        if !selection.contains(start) {
            return TileRegion::new();
        }
        selected
    } else {
        layer_area(layer)
    };
    let target = layer.cell(start);
    let area = flood_fill(layer, start, bounds, geometry, |cell| {
        cell == target || extra.contains(&cell)
    });
    if selection.is_empty() {
        area
    } else {
        area.intersect(selection)
    }
}

/// Invert Selection: the cells of `bounds` not in `region`.
pub fn invert(region: &TileRegion, bounds: IRect) -> TileRegion {
    TileRegion::from_rect(bounds).subtract(region)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};

    use bt_core::{Map, Orientation, StaggerAxis, StaggerIndex};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder, grid_layer};
    use bt_utils::rng::Rng;

    use super::*;
    use crate::testutil::{drawn, tile};

    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::new(6, 4))
    }

    #[test]
    fn a_flood_stops_at_other_tiles_and_never_goes_diagonally_on_squares() {
        let layer = grid_layer(
            "
            1 1 2 1 1 1
            1 2 1 1 2 1
            2 1 1 2 1 1
            1 1 2 1 1 1
            ",
        )
        .unwrap();
        let wand = magic_wand(&layer, IVec2::new(0, 0), &[], &square());
        assert_eq!(
            wand,
            drawn("##....\n#....."),
            "(1, 2) touches (0, 1) only at a corner"
        );
        let wider = magic_wand(&layer, IVec2::new(0, 0), &[tile(2)], &square());
        assert_eq!(wider.len(), 24, "with 2 as well, everything connects");
    }

    #[test]
    fn select_same_tile_takes_every_matching_cell_and_empty_space() {
        let layer = grid_layer("1 . 2\n2 1 .").unwrap();
        assert_eq!(select_same_tile(&layer, &[tile(1)]), drawn("#..\n.#."));
        assert_eq!(
            select_same_tile(&layer, &[Cell::EMPTY]),
            drawn(".#.\n..#"),
            "empty cells of a finite layer"
        );
        let flipped = tile(1).with_flip(bt_core::Flip::H);
        assert!(
            select_same_tile(&layer, &[flipped]).is_empty(),
            "flips count"
        );
    }

    #[test]
    fn fills_on_finite_maps_spread_through_the_layer_then_keep_to_the_selection() {
        // The 1s form a U: from inside the selection the flood goes round through cells outside
        // it, then only the selected ones are kept.
        let layer = grid_layer(
            "
            1 2 1
            1 2 1
            1 1 1
            ",
        )
        .unwrap();
        let geometry = MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(3));
        let selection = drawn("#.#\n#.#");
        let area = fill_area(&layer, IVec2::ZERO, &[], &geometry, &selection);
        assert_eq!(area, selection);
        let unbounded = fill_area(&layer, IVec2::ZERO, &[], &geometry, &TileRegion::new());
        assert_eq!(unbounded.len(), 7);
    }

    #[test]
    fn fills_on_infinite_maps_keep_within_the_selection_or_the_chunks() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .infinite()
            .tileset(atlas.tileset)
            .tile_layer_at("ground", IVec2::new(-3, 2), "1")
            .build()
            .unwrap();
        let layer = map.layers[0].as_tile_layer().unwrap();
        let geometry = map.geometry();
        // Empty space floods the allocated chunk, 16×16 less the one tile.
        let area = fill_area(layer, IVec2::new(-1, 5), &[], &geometry, &TileRegion::new());
        assert_eq!(area.len(), 255);
        assert_eq!(
            area.bounds(),
            Some(IRect {
                x: -16,
                y: 0,
                w: 16,
                h: 16
            })
        );
        let selection = TileRegion::from_rect(IRect {
            x: 100,
            y: 100,
            w: 3,
            h: 2,
        });
        let inside = fill_area(layer, IVec2::new(101, 100), &[], &geometry, &selection);
        assert_eq!(
            inside, selection,
            "a selection bounds the flood far from any chunk"
        );
        let outside = fill_area(layer, IVec2::new(0, 0), &[], &geometry, &selection);
        assert!(
            outside.is_empty(),
            "a start outside the selection fills nothing"
        );
    }

    #[test]
    fn inverting_takes_the_rest_of_the_bounds() {
        let bounds = IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
        };
        assert_eq!(invert(&drawn("#..\n.#."), bounds), drawn(".##\n#.#"));
    }

    /// A breadth-first search over `MapGeometry::neighbors`: what a flood fill must equal.
    fn searched(
        layer: &TileLayer,
        start: IVec2,
        bounds: IRect,
        geometry: &MapGeometry,
    ) -> BTreeSet<(i32, i32)> {
        let target = layer.cell(start);
        let mut found = BTreeSet::new();
        if !bounds.contains(start) {
            return found;
        }
        let mut queue = VecDeque::from([start]);
        found.insert((start.x, start.y));
        while let Some(p) = queue.pop_front() {
            for n in geometry.neighbors(p) {
                if bounds.contains(n) && layer.cell(n) == target && found.insert((n.x, n.y)) {
                    queue.push_back(n);
                }
            }
        }
        found
    }

    fn grids() -> Vec<MapGeometry> {
        let size = IVec2::new(9, 7);
        let mut grids = vec![
            MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), size),
            MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), size),
        ];
        for orientation in [Orientation::Hexagonal, Orientation::Staggered] {
            for axis in [StaggerAxis::X, StaggerAxis::Y] {
                for index in [StaggerIndex::Odd, StaggerIndex::Even] {
                    let mut grid = MapGeometry::new(orientation, IVec2::new(32, 32), size);
                    grid.stagger_axis = axis;
                    grid.stagger_index = index;
                    grid.hex_side_length = if orientation == Orientation::Hexagonal {
                        16
                    } else {
                        0
                    };
                    grids.push(grid);
                }
            }
        }
        grids
    }

    #[test]
    fn floods_equal_a_plain_search_on_every_grid() {
        let fuzz = Fuzz::from_env(0x00F1_00D5, 300);
        let grids = grids();
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let mut layer = TileLayer::finite(UVec2::new(9, 7));
            for p in (IRect {
                x: 0,
                y: 0,
                w: 9,
                h: 7,
            })
            .points()
            {
                // Two kinds and empty cells, so regions are ragged.
                match rng.below(3) {
                    Some(0) => {}
                    Some(kind) => {
                        layer.set_cell(p, tile(kind as u32)).unwrap();
                    }
                    None => {}
                }
            }
            let start = IVec2::new(rng.below(9).unwrap() as i32, rng.below(7).unwrap() as i32);
            let bounds = IRect {
                x: rng.below(3).unwrap() as i32 - 1,
                y: rng.below(3).unwrap() as i32 - 1,
                w: 6 + rng.below(5).unwrap() as i32,
                h: 4 + rng.below(5).unwrap() as i32,
            };
            for geometry in &grids {
                let target = layer.cell(start);
                let flood = flood_fill(&layer, start, bounds, geometry, |cell| cell == target);
                let flooded: BTreeSet<(i32, i32)> = flood.iter().map(|p| (p.x, p.y)).collect();
                assert_eq!(
                    flooded,
                    searched(&layer, start, bounds, geometry),
                    "case {case} ({}) on {:?} {:?} {:?}, from {start} in {bounds:?}",
                    fuzz.reproduce(),
                    geometry.orientation,
                    geometry.stagger_axis,
                    geometry.stagger_index,
                );
            }
        }
    }

    #[test]
    fn hexagonal_floods_connect_six_ways() {
        // Stagger axis y, odd rows shifted: (1, 1) touches (1, 0) and (2, 0) above it.
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map: Map = MapBuilder::new(UVec2::splat(32), UVec2::new(3, 2))
            .orientation(Orientation::Hexagonal)
            .hex_side_length(16)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd)
            .tileset(atlas.tileset)
            .tile_layer("ground", "2 1 2\n2 1 2")
            .build()
            .unwrap();
        let layer = map.layers[0].as_tile_layer().unwrap();
        let wand = magic_wand(layer, IVec2::new(2, 0), &[], &map.geometry());
        assert_eq!(
            wand,
            drawn("..#\n..#"),
            "(2, 0) reaches (2, 1) straight down"
        );
        let ones = magic_wand(layer, IVec2::new(1, 1), &[], &map.geometry());
        assert_eq!(ones, drawn(".#.\n.#."));
        let twos = magic_wand(layer, IVec2::new(0, 0), &[], &map.geometry());
        assert_eq!(
            twos,
            drawn("#..\n#.."),
            "(0, 0) reaches (0, 1), its lower-left"
        );
    }
}
