//! A rule tried at a position, as an explanation tells it: what came of it and every condition
//! checked there. These own their data, so an explanation outlives the run; the observer's
//! [`Attempt`](crate::Attempt) and [`Check`](crate::Check) borrow from the run instead.

use bt_core::{Cell, LayerId};
use glam::IVec2;

use crate::observer::{Check, Condition, Outcome};
use crate::rule_ref::RuleRef;
use crate::rulemap::MatchCell;

/// What came of trying a rule at a position: an [`Outcome`] that owns its data.
#[derive(Clone, Debug, PartialEq)]
pub enum AttemptOutcome {
    /// The rule matched and was applied.
    Applied {
        /// The random output chosen, by its index name; `None` when the rule has none.
        alternative: Option<String>,
        /// The layers a lock kept the output out of.
        locked: Vec<LayerId>,
    },
    /// The rule's `Probability` skipped it here; the checks say whether it would have matched.
    SkippedByProbability,
    /// The rule matched but its output would overlap its own earlier output
    /// (`NoOverlappingOutput`).
    Overlapping,
    /// No input set holds here.
    NoMatch,
    /// The run does not try the rule here: its input box does not fit inside the map
    /// (`MatchOutsideMap` is off) or does not reach the region the run covers.
    OutsideMap,
    /// The position is off the rule's `ModX`/`ModY` lattice.
    OffLattice,
    /// The rule is `Disabled`.
    Disabled,
    /// The rule can never match: it has no output, or every input set contradicts itself or
    /// has no condition (its rule map's notes say which).
    CanNeverMatch,
}

impl From<&Outcome<'_>> for AttemptOutcome {
    fn from(outcome: &Outcome<'_>) -> AttemptOutcome {
        match outcome {
            Outcome::Applied {
                alternative,
                locked,
            } => AttemptOutcome::Applied {
                alternative: alternative.map(str::to_owned),
                locked: locked.clone(),
            },
            Outcome::SkippedByProbability => AttemptOutcome::SkippedByProbability,
            Outcome::Overlapping => AttemptOutcome::Overlapping,
            Outcome::NoMatch => AttemptOutcome::NoMatch,
            Outcome::OutsideMap => AttemptOutcome::OutsideMap,
            Outcome::OffLattice => AttemptOutcome::OffLattice,
            Outcome::Disabled => AttemptOutcome::Disabled,
            Outcome::CanNeverMatch => AttemptOutcome::CanNeverMatch,
        }
    }
}

/// A condition checked at a cell, at the moment Tiled checks it: before the rule map ran in
/// the default mode, when the rule's turn comes with `MatchInOrder`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionCheck {
    /// The input set, by its index in the rule.
    pub input_set: usize,
    /// The layer read, by the name the rule uses.
    pub layer: String,
    /// Whether the map has a layer for the name (else every cell reads as empty).
    pub layer_present: bool,
    /// The cell read: the condition's cell, wrapped or clamped at a finite map's border.
    pub at: IVec2,
    /// Whether the cell must be one of the tiles or none of them.
    pub condition: Condition,
    /// The tiles, in the rule set's numbering ([`RuleSet::tileset`](crate::RuleSet::tileset)).
    pub tiles: Vec<MatchCell>,
    /// What the cell held when it was checked, in the run's numbering.
    pub found: Cell,
    /// What the cell held before the run, in the run's numbering.
    pub before: Cell,
    /// Whether the condition holds.
    pub holds: bool,
}

impl From<&Check<'_>> for ConditionCheck {
    fn from(check: &Check<'_>) -> ConditionCheck {
        ConditionCheck {
            input_set: check.input_set,
            layer: check.layer.to_owned(),
            layer_present: check.layer_present,
            at: check.at,
            condition: check.condition,
            tiles: check.tiles.to_vec(),
            found: check.found,
            before: check.before,
            holds: check.holds,
        }
    }
}

impl ConditionCheck {
    /// Whether it asks for particular tiles: "any of" tiles, not only the empty cell.
    pub fn names_tiles(&self) -> bool {
        self.condition == Condition::AnyOf && self.tiles.iter().any(|t| !t.is_empty())
    }
}

/// How one input set fared at an attempt.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct SetFailures {
    input_set: usize,
    failing: usize,
}

