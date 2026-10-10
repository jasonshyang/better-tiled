//! Running a rule set on a map, as Tiled's `AutoMapperWrapper` runs its `AutoMapper`s: the rule
//! maps in order over the same layers, each seeing what the earlier ones wrote, then the cells
//! that changed written back, the layers created, the objects placed and the tilesets added.
//!
//! A rule map's run (`AutoMapper::autoMap`): `DeleteTiles` erases first; then either every rule's
//! matches are collected against the layers as they are and applied rule by rule (the default),
//! or, with `MatchInOrder`, each rule is matched and applied position by position, seeing its own
//! writes and those of the rules before it.
//!
//! A region run (§7) runs every rule map inside its window of the plan (`window`), and keeps
//! only what falls in the area the plan recomputes.

mod anchor;
mod apply;
mod bind;
mod delete;
mod grid;
mod matching;
mod objects;
mod request;
mod targets;
mod window;
mod writeback;

use std::collections::{BTreeSet, HashMap};

use bt_core::Map;
use bt_utils::IRect;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::observer::{Attempt, Observer, Outcome};
use crate::problem::Problem;
use crate::region::{RegionPlan, plan_region};
use crate::rng::{self, Purpose};
use crate::rule_ref::RuleRef;
use crate::rulemap::{Rule, RuleMap};
use crate::rules::{LockedOutput, Report, RuleSet};

use anchor::{
    Anchor, AnchorIndex, MatchArea, ScanKey, anchor_candidates, anchored_positions, choose_anchors,
};
use apply::{OverlapState, Pending};
pub(crate) use bind::{TilesetBinding, find_layer, is_tiles};
use matching::{Border, BoundSet, Reader};
use objects::PlacedGroup;
pub(crate) use objects::rewire;
use targets::{Targets, refused_bindings};
use window::RuleMapWindow;

pub use request::{
    Area, LayerNames, LayerNamesBuilder, NameBinding, RunRequest, RunRequestBuilder, Unbound,
};

/// An object `DeleteTiles` removes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemovedObject {
    pub(crate) layer: bt_core::LayerId,
    pub(crate) object: bt_core::ObjectId,
}

/// The state of a run.
pub(crate) struct Run<'a, O: Observer> {
    pub(crate) set: &'a RuleSet,
    pub(crate) map: &'a Map,
    pub(crate) names: &'a LayerNames,
    pub(crate) seed: u64,
    pub(crate) observer: &'a mut O,
    pub(crate) tilesets: TilesetBinding,
    pub(crate) targets: Targets,
    pub(crate) placed: Vec<PlacedGroup>,
    pub(crate) removed: Vec<RemovedObject>,
    pub(crate) locked: Vec<LockedOutput>,
    pub(crate) matches: Vec<usize>,
    pub(crate) problems: Vec<Problem>,
    /// The plan of a region run; `None` for every other run.
    pub(crate) plan: Option<RegionPlan>,
}

/// A rule map bound for a run: its layers, its border and where its rules may be tried.
pub(crate) struct RuleMapRun<'r> {
    pub(crate) index: usize,
    pub(crate) rule_map: &'r RuleMap,
    pub(crate) border: Border,
    /// Whether written cells wrap around a finite map (`WrapBorder`).
    pub(crate) wrap: bool,
    /// The map's size, when it is finite.
    pub(crate) map_size: Option<IVec2>,
    /// Whether rules may match where their input box leaves a finite map.
    pub(crate) match_outside_map: bool,
    /// The grid read for each input name; `None` when the map has no such layer.
    pub(crate) inputs: Vec<Option<usize>>,
    /// The grid written for each tile output name.
    pub(crate) tile_outputs: Vec<usize>,
    /// The object target for each object output name.
    pub(crate) object_outputs: Vec<usize>,
    /// The cells the rules are run around (the region, grown by `AutomappingRadius`); in a
    /// region run, those of the window.
    pub(crate) region: CellSet,
    /// The window of a region run; `None` for every other run.
    pub(crate) window: Option<RuleMapWindow>,
}

