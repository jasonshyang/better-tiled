//! AutoMap layers: rules that run without destroying what was painted (`08-automap.md`, §10).
//! The painted map is the document's and the result is derived from it: stage 0 is the painted
//! map, stage `i + 1` is stage `i` after run `i` of the setup, and the last stage is what is
//! shown and saved. Each run binds the names the rules use to the map's layers; a name it leaves
//! unbound is a temporary layer, so chained rule maps need no map layer in between. The
//! project-file record keeps what was painted on the layers the rules write, where it differs
//! from the map as saved, so the painted map comes back when the map is opened again.

mod auto_layers;
mod change;
mod object_ids;
mod record;
mod rule_layers;
mod setup;

pub use auto_layers::AutoLayers;
pub use change::MapChange;
pub use record::{Restored, painted_record, read_from_project, restore};
pub use rule_layers::RuleLayers;
pub use setup::Setup;
