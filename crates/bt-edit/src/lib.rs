//! The logic behind better-tiled's editing commands, as functions over `bt_core::Map`.
//!
//! An operation takes the map it changes (`&mut Map`) and what it needs (a region, a brush, ids,
//! a delta), edits in place and returns a report of what changed; when nothing would change it
//! leaves the map untouched and returns an empty report. A target that does not exist is an
//! [`EditError`], never a silent no-op. There is no undo here: callers snapshot the map first.
//! No operation knows about keys, widgets, files or pixels: callers pass modes
//! ([`SelectionMode::Add`]), and every choice that is random comes from a seed they give, so a
//! preview is what gets painted. Everything that depends on the grid goes through
//! `bt_core::MapGeometry` and works on every orientation.
//!
//! Most of the crate follows the behaviour of Tiled's editor (`reference/tiled/src/tiled`); each
//! module names its sources and `docs/design/04-editing-operations.md` lists the differences.

mod clip;
mod error;
mod ids;
mod lattice;
mod region;
mod report;
mod select;
mod shapes;
#[cfg(test)]
mod testutil;
mod tileset_ops;

pub use clip::{
    ClipLayer, PasteTarget, TileClip, TileCut, copy, cut, erase, erase_targets, find_target_layers,
    paste,
};
pub use error::EditError;
pub use ids::IdRemap;
pub use lattice::{FlipDirection, RotateDirection};
pub use region::{SelectionMode, TileRegion};
pub use report::{
    EraseReport, LayerChange, ObjectsReport, PasteReport, RemovedTileset, ResizeReport,
};
pub use select::{fill_area, flood_fill, invert, magic_wand, select_same_tile};
pub use shapes::{FillShape, rect_select_rect, shape_fill_region};
pub use tileset_ops::{
    TilesetUsage, Unified, add_tileset, find_tileset, remove_tileset, replace_tileset, swap_tiles,
    tileset_usage, unify_tileset,
};
