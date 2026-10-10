//! The fixtures tests share. They build values, never files of a format: a test that needs a map
//! on disk writes it with the crate it tests. A drawn tileset's image is embedded, or written as
//! a PNG into a folder the test gives.

mod atlas;
mod error;
mod grid;
mod map;
mod pictures;
mod rules;
mod terrain;

pub use atlas::{Atlas, AtlasBuilder, GUTTER, MARKER};
pub use error::FixtureError;
pub use grid::{grid_cells, grid_layer};
pub use map::MapBuilder;
pub use pictures::{blank_picture, lane_of_pixel, paint_a2_autotile, paint_lanes};
pub use rules::{RULE_SLOT, rule_tiles};
pub use terrain::{TerrainSetBuilder, combination_tile, corners, edges, every_combination};
