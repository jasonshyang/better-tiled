//! Cached, typed diagnostics. File probes and full model validation run on workers.

mod automap;
mod collect;
mod model;
mod navigation;
mod repair;
mod resources;
mod state;

pub use model::{
    Issue, IssueKind, IssueSource, IssueTarget, LocateTarget, ResourceKind, deduplicate_issues,
};
pub(crate) use state::Issues;
pub use state::{IssueJob, IssuesLoaded};
