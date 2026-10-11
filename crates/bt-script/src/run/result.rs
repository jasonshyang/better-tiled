//! What a script that ran gives back (`10-agents.md`, §8): the maps after it, the AutoMap
//! layers' setup after it, and the report. `bt` writes the shown map to the file and the
//! painted one to the project file; the editor applies them as one step.

use std::sync::Arc;

use bt_automap::layers::{AutoLayers, Setup};
use bt_core::Map;

use crate::edit_report::EditOutcome;

/// A script that ran to its end.
#[derive(Clone, Debug)]
pub struct EditResult {
    /// What was painted after the script: what the editor's document holds.
    pub painted: Arc<Map>,
    /// The map as it shows after the script, what its file gets: with AutoMap layers, what the
    /// rules make of what was painted; else what was painted.
    pub shown: Arc<Map>,
    /// The AutoMap layers' setup after the script; none when the map has none.
    pub setup: Option<Setup>,
    /// The AutoMap layers after the script (what was painted and the rules' result), from
    /// which the project file's record is made; none when the map has none.
    pub layers: Option<AutoLayers>,
    /// Whether the script started, changed or stopped the AutoMap layers.
    pub setup_changed: bool,
    /// The report.
    pub outcome: EditOutcome,
}
