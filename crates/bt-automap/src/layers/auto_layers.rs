//! The result of AutoMap layers: the painted map run through each run of the setup in turn,
//! every stage kept, so a run can be explained on the map it read.

use std::sync::Arc;

use bt_core::Map;

use super::object_ids::keep_object_ids;
use super::setup::Setup;
use crate::engine::RunRequest;
use crate::problem::Problem;
use crate::rules::{Report, RuleSet};

/// The stages of AutoMap layers: the painted map, then the map after each run of the setup over
/// the stage before; the last stage is the result, which is shown and saved.
#[derive(Clone, Debug)]
pub struct AutoLayers {
    painted: Arc<Map>,
    /// The map after each run, in order.
    after: Vec<Arc<Map>>,
    /// Each run's report, in order.
    reports: Vec<Report>,
}

impl AutoLayers {
    /// The painted map run through every run of `setup` with `rules`, each over the whole of
    /// the stage before it. A run whose bindings are refused (a bound layer gone or of the wrong
    /// kind, a layer bound twice) changes nothing and says so in its report.
    pub fn derive(painted: Arc<Map>, setup: &Setup, rules: &RuleSet) -> AutoLayers {
        let mut after: Vec<Arc<Map>> = Vec::with_capacity(setup.runs.len());
        let mut reports = Vec::with_capacity(setup.runs.len());
        for (run, names) in setup.runs.iter().enumerate() {
            let mut map = Map::clone(after.last().unwrap_or(&painted));
            let request = RunRequest::builder(setup.run_seed(run))
                .layer_names(names.clone())
                .build();
            reports.push(rules.run(&mut map, &request));
            after.push(Arc::new(map));
        }
        AutoLayers {
            painted,
            after,
            reports,
        }
    }

    /// The result again after a change, from `previous` when there is one: the objects the
    /// rules place keep the ids they had there when they are the same objects.
    pub fn update(
        previous: Option<&AutoLayers>,
        painted: Arc<Map>,
        setup: &Setup,
        rules: &RuleSet,
    ) -> AutoLayers {
        let mut layers = AutoLayers::derive(painted, setup, rules);
        if let (Some(previous), Some(result)) = (previous, layers.after.last_mut()) {
            keep_object_ids(
                Arc::make_mut(result),
                &layers.painted,
                previous.result(),
                previous.painted(),
            );
        }
        layers
    }

    /// The painted map with no run made: its result is what was painted (rules that could not
    /// be read run nothing).
    pub fn painted_only(painted: Arc<Map>) -> AutoLayers {
        AutoLayers {
            painted,
            after: Vec::new(),
            reports: Vec::new(),
        }
    }

    /// The painted map, the first stage.
    pub fn painted(&self) -> &Arc<Map> {
        &self.painted
    }

    /// The result: the last stage.
    pub fn result(&self) -> &Arc<Map> {
        self.after.last().unwrap_or(&self.painted)
    }

    /// The map run `run` read: the painted map for the first, the stage before it for the
    /// others.
    pub fn input_of(&self, run: usize) -> Option<&Arc<Map>> {
        if run >= self.after.len() {
            return None;
        }
        match run.checked_sub(1) {
            None => Some(&self.painted),
            Some(before) => self.after.get(before),
        }
    }

    /// Each run's report, in order.
    pub fn reports(&self) -> &[Report] {
        &self.reports
    }

    /// The problems of every run, in order: refused bindings, outputs locks kept out.
    pub fn problems(&self) -> impl Iterator<Item = &Problem> {
        self.reports.iter().flat_map(|report| &report.problems)
    }
}