/// A rule tried (or not) at a position.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleAttempt {
    /// The rule.
    pub rule: RuleRef,
    /// Where its input box's top-left was put.
    pub position: IVec2,
    /// What came of it.
    pub outcome: AttemptOutcome,
    /// Whether one of its input sets holds here (where its `Probability` skipped it too, as
    /// the checks say); `false` where the rule is not tried.
    pub matched: bool,
    /// Every condition checked, input set by input set (a set made only of `Ignore` tiles has
    /// none and holds everywhere).
    pub checks: Vec<ConditionCheck>,
}

impl RuleAttempt {
    /// Each input set checked with how many of its checks fail, in the rule's order.
    fn failures_by_set(&self) -> Vec<SetFailures> {
        let mut sets: Vec<SetFailures> = Vec::new();
        for check in &self.checks {
            let failing = usize::from(!check.holds);
            match sets.iter_mut().find(|s| s.input_set == check.input_set) {
                Some(set) => set.failing += failing,
                None => sets.push(SetFailures {
                    input_set: check.input_set,
                    failing,
                }),
            }
        }
        sets
    }

    /// The input set closest to holding: the first with the fewest failing checks; `None` when
    /// nothing was checked.
    pub fn closest_set(&self) -> Option<usize> {
        self.failures_by_set()
            .into_iter()
            .min_by_key(|s| s.failing)
            .map(|s| s.input_set)
    }

    /// How many checks of the closest input set fail: 0 when it matched, `None` when nothing
    /// was checked and it did not match.
    pub fn failures(&self) -> Option<usize> {
        if self.matched {
            return Some(0);
        }
        self.failures_by_set().into_iter().map(|s| s.failing).min()
    }

    /// The "came close" signal: a condition of the closest input set that names particular
    /// tiles holds, so the rule is about what is here even if other conditions fail.
    pub fn holds_a_tile(&self) -> bool {
        let Some(closest) = self.closest_set() else {
            return false;
        };
        self.checks
            .iter()
            .any(|c| c.input_set == closest && c.holds && c.names_tiles())
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Flip, TileId};

    use super::*;
    use crate::rulemap::TableTile;

    fn check(
        input_set: usize,
        condition: Condition,
        tile: Option<u32>,
        holds: bool,
    ) -> ConditionCheck {
        ConditionCheck {
            input_set,
            layer: "set".into(),
            layer_present: true,
            at: IVec2::ZERO,
            condition,
            tiles: vec![MatchCell {
                tile: tile.map(|id| TableTile {
                    tileset: 0,
                    tile: TileId(id),
                    flip: Flip::empty(),
                }),
                mask: Flip::all(),
            }],
            found: Cell::EMPTY,
            before: Cell::EMPTY,
            holds,
        }
    }

    #[test]
    fn the_closest_input_set_says_how_near_an_attempt_came() {
        let attempt = RuleAttempt {
            rule: RuleRef::default(),
            position: IVec2::ZERO,
            outcome: AttemptOutcome::NoMatch,
            matched: false,
            checks: vec![
                check(0, Condition::AnyOf, Some(1), false),
                check(0, Condition::AnyOf, Some(2), false),
                check(1, Condition::AnyOf, Some(3), true),
                check(1, Condition::NoneOf, None, false),
            ],
        };
        assert_eq!(attempt.closest_set(), Some(1));
        assert_eq!(attempt.failures(), Some(1));
        assert!(attempt.holds_a_tile());
        // Only the empty cell asked for, or "none of": no particular tile.
        let empty_only = RuleAttempt {
            checks: vec![
                check(0, Condition::AnyOf, None, true),
                check(0, Condition::AnyOf, Some(4), false),
            ],
            ..attempt.clone()
        };
        assert!(!empty_only.holds_a_tile());
        let untried = RuleAttempt {
            outcome: AttemptOutcome::OffLattice,
            checks: Vec::new(),
            ..attempt
        };
        assert_eq!(untried.failures(), None);
        assert!(!untried.holds_a_tile());
    }

    #[test]
    fn outcomes_and_checks_are_owned_copies_of_the_observers() {
        let outcome = Outcome::Applied {
            alternative: Some("B"),
            locked: vec![LayerId(2)],
        };
        assert_eq!(
            AttemptOutcome::from(&outcome),
            AttemptOutcome::Applied {
                alternative: Some("B".into()),
                locked: vec![LayerId(2)]
            }
        );
        assert_eq!(
            AttemptOutcome::from(&Outcome::OffLattice),
            AttemptOutcome::OffLattice
        );
    }
}
