//! Region runs (§7): an edit changes the result only near it, so a region run recomputes, inside
//! windows around the edit, exactly what a full run gives there, and changes nothing outside.
//! The locality analysis (`locality`, with the condition algebra of `condition`) gives each rule
//! map a dependency box; `plan` turns an edit into the area written back and each rule map's
//! window, or into the reason the run must be a full run. The engine runs the plan.

mod condition;
mod locality;
mod plan;

pub(crate) use locality::{Locality, RuleReach};
pub(crate) use plan::{RegionPlan, plan_region};
