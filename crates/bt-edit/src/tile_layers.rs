//! Tile layers made to a map's measure: empty ones for new layers, and copies of a layer's cells
//! moved, cut to a new size, or put in a layer of the other bounds. Tiled's `TileLayer::resize`
//! (`reference/tiled/src/libtiled/tilelayer.cpp`) is the rule for cutting: a resized layer keeps
//! the cells that land inside its new size, finite or infinite.

use bt_core::{Map, TileBounds, TileLayer};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::error::EditError;

/// An empty tile layer that fits `map`: its size, finite or infinite as the map is.
pub(crate) fn blank_tile_layer(map: &Map) -> TileLayer {
    empty_layer(map_size(map), bounds_of(map))
}

/// The map's size in cells: the bounds of a finite map, what an infinite one declares.
pub(crate) fn map_size(map: &Map) -> UVec2 {
    UVec2::new(map.width, map.height)
}

/// The bounds every tile layer of `map` has.
pub(crate) fn bounds_of(map: &Map) -> TileBounds {
    if map.infinite {
        TileBounds::Infinite
    } else {
        TileBounds::Finite
    }
}

/// The cells `0..size`, with a size past `i32` cut to it (no map reaches it).
pub(crate) fn cells_rect(size: UVec2) -> IRect {
    let size = size.min(UVec2::splat(i32::MAX.cast_unsigned())).as_ivec2();
    IRect::from_min_size(IVec2::ZERO, size)
}

/// `tiles` with every cell moved by `offset` into a layer of `size` and `bounds`, keeping the
/// cells that land in `0..size`: what Resize Map makes of a layer (Tiled crops infinite layers
/// the same way).
pub(crate) fn resized_tile_layer(
    tiles: &TileLayer,
    size: UVec2,
    offset: IVec2,
    bounds: TileBounds,
) -> Result<TileLayer, EditError> {
    let mut resized = empty_layer(size, bounds);
    let area = cells_rect(size);
    for placed in tiles.cells() {
        let at = placed.at.saturating_add(offset);
        if area.contains(at) {
            resized.set_cell(at, placed.cell)?;
        }
    }
    Ok(resized)
}

/// `tiles` with the same cells in a layer of `size` and `bounds`. A cell that does not fit a
/// finite layer is the model's error.
pub(crate) fn rebounded_tile_layer(
    tiles: &TileLayer,
    size: UVec2,
    bounds: TileBounds,
) -> Result<TileLayer, EditError> {
    let mut rebounded = empty_layer(size, bounds);
    for placed in tiles.cells() {
        rebounded.set_cell(placed.at, placed.cell)?;
    }
    Ok(rebounded)
}

/// An empty layer of `size` and `bounds`.
fn empty_layer(size: UVec2, bounds: TileBounds) -> TileLayer {
    match bounds {
        TileBounds::Finite => TileLayer::finite(size),
        TileBounds::Infinite => TileLayer::infinite(size),
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::grid_layer;

    use super::*;
    use crate::testutil::{grid_of, trimmed};

    #[test]
    fn resizing_moves_cells_and_keeps_those_inside() {
        let layer = grid_layer("1 2 3\n4 5 6").unwrap();
        let resized = resized_tile_layer(
            &layer,
            UVec2::new(2, 3),
            IVec2::new(-1, 1),
            TileBounds::Finite,
        )
        .unwrap();
        assert_eq!(resized.size(), UVec2::new(2, 3));
        assert_eq!(
            grid_of(&resized, cells_rect(UVec2::new(2, 3))),
            trimmed(
                "
                . .
                2 3
                5 6
                "
            )
        );
        let infinite =
            resized_tile_layer(&layer, UVec2::new(1, 1), IVec2::ZERO, TileBounds::Infinite)
                .unwrap();
        assert!(infinite.is_infinite());
        assert_eq!(infinite.cells().count(), 1, "infinite layers are cut too");
    }

    #[test]
    fn rebounding_keeps_every_cell_or_fails() {
        let layer = grid_layer("1 . 3").unwrap();
        let infinite = rebounded_tile_layer(&layer, layer.size(), TileBounds::Infinite).unwrap();
        assert!(infinite.is_infinite());
        assert_eq!(infinite.cells().count(), 2);
        assert!(matches!(
            rebounded_tile_layer(&layer, UVec2::new(2, 1), TileBounds::Finite),
            Err(EditError::Model(_))
        ));
    }
}