impl RuleMapRun<'_> {
    /// On a finite map, the positions where a rule's input box fits inside the map: from (0, 0)
    /// to the map's size less the box's (Tiled's `QRect(0, 0, width - ruleWidth, height -
    /// ruleHeight)`).
    pub(crate) fn inside(&self, input_box: Bx) -> Option<IRect> {
        let size = self.map_size?;
        let reach = input_box.size() - IVec2::ONE;
        Some(IRect {
            x: 0,
            y: 0,
            w: size.x - reach.x,
            h: size.y - reach.y,
        })
    }

    /// Where anchors say nothing because the border rereads cells: the positions where a rule's
    /// box fits are covered by anchors, the others must be tried (`OverflowBorder`,
    /// `WrapBorder`).
    pub(crate) fn rereads_border(&self, input_box: Bx) -> Option<IRect> {
        match self.border {
            Border::Empty => None,
            Border::Overflow | Border::Wrap => self.inside(input_box),
        }
    }
}

/// A rule bound for a run.
pub(crate) struct BoundRule<'r> {
    pub(crate) index: usize,
    pub(crate) rule: &'r Rule,
    pub(crate) sets: Vec<BoundSet<'r>>,
}

impl<'a, O: Observer> Run<'a, O> {
    fn new(
        set: &'a RuleSet,
        map: &'a Map,
        request: &'a RunRequest,
        observer: &'a mut O,
        area: IRect,
        plan: Option<RegionPlan>,
    ) -> Run<'a, O> {
        let (tilesets, problems) = TilesetBinding::new(&set.table, map);
        let mut run = Run {
            set,
            map,
            names: &request.layer_names,
            seed: request.seed,
            observer,
            tilesets,
            targets: Targets::new(area),
            placed: Vec::new(),
            removed: Vec::new(),
            locked: Vec::new(),
            matches: vec![0; set.rule_maps().len()],
            problems,
            plan,
        };
        // Output layers are bound, and those the map lacks created, in rule map order before
        // anything runs, as Tiled prepares every rule map first.
        for rule_map in set.rule_maps().iter().filter(|r| r.is_used()) {
            for output in &rule_map.output_layers {
                if output.objects {
                    run.targets.object_output(map, run.names, &output.name);
                } else {
                    run.targets.tile_output(map, run.names, &output.name);
                }
            }
        }
        run
    }

    /// Binds rule map `index` for a run around `region`.
    fn bind_rule_map(&mut self, index: usize, region: &CellSet) -> Option<RuleMapRun<'a>> {
        let rule_map = self.set.rule_maps().get(index)?;
        let options = &rule_map.options;
        let finite = !self.map.infinite;
        let border = if !finite {
            Border::Empty
        } else if options.wrap_border {
            Border::Wrap
        } else if options.overflow_border {
            Border::Overflow
        } else {
            Border::Empty
        };
        let map_size = finite.then(|| {
            IVec2::new(
                i32::try_from(self.map.width).unwrap_or(i32::MAX),
                i32::try_from(self.map.height).unwrap_or(i32::MAX),
            )
        });
        let radius = options.radius;
        let grow = |rect: IRect| IRect {
            x: rect.x - radius,
            y: rect.y - radius,
            w: rect.w + 2 * radius,
            h: rect.h + 2 * radius,
        };
        let (region, window) = match &self.plan {
            None if radius == 0 => (region.clone(), None),
            None => {
                let mut grown = CellSet::new();
                for rect in region.rects() {
                    grown.insert_rect(grow(rect));
                }
                (grown, None)
            }
            Some(plan) => {
                // A full run's region on a finite map is the map; on an infinite one the rules
                // a region run allows are tried wherever they can match.
                let cells = plan.window(index)?;
                let full_region = match map_size {
                    Some(size) => Some(Bx::from_rect(grow(IRect::from_min_size(
                        IVec2::ZERO,
                        size,
                    )))?),
                    None => None,
                };
                let erased = match full_region {
                    Some(full) => cells.intersection(full),
                    None => Some(cells),
                };
                let window = RuleMapWindow { cells, full_region };
                let region = erased.map_or_else(CellSet::new, |b| CellSet::from_rect(b.rect()));
                (region, Some(window))
            }
        };
        let tile_outputs = rule_map
            .tile_outputs
            .iter()
            .map(|name| self.targets.tile_output(self.map, self.names, name))
            .collect();
        let object_outputs = rule_map
            .object_outputs
            .iter()
            .map(|name| self.targets.object_output(self.map, self.names, name))
            .collect();
        let inputs = rule_map
            .input_names
            .iter()
            .map(|name| self.targets.tile_input(self.map, self.names, name))
            .collect();
        Some(RuleMapRun {
            index,
            rule_map,
            border,
            wrap: finite && options.wrap_border,
            map_size,
            match_outside_map: options.match_outside_map,
            inputs,
            tile_outputs,
            object_outputs,
            region,
            window,
        })
    }

    /// Where a full run may match a rule whose input box is `input_box`: inside a finite map,
    /// unless the rule map lets rules match outside.
    fn fit(run: &RuleMapRun<'_>, input_box: Bx) -> Option<IRect> {
        (!run.match_outside_map)
            .then(|| run.inside(input_box))
            .flatten()
    }

    /// In a region run, the positions to try rule `index` at; `None` when there are none (and
    /// in every other run).
    fn window_area(&self, run: &RuleMapRun<'_>, index: usize, rule: &Rule) -> Option<Bx> {
        let window = run.window?;
        let reach = self.set.locality.reach(run.index, index);
        window.rule_area(rule.input_box, reach, Self::fit(run, rule.input_box))
    }

    /// The rules of a rule map that can match in this run, bound: enabled, with an output, and
    /// with an input set that is possible and whose layers allow it; in a region run, with a
    /// position to try in the window.
    fn bind_rules(&self, run: &RuleMapRun<'a>) -> Vec<BoundRule<'a>> {
        let mut bound = Vec::new();
        for (index, rule) in run.rule_map.rules.iter().enumerate() {
            if rule.options.disabled || !rule.has_output() {
                continue;
            }
            if run.window.is_some() && self.window_area(run, index, rule).is_none() {
                continue;
            }
            let mut sets = Vec::new();
            for (set_index, set) in rule.input_sets.iter().enumerate() {
                if set.never.is_some() {
                    continue;
                }
                let grids: Vec<Option<usize>> = set
                    .layers
                    .iter()
                    .map(|l| run.inputs.get(l.target).copied().flatten())
                    .collect();
                // A layer the map lacks reads as empty: a set needing a tile there can never
                // match (Tiled drops it).
                let impossible = set.layers.iter().zip(&grids).any(|(layer, grid)| {
                    grid.is_none()
                        && layer.positions.iter().any(|p| {
                            let any = set.any_of(p);
                            let none = set.none_of(p);
                            let empty_allowed = any.is_empty() || any.iter().any(|c| c.is_empty());
                            !empty_allowed || none.iter().any(|c| c.is_empty())
                        })
                });
                if impossible {
                    continue;
                }
                sets.push(BoundSet {
                    index: set_index,
                    set,
                    grids,
                    cells: set
                        .cells
                        .iter()
                        .map(|c| self.tilesets.bind_match(c))
                        .collect(),
                });
            }
            if !sets.is_empty() {
                bound.push(BoundRule { index, rule, sets });
            }
        }
        bound
    }

    /// The cells a region run recomputes and keeps; `None` in every other run.
    pub(crate) fn exact_area(&self) -> Option<Bx> {
        self.plan.as_ref().and_then(|plan| plan.exact)
    }

    /// Whether the rule's `Probability` skips it at `p`.
    fn skipped(&self, rule: RuleRef, probability: f64, p: IVec2) -> bool {
        let skip = 1.0 - probability;
        skip != 0.0 && rng::unit(self.seed, rule, p, Purpose::Skip) < skip
    }

    /// Whether `rule` holds at `p`.
    fn holds(&mut self, run: &RuleMapRun<'_>, rule: &BoundRule<'_>, p: IVec2) -> bool {
        let reader = Reader {
            targets: &self.targets,
            tilesets: &self.tilesets,
            border: run.border,
        };
        if O::ACTIVE {
            let rule_ref = RuleRef {
                rule_map: run.index,
                rule: rule.index,
            };
            let mut holds = false;
            for set in &rule.sets {
                if reader.holds_observed(set, rule_ref, p, &run.rule_map.input_names, self.observer)
                {
                    holds = true;
                }
            }
            holds
        } else {
            rule.sets.iter().any(|set| reader.holds(set, p))
        }
    }

    fn attempted(&mut self, rule: RuleRef, position: IVec2, outcome: Outcome<'_>) {
        if O::ACTIVE {
            self.observer.attempt(&Attempt {
                rule,
                position,
                outcome,
            });
        }
    }

    /// Runs rule map `index` around `region`; returns the cells its matches' outputs cover when
    /// `track` asks for them (where later rule maps' region grows).
    fn run_rule_map(&mut self, index: usize, region: &CellSet, track: bool) -> CellSet {
        let mut applied = CellSet::new();
        let Some(run) = self.bind_rule_map(index, region) else {
            return applied;
        };
        if !run.rule_map.is_used() {
            return applied;
        }
        if run.rule_map.options.delete_tiles {
            self.delete_tiles(&run);
        }
        let rules = self.bind_rules(&run);
        // Where each rule may be tried: in a full run shared by the rules of one size, in a
        // region run each rule's own.
        let mut areas: Vec<MatchArea> = Vec::new();
        let mut area_of: Vec<usize> = Vec::with_capacity(rules.len());
        let mut by_size: HashMap<IVec2, usize> = HashMap::new();
        for rule in &rules {
            let input_box = rule.rule.input_box;
            let area = if run.window.is_some() {
                areas.push(MatchArea::of_box(
                    self.window_area(&run, rule.index, rule.rule),
                ));
                areas.len() - 1
            } else {
                *by_size.entry(input_box.size()).or_insert_with(|| {
                    areas.push(MatchArea::new(
                        &run.region,
                        input_box,
                        Self::fit(&run, input_box),
                    ));
                    areas.len() - 1
                })
            };
            area_of.push(area);
        }
        let candidates: Vec<Vec<Vec<Anchor>>> = rules
            .iter()
            .map(|rule| rule.sets.iter().map(anchor_candidates).collect())
            .collect();
        let mut index = AnchorIndex::new(
            &self.targets,
            &self.tilesets,
            candidates.iter().flatten().flatten(),
            run.window.map(|w| w.cells.rect()),
        );
        let anchors: Vec<Option<Vec<Anchor>>> = rules
            .iter()
            .zip(&candidates)
            .map(|(rule, sets)| {
                let rule_ref = RuleRef {
                    rule_map: run.index,
                    rule: rule.index,
                };
                if O::ACTIVE && self.observer.tries_every_position(rule_ref) {
                    None
                } else {
                    choose_anchors(sets, &index)
                }
            })
            .collect();
        let mut tracked = track.then(CellSet::new);
        let area_of_rule = |position: usize| area_of.get(position).and_then(|&a| areas.get(a));
        if run.rule_map.options.match_in_order {
            for (position, (rule, anchors)) in rules.iter().zip(&anchors).enumerate() {
                let Some(area) = area_of_rule(position) else {
                    continue;
                };
                self.run_rule_in_order(
                    &run,
                    rule,
                    anchors.as_deref(),
                    area,
                    &mut index,
                    &mut tracked,
                );
            }
        } else {
            let mut found: Vec<Vec<IVec2>> = Vec::with_capacity(rules.len());
            for (position, (rule, anchors)) in rules.iter().zip(&anchors).enumerate() {
                let Some(area) = area_of_rule(position) else {
                    found.push(Vec::new());
                    continue;
                };
                found.push(self.collect_matches(&run, rule, anchors.as_deref(), area, &index));
            }
            for (rule, positions) in rules.iter().zip(found) {
                let mut overlaps = OverlapState::default();
                for p in positions {
                    self.apply_match(&run, rule, p, &mut overlaps, &mut tracked, None, None);
                }
            }
        }
        if let Some(tracked) = tracked {
            applied = tracked;
        }
        applied
    }

    /// The positions where a rule matches, against the layers as they are.
    fn collect_matches(
        &mut self,
        run: &RuleMapRun<'_>,
        rule: &BoundRule<'_>,
        anchors: Option<&[Anchor]>,
        area: &MatchArea,
        index: &AnchorIndex,
    ) -> Vec<IVec2> {
        let rule_ref = RuleRef {
            rule_map: run.index,
            rule: rule.index,
        };
        let options = rule.rule.options;
        let keys: Vec<ScanKey> = match anchors {
            Some(anchors) => anchored_positions(
                anchors,
                index,
                area,
                &options,
                run.rereads_border(rule.rule.input_box),
            ),
            None => area.lattice(&options).collect(),
        };
        let mut found = Vec::new();
        for key in keys {
            let p = key.position();
            if self.skipped(rule_ref, options.probability, p) {
                self.attempted(rule_ref, p, Outcome::SkippedByProbability);
                continue;
            }
            if self.holds(run, rule, p) {
                found.push(p);
            } else {
                self.attempted(rule_ref, p, Outcome::NoMatch);
            }
        }
        found
    }

    /// Matches and applies a rule position by position (`MatchInOrder`).
    fn run_rule_in_order(
        &mut self,
        run: &RuleMapRun<'_>,
        rule: &BoundRule<'_>,
        anchors: Option<&[Anchor]>,
        area: &MatchArea,
        index: &mut AnchorIndex,
        tracked: &mut Option<CellSet>,
    ) {
        let rule_ref = RuleRef {
            rule_map: run.index,
            rule: rule.index,
        };
        let options = rule.rule.options;
        let mut overlaps = OverlapState::default();
        let mut try_at = |run_state: &mut Self,
                          p: IVec2,
                          index: &mut AnchorIndex,
                          pending: Option<&mut Pending<'_>>| {
            if run_state.skipped(rule_ref, options.probability, p) {
                run_state.attempted(rule_ref, p, Outcome::SkippedByProbability);
                return;
            }
            if run_state.holds(run, rule, p) {
                run_state.apply_match(run, rule, p, &mut overlaps, tracked, Some(index), pending);
            } else {
                run_state.attempted(rule_ref, p, Outcome::NoMatch);
            }
        };
        match anchors {
            Some(anchors) => {
                let mut pending = Pending {
                    current: ScanKey {
                        rect: 0,
                        y: i32::MIN,
                        x: i32::MIN,
                    },
                    keys: anchored_positions(
                        anchors,
                        index,
                        area,
                        &options,
                        run.rereads_border(rule.rule.input_box),
                    )
                    .into_iter()
                    .collect::<BTreeSet<ScanKey>>(),
                    anchors,
                    area,
                    options,
                };
                while let Some(key) = pending.keys.pop_first() {
                    pending.current = key;
                    try_at(self, key.position(), index, Some(&mut pending));
                }
            }
            None => {
                for key in area.lattice(&options) {
                    try_at(self, key.position(), index, None);
                }
            }
        }
    }
}

