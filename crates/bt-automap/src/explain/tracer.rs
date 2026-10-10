//! The observer explanations run the rules with. It asks the run to try the rules it watches at
//! the positions it watches (besides where their anchor tiles are), or one rule at every
//! position, and keeps what the run reports about them: their attempts with the checks made
//! there, the writes to one cell or by one rule, and how often every rule applied.

use std::collections::HashMap;

use bt_core::Cell;
use glam::IVec2;

use crate::explain::attempt::{ConditionCheck, RuleAttempt};
use crate::observer::{Attempt, Check, Observer, Outcome, Write, Writer};
use crate::rule_ref::RuleRef;
use crate::rules::RuleSet;

/// Which attempts a tracer keeps.
pub(crate) enum Watch {
    /// Each rule at its positions (sorted), tried there besides where its anchors are.
    Positions(HashMap<RuleRef, Vec<IVec2>>),
    /// One rule, tried at every position of its lattice: its matches, and the misses whose
    /// closest input set fails at most `max_failures` checks.
    Rule {
        /// The rule.
        rule: RuleRef,
        /// The most failing checks a miss kept may have.
        max_failures: usize,
    },
}

/// A cell written, owned.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WriteRecord {
    /// The layer, by the name the rules use.
    pub(crate) layer: String,
    /// The cell.
    pub(crate) at: IVec2,
    /// What it held.
    pub(crate) before: Cell,
    /// What it holds now.
    pub(crate) after: Cell,
    /// What wrote it.
    pub(crate) by: Writer,
}

/// A rule at a position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct AttemptKey {
    rule: RuleRef,
    position: IVec2,
}

/// An attempt whose checks have come and whose outcome has not, with when it was tried.
struct OpenAttempt {
    order: usize,
    checks: Vec<ConditionCheck>,
}

/// An attempt kept, with when it was tried.
struct KeptAttempt {
    order: usize,
    attempt: RuleAttempt,
}

/// What a tracer kept.
pub(crate) struct Traced {
    /// The attempts kept, in the order the run tried them, rule by rule.
    pub(crate) attempts: Vec<RuleAttempt>,
    /// The writes kept, in order.
    pub(crate) writes: Vec<WriteRecord>,
    /// How often each rule applied, by rule map and rule.
    pub(crate) applied: Vec<Vec<usize>>,
}

/// Records what a run reports about the attempts it watches.
pub(crate) struct Tracer<'s> {
    set: &'s RuleSet,
    watch: Watch,
    /// The cell whose writes are kept.
    cell: Option<IVec2>,
    open: HashMap<AttemptKey, OpenAttempt>,
    next: usize,
    kept: Vec<KeptAttempt>,
    writes: Vec<WriteRecord>,
    applied: Vec<Vec<usize>>,
}

