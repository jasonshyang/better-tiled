//! The result of AutoMap layers: the painted map run through each run of the setup in turn,
//! every stage kept, so a run can be explained on the map it read.

use std::sync::Arc;

use bt_core::{LayerId, Map};
use bt_utils::IRect;

use super::change::MapChange;
use super::object_ids::keep_object_ids;
use super::setup::Setup;
use crate::engine::{Area, RunRequest};
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

    /// The result again after what was painted changed, with the setup and rules `self` was
    /// made with (the caller's to know): the same when nothing the rules see changed; when
    /// only cells changed, each run recomputes just what the change can reach, with region
    /// runs that give there exactly what a full run gives (§7), and keeps the rest of its
    /// stage; else, or when a run cannot work around the change, full runs that keep placed
    /// objects' ids.
    pub fn after_painting(&self, painted: Arc<Map>, setup: &Setup, rules: &RuleSet) -> AutoLayers {
        match MapChange::between(&self.painted, &painted) {
            MapChange::Nothing => AutoLayers {
                painted,
                after: self.after.clone(),
                reports: self.reports.clone(),
            },
            MapChange::Cells(changed) => self
                .around(Arc::clone(&painted), setup, rules, changed)
                .unwrap_or_else(|| AutoLayers::update(Some(self), painted, setup, rules)),
            MapChange::Everything => AutoLayers::update(Some(self), painted, setup, rules),
        }
    }

    /// Each run recomputed around `changed`, the cells of what was painted that changed: a
    /// region run on the new stage before it, whose result inside the area it may change goes
    /// over the run's old stage. `None` when a run cannot work so (object outputs, a region run
    /// that falls back to a full run, layers that differ).
    fn around(
        &self,
        painted: Arc<Map>,
        setup: &Setup,
        rules: &RuleSet,
        changed: IRect,
    ) -> Option<AutoLayers> {
        let places_objects = rules
            .rule_maps()
            .iter()
            .any(|rule_map| !rule_map.object_outputs().is_empty());
        if places_objects || self.after.len() != setup.runs.len() {
            return None;
        }
        let mut after: Vec<Arc<Map>> = Vec::with_capacity(setup.runs.len());
        let mut reports = Vec::with_capacity(setup.runs.len());
        let mut region = changed;
        for (run, names) in setup.runs.iter().enumerate() {
            let input = Arc::clone(after.last().unwrap_or(&painted));
            let old = self.after.get(run)?;
            let affected = rules.affected_area(&input, region)?.union(region);
            let mut window = Map::clone(&input);
            let request = RunRequest::builder(setup.run_seed(run))
                .area(Area::Edit(region))
                .layer_names(names.clone())
                .build();
            let report = rules.run(&mut window, &request);
            if !report.incremental {
                return None;
            }
            let mut stage = Map::clone(old);
            copy_cells(&window, &mut stage, affected)?;
            reports.push(report);
            after.push(Arc::new(stage));
            region = affected;
        }
        Some(AutoLayers {
            painted,
            after,
            reports,
        })
    }

    /// AutoMap layers whose result is known without running them: `result` is what the rules
    /// made of `painted`, as the map's file holds it or the editor shows it. It stands for the
    /// layers before a change: [`AutoLayers::update`] from it keeps the ids of the objects the
    /// rules placed in `result`. The stages before the result and the runs' reports are not
    /// known, so it has the result as its one stage and no reports.
    pub fn known(painted: Arc<Map>, result: Arc<Map>) -> AutoLayers {
        AutoLayers {
            painted,
            after: vec![result],
            reports: Vec::new(),
        }
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

/// Copies the cells of every tile layer of `from` inside `area` into the same layer of `into`;
/// `None` when the two maps' layers differ.
fn copy_cells(from: &Map, into: &mut Map, area: IRect) -> Option<()> {
    if layer_ids(from) != layer_ids(into) {
        return None;
    }
    let mut copied = true;
    into.for_each_layer_mut(|layer| {
        let source = from.layer(layer.id).and_then(|l| l.as_tile_layer());
        let (Some(target), Some(source)) = (layer.as_tile_layer_mut(), source) else {
            return;
        };
        for p in area.points() {
            if target.contains(p) && target.set_cell(p, source.cell(p)).is_err() {
                copied = false;
            }
        }
    });
    copied.then_some(())
}

/// The ids of a map's layers, depth first.
fn layer_ids(map: &Map) -> Vec<LayerId> {
    let mut ids = Vec::new();
    map.for_each_layer(|layer, _| ids.push(layer.id));
    ids
}
