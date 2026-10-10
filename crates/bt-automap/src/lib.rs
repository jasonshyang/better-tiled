//! Tiled's automapping for better-tiled: rule maps compiled once and run on maps in memory with
//! the results Tiled 1.12 gives, randomness drawn from a seed, every attempt, check and write
//! reported to an observer the engine is generic over, and every problem typed.
//!
//! The crate ports Tiled's `AutoMapper` (`src/tiled/automapper.cpp`), `AutomappingManager`
//! (`src/tiled/automappingmanager.cpp`, with `src/tiled/automapperwrapper.cpp`) and
//! `automappingutils.cpp`, Copyright 2010-2026 Stefan Beller, Seanba, Thorbjørn Lindeijer and
//! Tiled's contributors, licensed under the GNU GPL version 2 or later and used here under
//! version 3 or later. `docs/design/08-automap.md` is the design; its "As built" section lists
//! every way the crate departs from Tiled and why.
//!
//! Compiling and running work on maps in memory ([`RuleSet::builder`], [`RuleSet::run`]); the
//! [`files`] module is the only one that reads files.

mod analysis;
mod bx;
mod cell_set;
mod engine;
mod explain;
pub mod files;
pub mod layers;
mod object_cells;
mod observer;
mod problem;
mod region;
mod rng;
mod rule_ref;
mod rulemap;
mod rules;

pub use analysis::{MissingLayer, TilesetCopy};
pub use engine::{
    Area, LayerNames, LayerNamesBuilder, NameBinding, RunRequest, RunRequestBuilder, Unbound,
};
pub use explain::{
    AttemptOutcome, CellWrite, ConditionCheck, Explanation, LayerAtCell, PictureLayer, PictureRole,
    RuleAttempt, RuleMatches, RulePicture, RuleSummary, UnnamedTile,
};
pub use files::Error;
pub use observer::{Attempt, Check, Condition, NoObserver, Observer, Outcome, Write, Writer};
pub use problem::{Problem, ProblemKind, Severity};
pub use rule_ref::RuleRef;
pub use rulemap::{MatchCell, RuleMap, TableTile};
pub use rules::{LockedOutput, Report, RuleSet, RuleSetBuilder};