/// Runs `set` on `map` as `request` asks, reporting to `observer`.
pub(crate) fn run<O: Observer>(
    set: &RuleSet,
    map: &mut Map,
    request: &RunRequest,
    observer: &mut O,
) -> Report {
    let mut report = Report::default();
    if let Some(problem) = refused_bindings(map, &request.layer_names) {
        report.problems.push(problem);
        return report;
    }
    let map_rect = IRect {
        x: 0,
        y: 0,
        w: i32::try_from(map.width).unwrap_or(i32::MAX),
        h: i32::try_from(map.height).unwrap_or(i32::MAX),
    };
    let whole = || {
        if map.infinite {
            // The union of the tile layers' chunks, as Tiled's AutoMap takes it.
            let mut bounds: Option<IRect> = None;
            map.for_each_layer(|layer, _| {
                if let Some(chunks) = layer.as_tile_layer().and_then(|t| t.chunk_bounds()) {
                    bounds = Some(bounds.map_or(chunks, |b| b.union(chunks)));
                }
            });
            bounds.map_or_else(CellSet::new, CellSet::from_rect)
        } else {
            CellSet::from_rect(map_rect)
        }
    };
    let plan = match &request.area {
        Area::Edit(edit) => match plan_region(&set.locality, set.rule_maps(), map, *edit) {
            Ok(plan) => Some(plan),
            Err(problem) => {
                report.problems.push(problem);
                None
            }
        },
        Area::Whole | Area::Selection(_) => None,
    };
    if let Some(plan) = plan {
        report.incremental = true;
        if plan.exact.is_none() {
            report.rule_map_matches = vec![0; set.rule_maps().len()];
            return report;
        }
        let interest = plan.interest((!map.infinite).then_some(map_rect));
        let mut run_state = Run::new(set, map, request, observer, interest, Some(plan));
        let region = CellSet::new();
        for index in 0..set.rule_maps().len() {
            run_state.run_rule_map(index, &region, false);
        }
        let state = writeback::RunResult::from_run(run_state);
        writeback::write_back(set, map, state, &mut report);
        return report;
    }
    let mut region = match &request.area {
        Area::Whole | Area::Edit(_) => whole(),
        Area::Selection(rects) => {
            report.incremental = true;
            let mut cells = CellSet::new();
            for rect in rects {
                cells.insert_rect(*rect);
            }
            cells
        }
    };
    let interest = area_of_interest(set, map, &region, map_rect);
    let mut run_state = Run::new(set, map, request, observer, interest, None);
    // Tiled grows the region by what each rule map wrote, until it covers a finite map.
    let mut tracking = map.infinite || !covers(&region, map_rect);
    for index in 0..set.rule_maps().len() {
        if tracking && !map.infinite && covers(&region, map_rect) {
            tracking = false;
        }
        let applied = run_state.run_rule_map(index, &region, tracking);
        if tracking {
            region.insert_set(&applied);
            if !map.infinite {
                region = region.intersection_with_rect(map_rect);
            }
        }
    }
    let state = writeback::RunResult::from_run(run_state);
    writeback::write_back(set, map, state, &mut report);
    report
}

