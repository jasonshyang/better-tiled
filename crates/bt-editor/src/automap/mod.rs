//! AutoMap in the editor: the rules each map runs, read on a worker and read again when their
//! files change (a failed read keeps the rules there were), and Map ▸ AutoMap, which runs them
//! over the selection or the whole map as one step and says what they did, or why nothing.

mod command;
mod explaining;
mod job;
mod live;
mod reading;
mod rules;
mod saving;

pub use explaining::{CellExplanation, ExplainedRun};
pub use job::{KnownTileset, RulesJob, RulesLoaded};
pub use live::LiveEdit;
pub(crate) use rules::AutoMapState;
pub use rules::{AutoMapRules, LastRun};
pub(crate) use saving::read_layers;
