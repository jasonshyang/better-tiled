//! Tile layers written as ASCII grids: a row a line, cells separated by spaces.
//!
//! A cell is `.` (empty) or a tile id with an optional `@slot` (the tileset's index in the map, 0
//! when left out) and flips: `h` and `v` mirror, `d` mirrors along the anti-diagonal (a 60° turn
//! on hexagonal maps), `x` turns 120° on hexagonal maps. So `3`, `3h`, `12@1vd`. Blank lines are
//! skipped, so a grid can be indented in a raw string.
//!
//! In rule maps, a capital letter is one of AutoMap's rule tiles ([`super::rule_tiles`]), in
//! slot [`super::RULE_SLOT`] unless an `@slot` says otherwise: `X` Negate, `I` Ignore, `N`
//! NonEmpty, `E` Empty, `O` Other.

use bt_core::{Cell, Flip, PlacedCell, TileId, TileLayer, TilesetSlot};
use glam::{IVec2, UVec2};

use super::error::FixtureError;
use super::rules::{RULE_SLOT, RULE_TILE_LETTERS};

/// The cells a grid places, row by row from (0, 0); empty cells are left out.
pub fn grid_cells(text: &str) -> Result<Vec<PlacedCell>, FixtureError> {
    Ok(parse(text)?.cells)
}

/// A finite tile layer the size of the grid, holding its cells.
pub fn grid_layer(text: &str) -> Result<TileLayer, FixtureError> {
    let grid = parse(text)?;
    let mut layer = TileLayer::finite(grid.size);
    for placed in grid.cells {
        layer
            .set_cell(placed.at, placed.cell)
            .map_err(|_| FixtureError::OutsideLayer {
                x: placed.at.x,
                y: placed.at.y,
            })?;
    }
    Ok(layer)
}

/// A parsed grid: its size in cells and the cells that are not empty.
struct Grid {
    size: UVec2,
    cells: Vec<PlacedCell>,
}

fn parse(text: &str) -> Result<Grid, FixtureError> {
    let mut cells = Vec::new();
    let mut width: Option<usize> = None;
    let mut height = 0u32;
    let rows = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    for (y, (line_index, line)) in (0i32..).zip(rows) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let expected = *width.get_or_insert(tokens.len());
        if tokens.len() != expected {
            return Err(FixtureError::RaggedGrid {
                line: line_index + 1,
                expected,
                found: tokens.len(),
            });
        }
        for (x, token) in (0i32..).zip(tokens) {
            if let Some(cell) = parse_cell(token).ok_or_else(|| FixtureError::UnknownToken {
                line: line_index + 1,
                token: token.to_owned(),
            })? {
                cells.push(PlacedCell {
                    at: IVec2::new(x, y),
                    cell,
                });
            }
        }
        height += 1;
    }
    let width = u32::try_from(width.unwrap_or(0)).unwrap_or(u32::MAX);
    Ok(Grid {
        size: UVec2::new(width, height),
        cells,
    })
}

/// A token's cell: `Some(None)` for `.`, `None` for a token that is not a cell.
fn parse_cell(token: &str) -> Option<Option<Cell>> {
    if token == "." {
        return Some(None);
    }
    let rule_tile = token
        .chars()
        .next()
        .and_then(|c| RULE_TILE_LETTERS.iter().position(|&letter| letter == c));
    let (id, mut rest, mut slot) = match rule_tile {
        Some(index) => (
            u32::try_from(index).ok()?,
            token.get(1..)?,
            u32::try_from(RULE_SLOT).ok()?,
        ),
        None => {
            let digits_end = token
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(token.len());
            let id: u32 = token.get(..digits_end)?.parse().ok()?;
            (id, token.get(digits_end..)?, 0u32)
        }
    };
    if let Some(after_at) = rest.strip_prefix('@') {
        let slot_end = after_at
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_at.len());
        slot = after_at.get(..slot_end)?.parse().ok()?;
        rest = after_at.get(slot_end..)?;
    }
    let mut flip = Flip::empty();
    for c in rest.chars() {
        let bit = match c {
            'h' => Flip::H,
            'v' => Flip::V,
            'd' => Flip::D,
            'x' => Flip::HEX120,
            _ => return None,
        };
        if flip.contains(bit) {
            return None;
        }
        flip |= bit;
    }
    let slot = TilesetSlot::new(usize::try_from(slot).ok()?)?;
    Some(Some(Cell::new(slot, TileId(id)).with_flip(flip)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_parse_every_token() {
        let layer = grid_layer(
            "
            0 . 3h
            12@1vd . 2x
            ",
        )
        .unwrap();
        assert_eq!(layer.size(), UVec2::new(3, 2));
        let slot = |i| TilesetSlot::new(i).unwrap();
        assert_eq!(layer.cell(IVec2::new(0, 0)), Cell::new(slot(0), TileId(0)));
        assert!(layer.cell(IVec2::new(1, 0)).is_empty());
        assert_eq!(
            layer.cell(IVec2::new(2, 0)),
            Cell::new(slot(0), TileId(3)).with_flip(Flip::H)
        );
        assert_eq!(
            layer.cell(IVec2::new(0, 1)),
            Cell::new(slot(1), TileId(12)).with_flip(Flip::V | Flip::D)
        );
        assert_eq!(
            layer.cell(IVec2::new(2, 1)),
            Cell::new(slot(0), TileId(2)).with_flip(Flip::HEX120)
        );
        assert_eq!(grid_cells("1 .\n. 2").unwrap().len(), 2);
    }

    #[test]
    fn grids_refuse_unknown_tokens_and_ragged_rows() {
        for token in ["a", "3q", "3hh", "@1", "3@", "-1"] {
            assert!(
                matches!(
                    grid_cells(token),
                    Err(FixtureError::UnknownToken { line: 1, .. })
                ),
                "{token}"
            );
        }
        assert!(matches!(
            grid_cells("1 2\n3"),
            Err(FixtureError::RaggedGrid {
                line: 2,
                expected: 2,
                found: 1
            })
        ));
    }
}
