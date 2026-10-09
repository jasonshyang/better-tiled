//! Worlds (`.world` files): maps laid out next to each other.

use bt_utils::IRect;
use glam::IVec2;

use crate::file_ref::FileRef;
use crate::property::Properties;

/// A world: maps listed with their places, and patterns that find more by file name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    /// The maps listed.
    pub maps: Vec<WorldMap>,
    /// The patterns.
    pub patterns: Vec<WorldPattern>,
    /// Whether only the maps next to the current one are shown.
    pub only_show_adjacent_maps: bool,
    /// The grid maps snap to, in pixels (Tiled 1.13); zero for none.
    pub grid_size: IVec2,
    /// Custom properties (Tiled 1.12).
    pub properties: Properties,
}

/// A map placed in a world.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldMap {
    /// The map file, relative to the world.
    pub file: FileRef,
    /// Where the map is and how big, in pixels.
    pub rect: IRect,
}

/// Maps found by matching the names of the files in the world's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldPattern {
    /// The regular expression searched for in file names; its two capture groups are the map's
    /// column and row.
    pub regexp: String,
    /// Pixels per column and row (never 0).
    pub multiplier: IVec2,
    /// Added to every map's position.
    pub offset: IVec2,
    /// Each map's size in pixels.
    pub map_size: IVec2,
}

impl WorldPattern {
    /// A pattern with Tiled's defaults: multipliers of 1, no offset, maps of 1×1 pixels.
    pub fn new(regexp: impl Into<String>) -> WorldPattern {
        WorldPattern {
            regexp: regexp.into(),
            multiplier: IVec2::ONE,
            offset: IVec2::ZERO,
            map_size: IVec2::ONE,
        }
    }

    /// The rectangle of the map in `column` and `row`.
    pub fn map_rect(&self, column: i32, row: i32) -> IRect {
        IRect::from_min_size(
            IVec2::new(column, row) * self.multiplier + self.offset,
            self.map_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patterns_maps_go_on_its_grid() {
        let pattern = WorldPattern {
            multiplier: IVec2::new(6400, 3200),
            offset: IVec2::new(-6400, 10),
            map_size: IVec2::new(6400, 3200),
            ..WorldPattern::new(r"ow-p0*(\d+)-n0*(\d+)\.tmx")
        };
        assert_eq!(
            pattern.map_rect(2, 3),
            IRect {
                x: 6400,
                y: 9610,
                w: 6400,
                h: 3200
            }
        );
    }
}