/// Whether `region` holds every cell of `rect`.
fn covers(region: &CellSet, rect: IRect) -> bool {
    region.intersection_with_rect(rect).len()
        == usize::try_from(i64::from(rect.w.max(0)) * i64::from(rect.h.max(0)))
            .unwrap_or(usize::MAX)
}

/// The cells grids copy densely: the region grown by the farthest any rule reads or writes,
/// within a finite map.
fn area_of_interest(set: &RuleSet, map: &Map, region: &CellSet, map_rect: IRect) -> IRect {
    if !map.infinite {
        return map_rect;
    }
    let Some(bounds) = region.bounding() else {
        return IRect::default();
    };
    let mut reach = IVec2::ZERO;
    for rule_map in set.rule_maps() {
        for rule in &rule_map.rules {
            let mut extent = rule.input_box.size();
            if let Some(out) = rule.output_cells.bounding() {
                extent = extent
                    .max(out.max.abs() + IVec2::ONE)
                    .max(out.min.abs() + IVec2::ONE);
            }
            reach = reach.max(extent + IVec2::splat(rule_map.options.radius.max(0)));
        }
    }
    let grown = Bx::grown(bounds, reach);
    grown.rect()
}

impl RuleSet {
    /// Runs the rules on the whole map (`None`), or after an edit of `region`: an exact region
    /// run ([`Area::Edit`]) that gives what a full run gives inside the area the edit can
    /// change ([`RuleSet::affected_area`]) and changes nothing outside it, or, when the rules
    /// cannot be run exactly around an edit, a full run whose report says why.
    pub fn apply(&self, map: &mut Map, region: Option<IRect>, seed: u64) -> Report {
        let area = region.map_or(Area::Whole, Area::Edit);
        self.run(map, &RunRequest::builder(seed).area(area).build())
    }

