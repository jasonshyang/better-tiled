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
