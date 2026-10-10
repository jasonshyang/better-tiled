//! Layer commands: where layers are, new and duplicated layers, removing, dragging, raising,
//! lowering, grouping and ungrouping them, Merge Layer Down, Layer via Copy and Cut,
//! visibility and locks, and the Offset Layers tool. Layers are named by id; a place in the
//! tree is a group (`None` for the map's top level) and an index among its layers, bottom
//! first. Each module names the Tiled sources it follows.

mod arrange;
mod create;
mod flags;
#[cfg(test)]
mod fuzz;
mod merge;
mod offset;
pub(crate) mod tree;
mod via;

pub use arrange::{
    group_layers, lower_layers, move_layers, raise_layers, remove_layers, ungroup_layers,
};
pub use create::{LayerType, duplicate_layers, new_layer};
pub use flags::{other_layers, toggle_locked, toggle_visible};
pub use merge::{can_merge_down, merge_layers_down};
pub use offset::offset_layers;
pub use tree::{LayerLocation, children, layer_order, locate};
pub use via::{LayerViaSource, layer_via_copy, layer_via_cut};