impl<'s> Tracer<'s> {
    /// A tracer of `set`'s run watching `watch`, keeping the writes to `cell` (and, watching a
    /// rule, the rule's own).
    pub(crate) fn new(set: &'s RuleSet, watch: Watch, cell: Option<IVec2>) -> Tracer<'s> {
        Tracer {
            set,
            watch,
            cell,
            open: HashMap::new(),
            next: 0,
            kept: Vec::new(),
            writes: Vec::new(),
            applied: set
                .rule_maps()
                .iter()
                .map(|r| vec![0; r.rule_count()])
                .collect(),
        }
    }

    fn watches(&self, rule: RuleRef, position: IVec2) -> bool {
        match &self.watch {
            Watch::Positions(positions) => positions.get(&rule).is_some_and(|list| {
                list.binary_search_by_key(&(position.y, position.x), |p| (p.y, p.x))
                    .is_ok()
            }),
            Watch::Rule { rule: watched, .. } => *watched == rule,
        }
    }

    /// Whether an input set of `rule` holds, as `checks` (all of them, at one position) say.
    fn holds(&self, rule: RuleRef, checks: &[ConditionCheck]) -> bool {
        let Some(compiled) = self
            .set
            .rule_maps()
            .get(rule.rule_map)
            .and_then(|r| r.rules.get(rule.rule))
        else {
            return false;
        };
        compiled.input_sets.iter().enumerate().any(|(index, set)| {
            if set.never.is_some() {
                return false;
            }
            // A set of Ignore tiles only holds everywhere, unchecked.
            if set.layers.is_empty() {
                return true;
            }
            let mut of_set = checks.iter().filter(|c| c.input_set == index).peekable();
            of_set.peek().is_some() && of_set.all(|c| c.holds)
        })
    }

    fn keeps(&self, attempt: &RuleAttempt) -> bool {
        match self.watch {
            Watch::Positions(_) => true,
            Watch::Rule { max_failures, .. } => {
                attempt.matched || attempt.failures().is_some_and(|f| f <= max_failures)
            }
        }
    }

    /// What the tracer kept, the attempts in the order the run tried them.
    pub(crate) fn finish(self) -> Traced {
        let mut kept = self.kept;
        kept.sort_by_key(|k| (k.attempt.rule.rule_map, k.attempt.rule.rule, k.order));
        Traced {
            attempts: kept.into_iter().map(|k| k.attempt).collect(),
            writes: self.writes,
            applied: self.applied,
        }
    }
}

impl Observer for Tracer<'_> {
    fn tries_every_position(&self, rule: RuleRef) -> bool {
        matches!(&self.watch, Watch::Rule { rule: watched, .. } if *watched == rule)
    }

    fn extra_positions(&self, rule: RuleRef) -> &[IVec2] {
        match &self.watch {
            Watch::Positions(positions) => positions.get(&rule).map_or(&[], Vec::as_slice),
            Watch::Rule { .. } => &[],
        }
    }

    fn check(&mut self, check: &Check<'_>) {
        if !self.watches(check.rule, check.position) {
            return;
        }
        let key = AttemptKey {
            rule: check.rule,
            position: check.position,
        };
        let next = &mut self.next;
        let open = self.open.entry(key).or_insert_with(|| {
            *next += 1;
            OpenAttempt {
                order: *next,
                checks: Vec::new(),
            }
        });
        open.checks.push(ConditionCheck::from(check));
    }

    fn attempt(&mut self, attempt: &Attempt<'_>) {
        if matches!(attempt.outcome, Outcome::Applied { .. })
            && let Some(count) = self
                .applied
                .get_mut(attempt.rule.rule_map)
                .and_then(|r| r.get_mut(attempt.rule.rule))
        {
            *count += 1;
        }
        if !self.watches(attempt.rule, attempt.position) {
            return;
        }
        let key = AttemptKey {
            rule: attempt.rule,
            position: attempt.position,
        };
        let open = match self.open.remove(&key) {
            Some(open) => open,
            None => {
                self.next += 1;
                OpenAttempt {
                    order: self.next,
                    checks: Vec::new(),
                }
            }
        };
        let matched = match attempt.outcome {
            Outcome::Applied { .. } | Outcome::Overlapping => true,
            Outcome::SkippedByProbability => self.holds(attempt.rule, &open.checks),
            _ => false,
        };
        let kept = RuleAttempt {
            rule: attempt.rule,
            position: attempt.position,
            outcome: (&attempt.outcome).into(),
            matched,
            checks: open.checks,
        };
        if self.keeps(&kept) {
            self.kept.push(KeptAttempt {
                order: open.order,
                attempt: kept,
            });
        }
    }

    fn write(&mut self, write: &Write<'_>) {
        let by_watched = match (&self.watch, write.by) {
            (Watch::Rule { rule, .. }, Writer::Rule { rule: by, .. }) => *rule == by,
            _ => false,
        };
        if self.cell == Some(write.at) || by_watched {
            self.writes.push(WriteRecord {
                layer: write.layer.to_owned(),
                at: write.at,
                before: write.before,
                after: write.after,
                by: write.by,
            });
        }
    }
}