    /// Runs the rules around the cells of a selection, as Tiled runs them on a selection: a
    /// rule is tried where its input box overlaps a selected cell, and each rule map's region
    /// grows by what the earlier ones wrote.
    pub fn apply_to(&self, map: &mut Map, rects: &[IRect], seed: u64) -> Report {
        let area = Area::Selection(rects.to_vec());
        self.run(map, &RunRequest::builder(seed).area(area).build())
    }

    /// Runs the rules as `request` asks.
    pub fn run(&self, map: &mut Map, request: &RunRequest) -> Report {
        run(self, map, request, &mut crate::observer::NoObserver)
    }

    /// Runs the rules as `request` asks, reporting every attempt, check and write to
    /// `observer`.
    pub fn run_observed<O: Observer>(
        &self,
        map: &mut Map,
        request: &RunRequest,
        observer: &mut O,
    ) -> Report {
        run(self, map, request, observer)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Cell, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
    use glam::UVec2;

    use super::*;
    use crate::observer::{Check, Write};
    use crate::problem::ProblemKind;

    /// Counts what a run reports.
    #[derive(Default)]
    struct Counting {
        every_position: bool,
        applied: usize,
        no_match: usize,
        checks: usize,
        holding: usize,
        writes: Vec<IVec2>,
    }

    impl Observer for Counting {
        fn tries_every_position(&self, _rule: RuleRef) -> bool {
            self.every_position
        }

        fn attempt(&mut self, attempt: &Attempt<'_>) {
            match attempt.outcome {
                Outcome::Applied { .. } => self.applied += 1,
                Outcome::NoMatch => self.no_match += 1,
                _ => {}
            }
        }

        fn check(&mut self, check: &Check<'_>) {
            self.checks += 1;
            if check.holds {
                self.holding += 1;
            }
        }

        fn write(&mut self, write: &Write<'_>) {
            self.writes.push(write.at);
        }
    }

    fn rules_and_map() -> (RuleSet, Map) {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let rule_map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 3))
            .tileset(Arc::clone(&atlas))
            .tileset(rule_tiles().unwrap())
            .tile_layer("input_set", ". . .\n. 2 .\n. . .")
            .tile_layer("output_set", ". . .\n. 3 .\n. . .")
            .build()
            .unwrap();
        let rules = RuleSet::builder()
            .rule_map("/rules/rules.tmx", Arc::new(rule_map))
            .build();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 1))
            .tileset(atlas)
            .tile_layer("set", "2 . 2 1")
            .build()
            .unwrap();
        (rules, map)
    }

    #[test]
    fn an_observer_sees_every_attempt_check_and_write() {
        let (rules, mut map) = rules_and_map();
        let mut observer = Counting::default();
        let report = rules.run_observed(&mut map, &RunRequest::builder(1).build(), &mut observer);
        // Anchors: the rule is tried only where its tile is.
        assert_eq!((observer.applied, observer.no_match), (2, 0));
        assert_eq!((observer.checks, observer.holding), (2, 2));
        assert_eq!(observer.writes, [IVec2::new(0, 0), IVec2::new(2, 0)]);
        assert_eq!(report.matches, 2);
        assert_eq!(report.rule_map_matches, [2]);
        let three = Cell::new(TilesetSlot::new(0).unwrap(), TileId(3));
        let layer = map.layers[0].as_tile_layer().unwrap();
        assert_eq!(layer.cell(IVec2::new(2, 0)), three);
        assert_eq!(
            report.changed,
            Some(IRect {
                x: 0,
                y: 0,
                w: 3,
                h: 1
            })
        );
    }

    #[test]
    fn an_observer_can_have_every_position_tried() {
        let (rules, mut map) = rules_and_map();
        let mut observer = Counting {
            every_position: true,
            ..Counting::default()
        };
        rules.run_observed(&mut map, &RunRequest::builder(1).build(), &mut observer);
        assert_eq!((observer.applied, observer.no_match), (2, 2));
        assert_eq!((observer.checks, observer.holding), (4, 2));
        // The plain run gives the same map.
        let (rules, mut plain) = rules_and_map();
        rules.apply(&mut plain, None, 1);
        assert_eq!(plain, map);
    }

    #[test]
    fn a_run_whose_bindings_are_refused_does_nothing() {
        let (rules, mut map) = rules_and_map();
        let before = map.clone();
        let names = LayerNames::builder()
            .tiles("set", bt_core::LayerId(42))
            .build();
        let report = rules.run(&mut map, &RunRequest::builder(1).layer_names(names).build());
        assert_eq!(map, before);
        assert_eq!(
            report.problems.iter().map(|p| p.kind).collect::<Vec<_>>(),
            [ProblemKind::BindingRefused]
        );
    }
}
