//! Tile layers made to a map's measure: empty ones for new layers, and copies of a layer's cells
//! in a layer of another size or bounds.

use bt_core::{Map, TileBounds, TileLayer};
use glam::UVec2;

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
