//! Where one rule matches (§8, `explain_rule`): a full run with the rule tried at every position
//! of its lattice, recorded without changing the map. Every position where it matched (applied
//! or not), the near misses whose closest input set fails at most a given number of checks, and
//! the applied matches whose every written cell holds something else once the run is over.
//! Ranking near misses by failures alone drowns in noise on a pack with two thousand rules, so
//! [`RuleAttempt::holds_a_tile`] says which came close for what is there.

use bt_core::Map;
use glam::IVec2;

use crate::engine::{self, LayerNames, RunRequest};
use crate::explain::attempt::RuleAttempt;
use crate::explain::cell::full_run;
use crate::explain::rename::LayerRenaming;
use crate::explain::tracer::{Tracer, Watch, WriteRecord};
use crate::observer::Writer;
use crate::problem::Problem;
use crate::rule_ref::RuleRef;
use crate::rules::{Report, RuleSet};

/// Where a rule matches ([`RuleSet::explain_rule`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleMatches {
    /// The rule.
    pub rule: RuleRef,
    /// Every position where it matched, in the order the run tried them: applied, overlapping
    /// its own output (`NoOverlappingOutput`), or skipped by its `Probability` where an input
    /// set held.
    pub matches: Vec<RuleAttempt>,
    /// The positions where it did not match but its closest input set fails at most the
    /// number of checks asked for, in the same order.
    pub near_misses: Vec<RuleAttempt>,
    /// The applied matches whose every written cell holds something else after the run, in
    /// the order they applied.
    pub overwritten: Vec<IVec2>,
    /// How often each rule applied in the whole run, by rule map and rule.
    pub applied: Vec<Vec<usize>>,
    /// Why the run did not start, when it did not (its layer names are refused).
    pub problems: Vec<Problem>,
}

impl RuleMatches {
    /// The matches in the map's words: every name `names` binds to a layer of `map` becomes
    /// that layer's name.
    pub fn with_layer_names(mut self, names: &LayerNames, map: &Map) -> RuleMatches {
        let renaming = LayerRenaming::new(names, map);
        for check in self
            .matches
            .iter_mut()
            .chain(&mut self.near_misses)
            .flat_map(|a| &mut a.checks)
        {
            renaming.rename(&mut check.layer);
        }
        self
    }
}

/// The positions, in the order they applied, whose every write the run later overwrote:
/// `holds_now` says what a layer holds at a cell after the run.
fn overwritten(
    writes: &[WriteRecord],
    holds_now: impl Fn(&str, IVec2) -> bt_core::Cell,
) -> Vec<IVec2> {
    let mut positions: Vec<IVec2> = Vec::new();
    for write in writes {
        if let Writer::Rule { position, .. } = write.by
            && !positions.contains(&position)
        {
            positions.push(position);
        }
    }
    positions.retain(|&position| {
        writes
            .iter()
            .filter(|w| matches!(w.by, Writer::Rule { position: p, .. } if p == position))
            .all(|w| holds_now(&w.layer, w.at) != w.after)
    });
    positions
}

impl RuleSet {
    /// Where `rule` matches when the rules run on `map` now with `seed` (a full run that leaves
    /// `map` as it is): its matches, its near misses failing at most `max_failures` checks (0
    /// for none) and its applied matches later overwritten.
    pub fn explain_rule(
        &self,
        map: &Map,
        rule: RuleRef,
        seed: u64,
        max_failures: usize,
    ) -> RuleMatches {
        self.explain_rule_with(map, rule, &RunRequest::builder(seed).build(), max_failures)
    }

    /// [`RuleSet::explain_rule`] for a run with `request`'s seed and layer names (its area is
    /// ignored: an explanation is of a full run).
    pub fn explain_rule_with(
        &self,
        map: &Map,
        rule: RuleRef,
        request: &RunRequest,
        max_failures: usize,
    ) -> RuleMatches {
        let request = full_run(request);
        let mut tracer = Tracer::new(self, Watch::Rule { rule, max_failures }, None);
        let mut report = Report::default();
        let result = engine::trace(self, map, &request, &mut tracer, &mut report);
        let traced = tracer.finish();
        let (matches, near_misses): (Vec<RuleAttempt>, Vec<RuleAttempt>) =
            traced.attempts.into_iter().partition(|a| a.matched);
        let overwritten = result.as_ref().map_or_else(Vec::new, |result| {
            overwritten(&traced.writes, |layer, at| result.cell(layer, at))
        });
        RuleMatches {
            rule,
            matches,
            near_misses,
            overwritten,
            applied: traced.applied,
            problems: report.problems,
        }
    }
}
