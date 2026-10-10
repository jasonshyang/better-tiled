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

mod adjust_tiles;
mod axis;
mod brush;
mod clip;
mod duplicate_name;
mod error;
mod ids;
mod lattice;
mod layer_ops;
mod map_ops;
mod object_clip;
mod objects;
mod region;
mod report;
mod select;
mod shapes;
#[cfg(test)]
mod testutil;
mod tile_layers;
mod tileset_ops;

pub use adjust_tiles::{adjust_tile_indexes, adjust_tile_metadata, columns_changed};
pub use axis::Axis;
pub use brush::{Brush, BrushMode, Variation};
pub use clip::{
    ClipLayer, PasteTarget, TileClip, TileCut, copy, cut, erase, erase_targets, find_target_layers,
    paste,
};
pub use error::EditError;
pub use ids::{IdRemap, NewIds};
pub use lattice::{FlipDirection, RotateDirection};
pub use layer_ops::{
    LayerLocation, LayerType, LayerViaSource, can_merge_down, children, duplicate_layers,
    group_layers, layer_order, layer_via_copy, layer_via_cut, locate, lower_layers,
    merge_layers_down, move_layers, new_layer, offset_layers, other_layers, raise_layers,
    remove_layers, toggle_locked, toggle_visible, ungroup_layers,
};
pub use map_ops::{
    autocrop, autocrop_to_layer, crop_to, make_finite, make_infinite, offset_map, resize_map,
};
pub use object_clip::{
    ObjectClip, copy_objects, copy_objects_in, cut_objects, cut_objects_in, paste_objects,
    paste_objects_in, paste_offset,
};
pub use objects::align::{
    Align, align_deltas, align_objects, align_objects_in, distribute_deltas, distribute_objects,
    distribute_objects_in,
};
pub use objects::bounds::{object_bounds, objects_bounds, objects_bounds_in};
pub use objects::convert::{
    rect_to_polygon, rect_to_polygon_in, reset_tile_object_size, reset_tile_object_size_in,
};
pub use objects::edit::{
    add_object, add_object_in, duplicate_objects, duplicate_objects_in, move_objects,
    move_objects_in, move_objects_to_layer, remove_objects, remove_objects_in,
};
pub use objects::find::{Found, find_object, object_index_in, object_mut, object_mut_in};
pub use objects::hit::{RectMode, object_contains, objects_at, objects_in_quad};
pub use objects::order::{
    lower_objects, lower_objects_in, lower_objects_to_bottom, lower_objects_to_bottom_in,
    move_objects_down, move_objects_down_in, move_objects_up, move_objects_up_in, raise_objects,
    raise_objects_in, raise_objects_to_top, raise_objects_to_top_in, render_order,
};
pub use objects::snap::{SnapMode, move_anchor, move_anchor_in, snap, snap_delta};
pub use objects::space::ObjectSpace;
pub use objects::transform::{
    Pivot, flip_objects, flip_objects_in, flip_origin, flip_origin_in, rotate_objects,
    rotate_objects_in,
};
pub use region::{SelectionMode, TileRegion};
pub use report::{
    AdjustReport, EraseReport, LayerChange, LayersReport, ObjectsReport, PasteReport,
    RemovedTileset, ResizeReport,
};
pub use select::{fill_area, flood_fill, invert, magic_wand, select_same_tile};
pub use shapes::{FillShape, rect_select_rect, shape_fill_region};
pub use tileset_ops::{
    TilesetUsage, Unified, add_tileset, find_tileset, remove_tileset, replace_tileset, swap_tiles,
    tileset_usage, unify_tileset,
};
