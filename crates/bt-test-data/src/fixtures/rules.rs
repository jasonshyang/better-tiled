//! AutoMap's rule tiles for rule maps built in tests: a drawn copy of Tiled's built-in rule
//! tileset (`:/automap-tiles.tsx`), each tile with the `MatchType` property that gives it its
//! meaning, in Tiled's order. Grids name them by letter ([`super::grid_cells`]).

use std::sync::Arc;

use bt_core::{Properties, Property, TileData, TileId, Tileset, Value};

use super::atlas::Atlas;
use super::error::FixtureError;

/// The slot grids put rule tiles in unless an `@slot` says otherwise: a rule map's second
/// tileset, after the tiles it matches.
pub const RULE_SLOT: usize = 1;

/// The letters grids write the rule tiles with, by tile id: Negate, Ignore, NonEmpty, Empty,
/// Other.
pub(crate) const RULE_TILE_LETTERS: [char; 5] = ['X', 'I', 'N', 'E', 'O'];

/// The `MatchType` of each rule tile, by tile id, as in Tiled's rule tileset.
const MATCH_TYPES: [&str; 5] = ["Negate", "Ignore", "NonEmpty", "Empty", "Other"];

/// The rule tileset: five 32×32 tiles in a row, tile `i` with the `MatchType` Tiled's built-in
/// tile `i` has.
pub fn rule_tiles() -> Result<Arc<Tileset>, FixtureError> {
    let atlas = Atlas::builder()
        .name("automap-rules")
        .tile_size(32, 32)
        .grid(5, 1)
        .build_embedded()?;
    let mut tileset = Tileset::clone(&atlas.tileset);
    tileset.name = "AutoMap Rules".to_owned();
    for (id, kind) in (0u32..).zip(MATCH_TYPES) {
        let mut properties = Properties::new();
        properties.insert(
            "MatchType".to_owned(),
            Property::new(Value::String(kind.to_owned())),
        );
        tileset.tiles.insert(
            TileId(id),
            TileData {
                properties,
                ..TileData::default()
            },
        );
    }
    Ok(Arc::new(tileset))
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, TilesetSlot};
    use glam::IVec2;

    use super::*;
    use crate::fixtures::grid_cells;

    #[test]
    fn rule_tiles_say_what_they_mean_and_grids_name_them_by_letter() {
        let tiles = rule_tiles().unwrap();
        assert_eq!(tiles.tile_count(), 5);
        assert_eq!(
            tiles.tile(TileId(3)).unwrap().properties["MatchType"].value,
            Value::String("Empty".into())
        );
        let cells = grid_cells("X I N E O E@2").unwrap();
        let rule = |id: u32, slot: usize| Cell::new(TilesetSlot::new(slot).unwrap(), TileId(id));
        let found: Vec<Cell> = cells.iter().map(|c| c.cell).collect();
        assert_eq!(
            found,
            [
                rule(0, RULE_SLOT),
                rule(1, RULE_SLOT),
                rule(2, RULE_SLOT),
                rule(3, RULE_SLOT),
                rule(4, RULE_SLOT),
                rule(3, 2)
            ]
        );
        assert_eq!(cells[5].at, IVec2::new(5, 0));
    }
}
