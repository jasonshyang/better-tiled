//! Explanations (§8): what the rules do at a cell and where a rule matches, recorded by a full
//! run through the engine's observer without changing the map (an untraced run pays nothing for
//! it), and the rules as written, summarised and drawn, for the AutoMap panel.
//!
//! Checks see the cells as Tiled checks them: as they were before the rule map ran in the
//! default mode, as they are when the rule's turn comes with `MatchInOrder`. Whether an
//! explanation is of "the run that just happened" or "a run now" is the caller's choice: the
//! crate explains the map it is given.

mod attempt;
mod cell;
mod picture;
mod rename;
mod rule;
mod summary;
mod tracer;

pub use attempt::{AttemptOutcome, ConditionCheck, RuleAttempt};
pub use cell::{CellWrite, Explanation, LayerAtCell, UnnamedTile};
pub use picture::{PictureLayer, PictureRole, RulePicture};
pub use rule::RuleMatches;
pub use summary::RuleSummary;
