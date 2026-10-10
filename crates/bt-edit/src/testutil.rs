//! What the crate's tests share: regions drawn as text, cells of the first tileset, and maps of
//! the fixture tilesets.

use bt_core::{Cell, TileId, TilesetSlot};
use glam::IVec2;

use crate::region::TileRegion;

/// A region drawn as rows of `#` (in) and `.` (out) from (0, 0); blank lines are skipped.
pub(crate) fn drawn(text: &str) -> TileRegion {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .zip(0..)
        .flat_map(|(line, y)| {
            line.trim()
                .chars()
                .zip(0..)
                .filter(|(c, _)| *c == '#')
                .map(move |(_, x)| IVec2::new(x, y))
        })
        .collect()
}

/// Tile `id` of the tileset in `slot`.
pub(crate) fn tile_of(slot: usize, id: u32) -> Cell {
    Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id))
}

/// Tile `id` of the first tileset.
pub(crate) fn tile(id: u32) -> Cell {
    tile_of(0, id)
}

/// The cells of a tile layer in `rect` as an ASCII grid (the fixtures' notation): what a test
/// compares a layer against.
pub(crate) fn grid_of(layer: &bt_core::TileLayer, rect: bt_utils::IRect) -> String {
    (rect.y..rect.y + rect.h)
        .map(|y| {
            (rect.x..rect.x + rect.w)
                .map(|x| cell_token(layer.cell(IVec2::new(x, y))))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell_token(cell: Cell) -> String {
    let Some(slot) = cell.slot() else {
        return ".".to_owned();
    };
    let mut token = cell.tile().0.to_string();
    if slot.index() != 0 {
        token.push_str(&format!("@{}", slot.index()));
    }
    for (flag, letter) in [
        (bt_core::Flip::H, 'h'),
        (bt_core::Flip::V, 'v'),
        (bt_core::Flip::D, 'd'),
        (bt_core::Flip::HEX120, 'x'),
    ] {
        if cell.flip().contains(flag) {
            token.push(letter);
        }
    }
    token
}

/// The text of a grid with its lines trimmed, so a test can indent the grid it expects.
pub(crate) fn trimmed(grid: &str) -> String {
    grid.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A grid of every shape: orthogonal, isometric, oblique, and staggered and hexagonal along both
/// axes with both indexes.
pub(crate) fn every_grid() -> Vec<bt_core::MapGeometry> {
    use bt_core::{MapGeometry, Orientation, StaggerAxis, StaggerIndex};
    let size = IVec2::new(12, 10);
    let mut grids = vec![
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), size),
        MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), size),
        MapGeometry {
            skew: IVec2::new(8, 0),
            ..MapGeometry::new(Orientation::Oblique, IVec2::splat(16), size)
        },
    ];
    for orientation in [Orientation::Staggered, Orientation::Hexagonal] {
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            for index in [StaggerIndex::Odd, StaggerIndex::Even] {
                grids.push(MapGeometry {
                    stagger_axis: axis,
                    stagger_index: index,
                    hex_side_length: if orientation == Orientation::Hexagonal {
                        16
                    } else {
                        0
                    },
                    ..MapGeometry::new(orientation, IVec2::new(32, 32), size)
                });
            }
        }
    }
    grids
}

/// A drawn atlas tileset called `name` (16×16 tiles, 4 by 4).
pub(crate) fn atlas(name: &str) -> std::sync::Arc<bt_core::Tileset> {
    bt_test_data::fixtures::Atlas::builder()
        .name(name)
        .build_embedded()
        .unwrap()
        .tileset
}

/// The cells of the tile layer with `id`.
pub(crate) fn tiles(map: &bt_core::Map, id: bt_core::LayerId) -> &bt_core::TileLayer {
    map.layer(id)
        .and_then(bt_core::Layer::as_tile_layer)
        .unwrap()
}

/// The cells of the tile layer with `id` over the map's area, as a grid.
pub(crate) fn map_grid(map: &bt_core::Map, id: bt_core::LayerId) -> String {
    let size = IVec2::new(map.width as i32, map.height as i32);
    grid_of(
        tiles(map, id),
        bt_utils::IRect::from_min_size(IVec2::ZERO, size),
    )
}

/// The cells of a clip's layer over the clip's rectangle, as a grid.
pub(crate) fn clip_grid(clip: &crate::clip::TileClip, layer: usize) -> String {
    grid_of(
        &clip.layers[layer].cells,
        bt_utils::IRect::from_min_size(IVec2::ZERO, clip.size),
    )
}

/// Asserts that a map breaks no invariant.
pub(crate) fn assert_valid(map: &bt_core::Map) {
    let violations = map.validate();
    assert!(violations.is_empty(), "{violations:?}");
}
