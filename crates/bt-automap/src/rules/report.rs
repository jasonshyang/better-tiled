//! What a run did, typed: where the map changed, how often rules matched, what was added and
//! removed, which outputs locks kept out, and every problem.

use bt_core::{LayerId, ObjectId, TilesetSlot};
use bt_utils::IRect;

use crate::problem::Problem;

/// What a run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The smallest rectangle holding every cell that changed, on any layer; `None` when no
    /// cell changed.
    pub changed: Option<IRect>,
    /// How many matches were applied, in all.
    pub matches: usize,
    /// How many matches each rule map applied, by its index in the rule set; empty when the run
    /// did not start.
    pub rule_map_matches: Vec<usize>,
    /// What went wrong or was left out: outputs a lock kept out, bindings refused, a region run
    /// done on the whole map.
    pub problems: Vec<Problem>,
    /// Whether only the region asked for was recomputed (a full run says `false`).
    pub incremental: bool,
    /// The layers created for outputs the map lacked, in the order they were added on top.
    pub created_layers: Vec<LayerId>,
    /// The tilesets added to the map because rules placed their tiles, by their new slots.
    pub tilesets_added: Vec<TilesetSlot>,
    /// The objects rules placed, by their new ids.
    pub objects_added: Vec<ObjectId>,
    /// The objects `DeleteTiles` removed.
    pub objects_removed: Vec<ObjectId>,
    /// The outputs a lock kept out of a layer.
    pub locked_outputs: Vec<LockedOutput>,
}

/// Outputs a lock kept out of a layer: the layer, the rule map, and how many matches it
/// skipped there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedOutput {
    /// The rule map, by its index in the rule set.
    pub rule_map: usize,
    /// The locked layer.
    pub layer: LayerId,
    /// The layer's name.
    pub name: String,
    /// How many matches could not write to it.
    pub matches: usize,
}
