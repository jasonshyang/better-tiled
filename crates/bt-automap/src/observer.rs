//! What a run reports as it goes, for tracing: every attempt (a rule at a position, with its
//! outcome), every condition checked and every cell written. The engine is generic over an
//! [`Observer`]; [`NoObserver`] reports nothing, and with it the engine builds nothing to report,
//! so a plain run pays nothing for tracing.
//!
//! Cells are in the run's numbering of tilesets: the map's slots, followed by the slots of the
//! tilesets the run adds to the map, in the order they would be added. Rule cells are in the rule
//! set's numbering ([`RuleSet::tileset`](crate::RuleSet::tileset)).

use bt_core::{Cell, LayerId};
use glam::IVec2;

use crate::rule_ref::RuleRef;
use crate::rulemap::MatchCell;

/// Watches a run.
pub trait Observer {
    /// Whether the observer wants reports at all. When `false` the engine skips building them,
    /// and checks stop at the first that fails.
    const ACTIVE: bool = true;

    /// Whether `rule` is tried at every position of its lattice, not only where its anchor tile
    /// is (positions without it never match, so the result is the same; their checks are
    /// reported).
    fn tries_every_position(&self, rule: RuleRef) -> bool {
        let _ = rule;
        false
    }

    /// A rule was tried at a position.
    fn attempt(&mut self, attempt: &Attempt<'_>) {
        let _ = attempt;
    }

    /// A condition was checked.
    fn check(&mut self, check: &Check<'_>) {
        let _ = check;
    }

    /// A cell was written.
    fn write(&mut self, write: &Write<'_>) {
        let _ = write;
    }
}

/// The observer of a plain run: it wants nothing.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct NoObserver;

impl Observer for NoObserver {
    const ACTIVE: bool = false;
}

/// A rule tried at a position.
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt<'a> {
    /// The rule.
    pub rule: RuleRef,
    /// Where its input box's top-left was put.
    pub position: IVec2,
    /// What came of it.
    pub outcome: Outcome<'a>,
}

/// What came of trying a rule at a position.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<'a> {
    /// The rule matched and was applied.
    Applied {
        /// The random output chosen, by its index name; `None` when the rule has none.
        alternative: Option<&'a str>,
        /// The layers a lock kept the output out of.
        locked: Vec<LayerId>,
    },
    /// The rule's `Probability` skipped it here.
    SkippedByProbability,
    /// The rule matched but its output would overlap its own earlier output
    /// (`NoOverlappingOutput`).
    Overlapping,
    /// No input set holds here.
    NoMatch,
    /// The rule's input box does not fit inside the map here (`MatchOutsideMap` is off).
    OutsideMap,
    /// The position is off the rule's `ModX`/`ModY` lattice.
    OffLattice,
    /// The rule is `Disabled`.
    Disabled,
    /// The rule can never match (see its rule map's notes).
    CanNeverMatch,
}

/// Whether a condition asks for one of its tiles or for none of them.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Condition {
    /// The cell must be one of the tiles.
    AnyOf,
    /// The cell must be none of the tiles.
    NoneOf,
}

/// A condition checked at a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Check<'a> {
    /// The rule.
    pub rule: RuleRef,
    /// Where its input box's top-left was put.
    pub position: IVec2,
    /// The input set, by its index in the rule.
    pub input_set: usize,
    /// The layer read, by the name the rule uses.
    pub layer: &'a str,
    /// Whether the map has a layer for the name (else every cell reads as empty).
    pub layer_present: bool,
    /// The cell read: the condition's cell, wrapped or clamped at a finite map's border.
    pub at: IVec2,
    /// Whether the cell must be one of the tiles or none of them.
    pub condition: Condition,
    /// The tiles, in the rule set's numbering.
    pub tiles: &'a [MatchCell],
    /// What the cell holds when it is checked.
    pub found: Cell,
    /// What the cell held before the run.
    pub before: Cell,
    /// Whether the condition holds.
    pub holds: bool,
}

/// What wrote a cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Writer {
    /// A rule applied at a position.
    Rule {
        /// The rule.
        rule: RuleRef,
        /// Where its input box's top-left was put.
        position: IVec2,
    },
    /// A rule map's `DeleteTiles`, before its rules ran.
    DeleteTiles {
        /// The rule map, by its index in the rule set.
        rule_map: usize,
    },
}

/// A cell written.
#[derive(Clone, Debug, PartialEq)]
pub struct Write<'a> {
    /// The layer written, by the name the rules use.
    pub layer: &'a str,
    /// The cell.
    pub at: IVec2,
    /// What it held.
    pub before: Cell,
    /// What it holds now.
    pub after: Cell,
    /// What wrote it.
    pub by: Writer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plain_observer_wants_nothing() {
        const { assert!(!NoObserver::ACTIVE) };
        let mut observer = NoObserver;
        observer.attempt(&Attempt {
            rule: RuleRef::default(),
            position: IVec2::ZERO,
            outcome: Outcome::NoMatch,
        });
        assert!(!observer.tries_every_position(RuleRef::default()));
    }
}
