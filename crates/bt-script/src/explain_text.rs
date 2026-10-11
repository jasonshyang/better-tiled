//! `bt explain MAP X,Y` (`10-agents.md`, §10.6): what an AutoMap run does at a cell, and why,
//! from `bt-automap`'s explanation of a run on the map as it is (`08-automap.md`, §8). A tile at
//! the cell no rule names is said first; then each layer the rules write, before and after; the
//! changes at the cell in order, each by its rule (named by rule map and number, with its cells
//! in the rule map) and where the rule matched; the rules that matched and changed nothing, with
//! why; the rules that came close, each with what it would write and its conditions that fail
//! (what the layer must hold and what it has, and what the run changed there earlier); and the
//! rest counted, listed when all are asked for. Rules match layers by exact name: a layer the map
//! lacks reads as empty, with a near miss pointed out.
//!
//! On a map with AutoMap layers, each run is explained on what it reads, in the map's layer
//! names. Loading the rules is the caller's: it gives the rule set and the bindings. The editor's
//! Explain view (`apps/better-tiled/src/panels/automap_explain.rs`) groups the same way.

use std::sync::Arc;

use bt_automap::layers::{AutoLayers, Setup};
use bt_automap::{
    AttemptOutcome, Condition, ConditionCheck, Explanation, LayerNames, MatchCell, PictureRole,
    RuleAttempt, RuleRef, RuleSet, RuleSummary, RunRequest, Unbound, Writer,
};
use bt_core::{Cell, Map, TilesetSlot};
use bt_utils::text::{and_list, plural, slug};
use glam::{IVec2, UVec2};
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::info::RuleNote;
use crate::names::Names;
use crate::objects::flips_text;
use crate::report::{Line, LineKind, Outcome, Report, Section};
use crate::rule_words::{
    near_miss_layer, problem_line, rule_map_name, rules_file_name, tile_layer_named,
};
use crate::shapes::map_cells;
use crate::words::{cell_text, region_text};

/// The rules that came close listed unless all are asked for.
const CLOSE_LISTED: usize = 10;

/// The tiles a condition or an unnamed tile's line names before "and N more".
const TILES_NAMED: usize = 8;

/// What `bt explain` is asked.
pub struct ExplainRequest<'a> {
    map: &'a Map,
    rules: &'a RuleSet,
    rules_file: String,
    cell: IVec2,
    seed: Option<u64>,
    bindings: LayerNames,
    layers: Option<AutoMapLayers<'a>>,
    run: Option<usize>,
    all: bool,
}

/// A map's AutoMap layers: what was painted and their setup.
struct AutoMapLayers<'a> {
    painted: &'a Map,
    setup: &'a Setup,
}

impl<'a> ExplainRequest<'a> {
    /// What a run of `rules` does at `cell` of `map`, with seed 1 and the rules finding layers
    /// by name; the rules are named by the file they were read from.
    pub fn new(map: &'a Map, rules: &'a RuleSet, cell: IVec2) -> ExplainRequest<'a> {
        ExplainRequest {
            map,
            rules,
            rules_file: rules
                .rules_file()
                .map_or_else(|| "the rules".to_owned(), rules_file_name),
            cell,
            seed: None,
            bindings: LayerNames::by_name(),
            layers: None,
            run: None,
            all: false,
        }
    }

    /// What the rules are called: the rules file as the command was given it.
    pub fn rules_file(mut self, name: impl Into<String>) -> ExplainRequest<'a> {
        self.rules_file = name.into();
        self
    }

    /// The seed of the rules' random choices (else 1, or the AutoMap layers' own).
    pub fn seed(mut self, seed: u64) -> ExplainRequest<'a> {
        self.seed = Some(seed);
        self
    }

    /// The map layers that stand for names the rules use (`RULE-LAYER=MAP-LAYER`).
    pub fn bindings(mut self, bindings: LayerNames) -> ExplainRequest<'a> {
        self.bindings = bindings;
        self
    }

    /// The map has AutoMap layers: explain each of their runs on what it reads, from what was
    /// painted, with their setup.
    pub fn automap_layers(mut self, painted: &'a Map, setup: &'a Setup) -> ExplainRequest<'a> {
        self.layers = Some(AutoMapLayers { painted, setup });
        self
    }

    /// Only the AutoMap layers' run `run`, counted from 1.
    pub fn run(mut self, run: usize) -> ExplainRequest<'a> {
        self.run = Some(run);
        self
    }

    /// List every rule that could write the cell, not only those that came close.
    pub fn all(mut self, all: bool) -> ExplainRequest<'a> {
        self.all = all;
        self
    }
}

/// `bt explain MAP X,Y`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplainOutcome {
    /// The cell.
    pub cell: IVec2,
    /// The rules file, by name.
    pub rules: String,
    /// How many rule maps there are.
    pub rule_maps: usize,
    /// How many rules there are.
    pub rule_count: usize,
    /// The seed of the rules' random choices (of the first run explained).
    pub seed: u64,
    /// Whether the runs are the map's AutoMap layers'.
    pub automap_layers: bool,
    /// What the rules warn about: rule maps that can't be read or used, tilesets the rules
    /// copy.
    pub warnings: Vec<RuleNote>,
    /// The runs explained.
    pub runs: Vec<ExplainedRun>,
    /// Whether every rule that could write the cell is listed.
    pub all: bool,
}

/// One run at the cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplainedRun {
    /// The run's heading on a map with AutoMap layers: `run 1 of 2 (walls=plat1)`.
    pub run: Option<String>,
    /// The bindings the rules ran with, as `RULE-LAYER=MAP-LAYER`.
    pub bindings: Vec<String>,
    /// The size of the finite map the cell is outside of, when it is.
    pub outside: Option<UVec2>,
    /// Why the run did not start, when it did not.
    pub problems: Vec<String>,
    /// Layers the rules read that the map does not have, which read as empty.
    pub missing: Vec<MissingLayer>,
    /// Tiles at the cell no rule's conditions name.
    pub unnamed: Vec<String>,
    /// Each layer the rules write, before and after: `set: a → b`.
    pub layers: Vec<String>,
    /// The writes to the cell, in order.
    pub changes: Vec<String>,
    /// Rules that matched and changed nothing here.
    pub matched: Vec<RuleTry>,
    /// Rules that came close: a condition of theirs about particular tiles holds.
    pub close: Vec<RuleTry>,
    /// The other rules that write the cell from some position.
    pub others: Vec<RuleTry>,
    /// Why the others fail, said once for all of them.
    pub others_reason: String,
}

/// A layer the rules read that the map lacks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingLayer {
    /// The name the rules use.
    pub name: String,
    /// A layer of the map named almost so.
    pub near: Option<String>,
}

/// A rule tried at a position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleTry {
    /// The rule: `rule 3 of rules.tmx (its cells at 5,1 1x1)`.
    pub rule: String,
    /// Where the top-left of its input landed on the map.
    pub at: IVec2,
    /// What came of it.
    pub outcome: String,
    /// What it would write at the cell: `Ground sewer-tileset:17`.
    pub would_write: Vec<String>,
    /// How many input sets it has (one is enough).
    pub input_sets: usize,
    /// The conditions of its closest input set that do not hold.
    pub failing: Vec<String>,
    /// Conditions left out because they read outside the map.
    pub outside_map: usize,
}

impl ExplainOutcome {
    /// What `request` asks.
    pub fn from_request(request: &ExplainRequest<'_>) -> Result<ExplainOutcome, Error> {
        let rules = request.rules;
        let mut warnings: Vec<RuleNote> = rules.problems().iter().map(note_of).collect();
        for (index, rule_map) in rules.rule_maps().iter().enumerate() {
            for problem in rule_map
                .problems()
                .iter()
                .filter(|p| p.severity() == bt_automap::Severity::Error)
            {
                warnings.push(RuleNote {
                    kind: LineKind::Warning,
                    text: format!(
                        "{} is not used: {}",
                        rule_map_name(rules, index),
                        problem.text
                    ),
                });
            }
        }
        let summaries = rules.rule_summaries();
        let mut runs = Vec::new();
        let seed;
        match &request.layers {
            None => {
                if let Some(run) = request.run {
                    return Err(Error::RunWithoutLayers { run });
                }
                seed = request.seed.unwrap_or(1);
                let names = Names::of(request.map);
                warnings.extend(copies_of(rules, request.map));
                let explained = Explained {
                    map: request.map,
                    names: &names,
                    rules,
                    summaries: &summaries,
                    bindings: &request.bindings,
                };
                runs.push(explained.run(request.cell, seed, None));
            }
            Some(layers) => {
                let mut setup = layers.setup.clone();
                if let Some(seed) = request.seed {
                    setup.seed = seed;
                }
                seed = setup.seed;
                let count = setup.runs.len();
                let which: Vec<usize> = match request.run {
                    Some(run) if (1..=count).contains(&run) => vec![run - 1],
                    Some(run) => return Err(Error::NoSuchRun { run, runs: count }),
                    None => (0..count).collect(),
                };
                let stages = AutoLayers::derive(Arc::new(layers.painted.clone()), &setup, rules);
                warnings.extend(copies_of(rules, stages.result()));
                for index in which {
                    let (Some(input), Some(bindings)) =
                        (stages.input_of(index), setup.runs.get(index))
                    else {
                        continue;
                    };
                    let names = Names::of(input);
                    let explained = Explained {
                        map: input,
                        names: &names,
                        rules,
                        summaries: &summaries,
                        bindings,
                    };
                    let heading = format!(
                        "run {} of {count}{}",
                        index + 1,
                        bindings_text(bindings, input)
                            .map_or_else(String::new, |b| format!(" ({})", b.join(", ")))
                    );
                    runs.push(explained.run(request.cell, setup.run_seed(index), Some(heading)));
                }
            }
        }
        Ok(ExplainOutcome {
            cell: request.cell,
            rules: request.rules_file.clone(),
            rule_maps: rules.rule_maps().len(),
            rule_count: rules.rule_count(),
            seed,
            automap_layers: request.layers.is_some(),
            warnings,
            runs,
            all: request.all,
        })
    }
}

fn note_of(problem: &bt_automap::Problem) -> RuleNote {
    let line = problem_line(problem);
    RuleNote {
        kind: line.kind,
        text: line.text,
    }
}

/// The tilesets the rules use that copy one of the map's, as warnings.
fn copies_of(rules: &RuleSet, map: &Map) -> Vec<RuleNote> {
    rules
        .tileset_copies(map)
        .iter()
        .map(|copy| note_of(&copy.problem()))
        .collect()
}

/// The bindings as `RULE-LAYER=MAP-LAYER`; `None` when there are none.
fn bindings_text(bindings: &LayerNames, map: &Map) -> Option<Vec<String>> {
    let name = |id: bt_core::LayerId| {
        map.layer(id)
            .map_or_else(|| format!("#{}", id.0), |layer| layer.name.clone())
    };
    let mut words: Vec<String> = bindings
        .tile_bindings()
        .iter()
        .map(|b| format!("{}={}", b.name, name(b.layer)))
        .collect();
    words.extend(
        bindings
            .object_bindings()
            .iter()
            .map(|b| format!("{}={} (objects)", b.name, name(b.layer))),
    );
    (!words.is_empty()).then_some(words)
}

/// What explains one run.
struct Explained<'e> {
    map: &'e Map,
    names: &'e Names,
    rules: &'e RuleSet,
    summaries: &'e [RuleSummary],
    bindings: &'e LayerNames,
}

impl Explained<'_> {
    /// The run at `cell` with `seed`.
    fn run(&self, cell: IVec2, seed: u64, heading: Option<String>) -> ExplainedRun {
        let request = RunRequest::builder(seed)
            .layer_names(self.bindings.clone())
            .build();
        let explanation = self
            .rules
            .explain_cell_with(self.map, cell, &request)
            .with_layer_names(self.bindings, self.map);
        let words = TileWords {
            map: self.map,
            names: self.names,
            rules: self.rules,
            explanation: &explanation,
        };
        let outside = (!self.map.infinite && !map_cells(self.map).contains(cell))
            .then(|| UVec2::new(self.map.width, self.map.height));
        let unnamed = explanation
            .unnamed
            .iter()
            .map(|u| {
                let named: Vec<String> = u.named.iter().map(|t| words.table_tile(*t)).collect();
                format!(
                    "{} has {}, which no rule's conditions name (they name {} on {}): the rules see it only as a tile",
                    u.layer,
                    words.run_cell(u.tile),
                    crate::words::capped_list(&named, TILES_NAMED),
                    u.layer
                )
            })
            .collect();
        let layers = explanation
            .layers
            .iter()
            .filter(|layer| layer.present || layer.before != layer.after)
            .map(|layer| {
                let made = if layer.present {
                    ""
                } else {
                    " (the map has no such layer: the run makes it)"
                };
                if layer.before == layer.after {
                    format!(
                        "{}: {}, unchanged{made}",
                        layer.layer,
                        words.run_cell(layer.before)
                    )
                } else {
                    format!(
                        "{}: {} → {}{made}",
                        layer.layer,
                        words.run_cell(layer.before),
                        words.run_cell(layer.after)
                    )
                }
            })
            .collect();
        let changes = explanation
            .writes
            .iter()
            .enumerate()
            .map(|(index, write)| {
                let by = match write.by {
                    Writer::Rule { rule, position } => format!(
                        "{}, matched at {}",
                        self.rule_text(rule),
                        cell_text(position)
                    ),
                    Writer::DeleteTiles { rule_map } => {
                        format!("DeleteTiles of {}", rule_map_name(self.rules, rule_map))
                    }
                };
                format!(
                    "{}. {} ← {} by {by}",
                    index + 1,
                    write.layer,
                    words.run_cell(write.after)
                )
            })
            .collect();
        let groups = Groups::of(&explanation);
        let others_reason = others_reason(&groups.others);
        ExplainedRun {
            run: heading,
            bindings: bindings_text(self.bindings, self.map).unwrap_or_default(),
            outside,
            problems: explanation
                .problems
                .iter()
                .map(|p| p.text.clone())
                .collect(),
            missing: self.missing_inputs(),
            unnamed,
            layers,
            changes,
            matched: groups
                .matched
                .iter()
                .map(|a| self.rule_try(a, cell, &words))
                .collect(),
            close: groups
                .close
                .iter()
                .map(|a| self.rule_try(a, cell, &words))
                .collect(),
            others: groups
                .others
                .iter()
                .map(|a| self.rule_try(a, cell, &words))
                .collect(),
            others_reason,
        }
    }

    /// `rule 3 of rules.tmx (its cells at 5,1 1x1)`.
    fn rule_text(&self, rule: RuleRef) -> String {
        let cells = self.summary(rule).map_or_else(String::new, |s| {
            format!(" (its cells at {})", region_text(s.area))
        });
        format!(
            "rule {} of {}{cells}",
            rule.rule + 1,
            rule_map_name(self.rules, rule.rule_map)
        )
    }

    fn summary(&self, rule: RuleRef) -> Option<&RuleSummary> {
        self.summaries.iter().find(|s| s.rule == rule)
    }

    /// The layers the rules read that the run has no layer for, leaving out those an earlier
    /// rule map writes: bound names have their layer, others are found by exact name.
    fn missing_inputs(&self) -> Vec<MissingLayer> {
        let mut written: Vec<&str> = Vec::new();
        let mut missing: Vec<MissingLayer> = Vec::new();
        for rule_map in self.rules.rule_maps().iter().filter(|r| r.is_used()) {
            for name in rule_map.inputs() {
                let has = self.bindings.tile_layer(name).is_some()
                    || (self.bindings.unbound() == Unbound::LookUpByName
                        && tile_layer_named(self.map, name).is_some());
                if has
                    || written.contains(&name.as_str())
                    || missing.iter().any(|m| m.name == *name)
                {
                    continue;
                }
                missing.push(MissingLayer {
                    name: name.clone(),
                    near: near_miss_layer(self.map, name),
                });
            }
            written.extend(rule_map.tile_outputs().iter().map(String::as_str));
        }
        missing
    }

    /// A rule tried at a position, as a line and its details.
    fn rule_try(&self, attempt: &RuleAttempt, cell: IVec2, words: &TileWords<'_>) -> RuleTry {
        let closest = attempt.closest_set();
        let input_sets = {
            let mut sets: Vec<usize> = attempt.checks.iter().map(|c| c.input_set).collect();
            sets.sort_unstable();
            sets.dedup();
            sets.len()
        };
        let skip_outside = attempt.outcome == AttemptOutcome::OutsideMap;
        let bounds = map_cells(self.map);
        let mut outside_map = 0;
        let mut failing = Vec::new();
        for check in attempt
            .checks
            .iter()
            .filter(|c| !c.holds && Some(c.input_set) == closest)
        {
            if skip_outside && !self.map.infinite && !bounds.contains(check.at) {
                outside_map += 1;
                continue;
            }
            failing.push(words.condition(check));
        }
        RuleTry {
            rule: self.rule_text(attempt.rule),
            at: attempt.position,
            outcome: self.outcome_text(attempt),
            would_write: self.would_write(attempt.rule, attempt.position, cell, words),
            input_sets,
            failing,
            outside_map,
        }
    }

    /// What came of an attempt, in words.
    fn outcome_text(&self, attempt: &RuleAttempt) -> String {
        let failures = attempt.failures().unwrap_or(0);
        let checked = attempt.closest_set().map_or(0, |set| {
            attempt.checks.iter().filter(|c| c.input_set == set).count()
        });
        match &attempt.outcome {
            AttemptOutcome::Applied { locked, .. } if !locked.is_empty() => {
                let layers: Vec<String> = locked
                    .iter()
                    .map(|id| {
                        self.map
                            .layer(*id)
                            .map_or_else(|| format!("#{}", id.0), |l| l.name.clone())
                    })
                    .collect();
                format!(
                    "matched, but its output to {} was left out: the layer is locked",
                    and_list(&layers)
                )
            }
            AttemptOutcome::Applied { .. } => {
                "matched and applied, but the random output it picked writes nothing here".to_owned()
            }
            AttemptOutcome::SkippedByProbability => {
                "matched, but its Probability left it out here".to_owned()
            }
            AttemptOutcome::Overlapping => {
                "matched, but its output would overlap an earlier match of the same rule (NoOverlappingOutput)".to_owned()
            }
            AttemptOutcome::NoMatch => conditions_text(failures, checked),
            AttemptOutcome::OutsideMap => {
                "not tried there: it would reach outside the map (MatchOutsideMap is off)".to_owned()
            }
            AttemptOutcome::OffLattice => {
                "not tried there: ModX/ModY and OffsetX/OffsetY leave that position out".to_owned()
            }
            AttemptOutcome::Disabled => "disabled (the Disabled option)".to_owned(),
            AttemptOutcome::CanNeverMatch => {
                "never matches: no input set can hold, or it has no output".to_owned()
            }
        }
    }

    /// What a rule matched at `position` would write at `cell`, by layer: from the rule map's
    /// output layers, in the map's names.
    fn would_write(
        &self,
        rule: RuleRef,
        position: IVec2,
        cell: IVec2,
        words: &TileWords<'_>,
    ) -> Vec<String> {
        let Some(summary) = self.summary(rule) else {
            return Vec::new();
        };
        let Some(picture) = self.rules.rule_picture(rule) else {
            return Vec::new();
        };
        let Some(rule_map) = self.rules.rule_maps().get(rule.rule_map) else {
            return Vec::new();
        };
        let at = cell - position + summary.input.min();
        let mut by_layer: Vec<LayerWrites> = Vec::new();
        for layer in picture
            .layers
            .iter()
            .filter(|l| l.role == PictureRole::Output)
        {
            let Some(target) = layer.name.split_once('_').map(|(_, name)| name) else {
                continue;
            };
            let shown = self
                .bindings
                .tile_layer(target)
                .and_then(|id| self.map.layer(id))
                .map_or_else(|| target.to_owned(), |l| l.name.clone());
            for placed in layer.cells.iter().filter(|p| p.at == at) {
                let Some(text) = words.rule_map_tile(rule_map.map(), placed.cell) else {
                    continue;
                };
                match by_layer.iter_mut().find(|writes| writes.layer == shown) {
                    Some(writes) if !writes.tiles.contains(&text) => writes.tiles.push(text),
                    Some(_) => {}
                    None => by_layer.push(LayerWrites {
                        layer: shown.clone(),
                        tiles: vec![text],
                    }),
                }
            }
        }
        by_layer
            .into_iter()
            .map(|writes| match writes.tiles.as_slice() {
                [one] => format!("{} {one}", writes.layer),
                _ => format!("{} one of {}", writes.layer, writes.tiles.join(", ")),
            })
            .collect()
    }
}

/// The tiles a rule would write on a layer.
struct LayerWrites {
    layer: String,
    tiles: Vec<String>,
}

/// How near an attempt came, for choosing each rule's best: what came of it, then how many of
/// its conditions fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    /// Applied, matched but held back, tried, or never tried.
    group: u8,
    /// Failing conditions of a tried rule.
    failures: usize,
}

/// `1 of its 3 conditions doesn't hold`.
fn conditions_text(failing: usize, of: usize) -> String {
    match (failing, of) {
        (_, 0) => "it matches nothing here".to_owned(),
        (1, 1) => "its condition doesn't hold".to_owned(),
        (1, _) => format!("1 of its {of} conditions doesn't hold"),
        _ => format!("{failing} of its {of} conditions don't hold"),
    }
}

/// The rules of an explanation by what came of them at the cell, each rule once, at its best
/// position, those that wrote the cell left out: as the editor's Explain view groups them.
struct Groups<'e> {
    matched: Vec<&'e RuleAttempt>,
    close: Vec<&'e RuleAttempt>,
    others: Vec<&'e RuleAttempt>,
}

impl<'e> Groups<'e> {
    fn of(explanation: &'e Explanation) -> Groups<'e> {
        let rank = |a: &RuleAttempt| -> Rank {
            let rank = |group, failures| Rank { group, failures };
            match a.outcome {
                AttemptOutcome::Applied { .. } => rank(0, 0),
                AttemptOutcome::SkippedByProbability | AttemptOutcome::Overlapping => rank(1, 0),
                AttemptOutcome::NoMatch
                | AttemptOutcome::OutsideMap
                | AttemptOutcome::OffLattice => rank(2, a.failures().unwrap_or(usize::MAX)),
                AttemptOutcome::Disabled | AttemptOutcome::CanNeverMatch => rank(3, 0),
            }
        };
        let mut best: Vec<&RuleAttempt> = Vec::new();
        for attempt in &explanation.attempts {
            match best.iter_mut().find(|b| b.rule == attempt.rule) {
                Some(kept) if rank(attempt) < rank(kept) => *kept = attempt,
                Some(_) => {}
                None => best.push(attempt),
            }
        }
        let wrote = |a: &RuleAttempt| {
            explanation.writes.iter().any(|w| {
                w.by == Writer::Rule {
                    rule: a.rule,
                    position: a.position,
                }
            })
        };
        let mut groups = Groups {
            matched: Vec::new(),
            close: Vec::new(),
            others: Vec::new(),
        };
        for attempt in best {
            if wrote(attempt) {
                continue;
            } else if attempt.matched || matches!(attempt.outcome, AttemptOutcome::Applied { .. }) {
                groups.matched.push(attempt);
            } else if attempt.holds_a_tile() {
                groups.close.push(attempt);
            } else {
                groups.others.push(attempt);
            }
        }
        let holding = |a: &RuleAttempt| {
            let closest = a.closest_set();
            a.checks
                .iter()
                .filter(|c| c.holds && c.names_tiles() && Some(c.input_set) == closest)
                .count()
        };
        groups.close.sort_by_key(|a| {
            (
                a.failures().unwrap_or(usize::MAX),
                std::cmp::Reverse(holding(a)),
                a.rule,
            )
        });
        groups.matched.sort_by_key(|a| (rank(a), a.rule));
        groups.others.sort_by_key(|a| (rank(a), a.rule));
        groups
    }
}

/// Why the other rules fail, said once: they need tiles on layers the map lacks or has empty
/// here, or nothing here is what they look for.
fn others_reason(others: &[&RuleAttempt]) -> String {
    let mut layers: Vec<String> = Vec::new();
    let mut missing = true;
    let mut any = false;
    for check in others.iter().flat_map(|a| &a.checks).filter(|c| !c.holds) {
        if !check.found.is_empty() || check.condition != Condition::AnyOf {
            return "nothing here is what they look for".to_owned();
        }
        any = true;
        missing &= !check.layer_present;
        if !layers.contains(&check.layer) {
            layers.push(check.layer.clone());
        }
    }
    if !any {
        return "nothing here is what they look for".to_owned();
    }
    let lacks = if missing {
        "which the map lacks"
    } else if layers.len() == 1 {
        "which is empty here"
    } else {
        "which are empty here"
    };
    format!("they need tiles on {}, {lacks}", and_list(&layers))
}

/// How tiles of an explanation read.
struct TileWords<'w> {
    map: &'w Map,
    names: &'w Names,
    rules: &'w RuleSet,
    explanation: &'w Explanation,
}

impl TileWords<'_> {
    /// A cell of the run: a tile of the map's tilesets by its name, a tile of a tileset the run
    /// added by its tileset's; `empty` for none.
    fn run_cell(&self, cell: Cell) -> String {
        let Some(slot) = cell.slot() else {
            return "empty".to_owned();
        };
        let flipped = flipped_text(cell);
        if slot.index() < self.map.tilesets.len() {
            let plain = Cell::new(slot, cell.tile());
            return format!("{}{flipped}", self.names.tile_text(plain));
        }
        let added = slot
            .index()
            .checked_sub(self.map.tilesets.len())
            .and_then(|i| self.explanation.added_tilesets.get(i))
            .and_then(|table| self.rules.tileset(*table));
        match added {
            Some(tileset) => format!(
                "{}:{} (the rules' tileset){flipped}",
                slug(&tileset.tileset.name),
                cell.tile()
            ),
            None => format!("tile {}{flipped}", cell.tile()),
        }
    }

    /// A tile the rules name, by the map's name for it when the run binds its tileset to one of
    /// the map's.
    fn table_tile(&self, tile: bt_automap::TableTile) -> String {
        match self.rules.tileset(tile.tileset) {
            Some(reference) => self.tile_of(reference, tile.tile, tile.flip),
            None => format!("tile {}", tile.tile),
        }
    }

    /// A tile of a rule map's own tilesets, by the map's name for it when the run binds its
    /// tileset to one of the map's; `None` for an `Ignore` tile, which writes nothing.
    fn rule_map_tile(&self, rule_map: &Map, cell: Cell) -> Option<String> {
        let slot = cell.slot()?;
        let reference = rule_map.tilesets.get(slot.index())?;
        if let Some(match_type) = reference
            .tileset
            .tile(cell.tile())
            .and_then(|data| data.properties.get("MatchType"))
        {
            return match &match_type.value {
                bt_core::Value::String(kind) if kind == "Ignore" => None,
                bt_core::Value::String(kind) if kind == "Empty" => Some("empty".to_owned()),
                value => Some(format!("a {} tile", crate::words::value_text(value))),
            };
        }
        Some(self.tile_of(reference, cell.tile(), cell.flip()))
    }

    /// A tile of a tileset the rules use. A run binds the tileset to the map's when it is the
    /// same (the same file), or, embedded in a rule map, like one of the map's; then the tile
    /// reads by the map's name for it. A tileset of its own file like the map's is a copy whose
    /// tiles never match the map's, and the words say so.
    fn tile_of(
        &self,
        reference: &bt_core::TilesetRef,
        id: bt_core::TileId,
        flip: bt_core::Flip,
    ) -> String {
        let flipped = if flip.is_empty() {
            String::new()
        } else {
            format!(" (flipped {})", flips_text(flip))
        };
        let similar = || {
            self.map
                .tilesets
                .iter()
                .position(|r| reference.tileset.is_similar_to(&r.tileset))
        };
        let same = self
            .map
            .tilesets
            .iter()
            .position(|r| r.is_same_tileset_as(reference));
        let bound = same.or_else(|| {
            if reference.file.is_none() {
                similar()
            } else {
                None
            }
        });
        if let Some(slot) = bound.and_then(TilesetSlot::new) {
            return format!("{}{flipped}", self.names.tile_text(Cell::new(slot, id)));
        }
        let name = slug(&reference.tileset.name);
        let same_name = self
            .map
            .tilesets
            .iter()
            .any(|r| r.tileset.name == reference.tileset.name);
        let whose = if similar().is_some() {
            " (the rules' copy of the tileset)"
        } else if same_name {
            " (the rules' tileset of that name, not the map's)"
        } else {
            " (the rules' tileset)"
        };
        format!("{name}:{id}{whose}{flipped}")
    }

    /// A tile a condition names.
    fn match_cell(&self, cell: MatchCell) -> String {
        match cell.tile {
            None => "empty".to_owned(),
            Some(tile) => {
                let any_flip = if cell.mask.is_empty() {
                    " (any flip)"
                } else {
                    ""
                };
                format!("{}{any_flip}", self.table_tile(tile))
            }
        }
    }

    /// A condition that does not hold: `walls 10,9 must be platformv2-3t:2, has empty`.
    fn condition(&self, check: &ConditionCheck) -> String {
        let only_empty = matches!(check.tiles.as_slice(), [only] if only.is_empty());
        let wanted = if only_empty {
            match check.condition {
                Condition::AnyOf => "must be empty".to_owned(),
                Condition::NoneOf => "must have a tile".to_owned(),
            }
        } else {
            let tiles: Vec<String> = check.tiles.iter().map(|t| self.match_cell(*t)).collect();
            let shown = crate::words::capped_list(&tiles, TILES_NAMED).replace(", ", " or ");
            match check.condition {
                Condition::AnyOf => format!("must be {shown}"),
                Condition::NoneOf => format!("must not be {shown}"),
            }
        };
        let earlier = if check.found == check.before {
            String::new()
        } else {
            format!(
                " (changed earlier in the run, from {})",
                self.run_cell(check.before)
            )
        };
        let missing = if check.layer_present {
            ""
        } else {
            " (the map has no tile layer of that name: it reads as empty)"
        };
        format!(
            "{} {} {wanted}, has {}{earlier}{missing}",
            check.layer,
            cell_text(check.at),
            self.run_cell(check.found)
        )
    }
}

/// ` (flipped h)`, or nothing.
fn flipped_text(cell: Cell) -> String {
    let flips = flips_text(cell.flip());
    if flips.is_empty() {
        String::new()
    } else {
        format!(" (flipped {flips})")
    }
}

impl Outcome for ExplainOutcome {
    const SCHEMA: &'static str = "bt.explain/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let counts = format!(
            "{}: {}, {}; seed {}",
            self.rules,
            plural(self.rule_maps, "rule map", "rule maps"),
            plural(self.rule_count, "rule", "rules"),
            self.seed
        );
        let heading = if self.automap_layers {
            format!(
                "cell {}: what the map's AutoMap layers do here ({counts}); each run is explained on what it reads, in the map's layer names",
                cell_text(self.cell)
            )
        } else {
            format!(
                "cell {}: what a run of the rules does here on the map as it is now ({counts})",
                cell_text(self.cell)
            )
        };
        let mut top = Section::headed(heading);
        top.add_lines(
            self.warnings
                .iter()
                .map(|w| Line::new(w.kind, w.text.clone())),
        );
        report.add_section(top);
        for run in &self.runs {
            report.add_section(run_section(run, self.all));
        }
        report
    }
}

/// A run's lines.
fn run_section(run: &ExplainedRun, all: bool) -> Section {
    let heading = match &run.run {
        Some(heading) => format!("{heading}:"),
        None if run.bindings.is_empty() => "the rules find their layers by name:".to_owned(),
        None => format!("with {}:", run.bindings.join(", ")),
    };
    let mut section = Section::headed(heading);
    for problem in &run.problems {
        section.add_line(Line::warning(format!("the run did not start: {problem}")));
    }
    if let Some(size) = run.outside {
        section.add_line(Line::note(format!(
            "the cell is outside the map, which is {}x{} cells",
            size.x, size.y
        )));
    }
    for unnamed in &run.unnamed {
        section.add_line(Line::fact(unnamed.clone()));
    }
    if !run.missing.is_empty() {
        let names: Vec<&str> = run.missing.iter().map(|m| m.name.as_str()).collect();
        let (layers, read) = if names.len() == 1 {
            ("layer", "it reads")
        } else {
            ("layers", "they read")
        };
        let mut line = Line::warning(format!(
            "the rules read the {layers} {}, which the map doesn't have: {read} as empty (bt explain MAP X,Y RULES {}=L says which layer {} for)",
            and_list(&names),
            names.first().copied().unwrap_or("NAME"),
            if names.len() == 1 {
                "it stands"
            } else {
                "each stands"
            }
        ));
        for missing in &run.missing {
            if let Some(near) = &missing.near {
                line = line.with_detail(Line::fact(format!(
                    "the map has a layer {near}, not {} (names must match exactly)",
                    missing.name
                )));
            }
        }
        section.add_line(line);
    }
    for layer in &run.layers {
        section.add_line(Line::fact(layer.clone()));
    }
    if run.changes.is_empty() {
        section.add_line(Line::fact("no rule changes this cell"));
    } else {
        section.add_line(
            Line::fact("changes, in order:")
                .with_details(run.changes.iter().map(|c| Line::fact(c.clone()))),
        );
    }
    if !run.matched.is_empty() {
        section.add_line(
            Line::fact("matched, changed nothing here:")
                .with_details(run.matched.iter().map(try_line)),
        );
    }
    if !run.close.is_empty() {
        let listed = if all { run.close.len() } else { CLOSE_LISTED };
        let mut line =
            Line::fact("came close (a condition of theirs about particular tiles holds):")
                .with_details(run.close.iter().take(listed).map(try_line));
        if run.close.len() > listed {
            line = line.with_detail(Line::fact(format!(
                "… and {} more (--all lists them)",
                run.close.len() - listed
            )));
        }
        section.add_line(line);
    }
    if !run.others.is_empty() {
        let count = run.others.len();
        if all {
            section.add_line(
                Line::fact(format!(
                    "the other rules that write this cell from some position ({}):",
                    run.others_reason
                ))
                .with_details(run.others.iter().map(try_line)),
            );
        } else {
            section.add_line(Line::fact(format!(
                "{} {} this cell from some position; {} (--all lists them)",
                plural(count, "other rule", "other rules"),
                if count == 1 { "writes" } else { "write" },
                run.others_reason
            )));
        }
    }
    section
}

/// A rule tried, with what it would write and its failing conditions under it.
fn try_line(rule: &RuleTry) -> Line {
    let mut line = Line::fact(format!(
        "{} at {}: {}",
        rule.rule,
        cell_text(rule.at),
        rule.outcome
    ));
    if !rule.would_write.is_empty() {
        line = line.with_detail(Line::fact(format!(
            "it would write {} here",
            rule.would_write.join(" and ")
        )));
    }
    if rule.input_sets > 1 && !rule.failing.is_empty() {
        line = line.with_detail(Line::fact(format!(
            "it has {} input sets, and one is enough; the closest:",
            rule.input_sets
        )));
    }
    line = line.with_details(rule.failing.iter().map(|f| Line::fact(f.clone())));
    if rule.outside_map > 0 {
        line = line.with_detail(Line::fact(format!(
            "and {} outside the map, where every cell reads as empty",
            plural(rule.outside_map, "condition", "conditions")
        )));
    }
    line
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, Tileset};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::report::{from_json, to_json};

    /// Rules over `tiles`: one rule map whose rule 1 turns tile 1 on `set` into 2, rule 2 turns
    /// tile 2 into 3, and rule 3 needs tile 4 with tile 5 to its right to write 6; matched in
    /// order, so each rule sees what the one before wrote.
    fn rules_over(tiles: &Arc<Tileset>) -> RuleSet {
        let mut rule_map = MapBuilder::new(UVec2::splat(16), UVec2::new(8, 1))
            .tileset(Arc::clone(tiles))
            .tile_layer("input_set", "1 . 2 . 4 5 . .")
            .tile_layer("output_set", "2 . 3 . 6 . . .")
            .build()
            .unwrap();
        rule_map.properties.insert(
            "MatchInOrder".into(),
            bt_core::Property::new(bt_core::Value::Bool(true)),
        );
        RuleSet::builder()
            .rule_map("/rules/rules.tmx", Arc::new(rule_map))
            .build()
    }

    fn map_of(tiles: &Arc<Tileset>, layer: &str, grid: &str) -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(Arc::clone(tiles))
            .tile_layer(layer, grid)
            .build()
            .unwrap()
    }

    #[test]
    fn changes_in_order_and_the_rules_that_came_close() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        let rules = rules_over(&tiles);
        let map = map_of(&tiles, "set", "1 4 .\n. . .");
        let explained = ExplainOutcome::from_request(
            &ExplainRequest::new(&map, &rules, IVec2::new(0, 0)).rules_file("rules.txt"),
        )
        .unwrap();
        let run = &explained.runs[0];
        assert_eq!(run.layers, ["set: ground:1 → ground:3"]);
        assert_eq!(
            run.changes,
            [
                "1. set ← ground:2 by rule 1 of rules.tmx (its cells at 0,0 1x1), matched at 0,0",
                "2. set ← ground:3 by rule 2 of rules.tmx (its cells at 2,0 1x1), matched at 0,0"
            ]
        );
        assert!(run.matched.is_empty());
        // Rule 3 needs 4 then 5: at 1,0 its tile 4 holds and 5 at 2,0 doesn't.
        let at_four = ExplainOutcome::from_request(
            &ExplainRequest::new(&map, &rules, IVec2::new(1, 0)).rules_file("rules.txt"),
        )
        .unwrap();
        let close = &at_four.runs[0].close;
        assert_eq!(close.len(), 1, "{at_four:?}");
        assert_eq!(close[0].rule, "rule 3 of rules.tmx (its cells at 4,0 2x1)");
        assert_eq!(close[0].would_write, ["set ground:6"]);
        assert_eq!(close[0].failing, ["set 2,0 must be ground:5, has empty"]);
        assert_eq!(
            at_four.report().text(),
            "cell 1,0: what a run of the rules does here on the map as it is now (rules.txt: 1 rule map, 3 rules; seed 1)\n\
             the rules find their layers by name:\n  \
             set: ground:4, unchanged\n  \
             no rule changes this cell\n  \
             came close (a condition of theirs about particular tiles holds):\n    \
             rule 3 of rules.tmx (its cells at 4,0 2x1) at 1,0: 1 of its 2 conditions doesn't hold\n      \
             it would write set ground:6 here\n      \
             set 2,0 must be ground:5, has empty\n  \
             2 other rules write this cell from some position; nothing here is what they look for (--all lists them)\n"
        );
        assert_eq!(
            from_json::<ExplainOutcome>(&to_json(&at_four).unwrap()).unwrap(),
            at_four
        );
    }

    #[test]
    fn a_layer_the_map_lacks_reads_as_empty_and_bindings_name_it() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        let rules = rules_over(&tiles);
        let map = map_of(&tiles, "Set", "1 . .\n. . .");
        let lacking = ExplainOutcome::from_request(
            &ExplainRequest::new(&map, &rules, IVec2::new(0, 0)).rules_file("rules.txt"),
        )
        .unwrap();
        assert_eq!(
            lacking.runs[0].missing,
            [MissingLayer {
                name: "set".into(),
                near: Some("Set".into())
            }]
        );
        let text = lacking.report().text();
        assert!(
            text.contains(
                "  warning: the rules read the layer set, which the map doesn't have: it reads as empty (bt explain MAP X,Y RULES set=L says which layer it stands for)\n    \
                 the map has a layer Set, not set (names must match exactly)\n"
            ),
            "{text}"
        );
        // Bound to the map's layer, the rules read it, in the map's name.
        let bound = ExplainOutcome::from_request(
            &ExplainRequest::new(&map, &rules, IVec2::new(0, 0))
                .rules_file("rules.txt")
                .bindings(LayerNames::builder().tiles("set", LayerId(1)).build()),
        )
        .unwrap();
        assert!(bound.runs[0].missing.is_empty());
        assert_eq!(bound.runs[0].layers, ["Set: ground:1 → ground:3"]);
        assert!(bound.report().text().contains("with set=Set:\n"));
        assert!(matches!(
            ExplainOutcome::from_request(
                &ExplainRequest::new(&map, &rules, IVec2::ZERO)
                    .rules_file("rules.txt")
                    .run(1)
            ),
            Err(Error::RunWithoutLayers { run: 1 })
        ));
    }

    #[test]
    fn automap_layers_are_explained_run_by_run_on_what_they_read() {
        let tiles = Atlas::builder()
            .name("ground")
            .build_embedded()
            .unwrap()
            .tileset;
        let rules = rules_over(&tiles);
        let painted = map_of(&tiles, "plat1", "1 . .\n. . .");
        let setup = Setup {
            rules: "/rules/rules.txt".into(),
            runs: vec![LayerNames::temporary().rebound("set", false, Some(LayerId(1)))],
            seed: 5,
        };
        let request = ExplainRequest::new(&painted, &rules, IVec2::ZERO)
            .rules_file("rules.txt")
            .automap_layers(&painted, &setup);
        let explained = ExplainOutcome::from_request(&request).unwrap();
        assert!(explained.automap_layers);
        assert_eq!(explained.seed, 5);
        assert_eq!(
            explained.runs[0].run.as_deref(),
            Some("run 1 of 1 (set=plat1)")
        );
        assert_eq!(explained.runs[0].layers, ["plat1: ground:1 → ground:3"]);
        assert!(matches!(
            ExplainOutcome::from_request(&request.run(2)),
            Err(Error::NoSuchRun { run: 2, runs: 1 })
        ));
    }

    #[test]
    fn tileds_sewers_name_the_tiles_no_rule_names() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut loader = bt_tmx::Loader::default();
        let path = examples.join("sewer_automap/sewers.tmx");
        let mut map = loader.load_map(&path).unwrap().value;
        let rules = bt_automap::files::load(
            &mut loader,
            &examples.join("sewer_automap/rules.txt"),
            Some(&path),
        )
        .unwrap();
        // A tile of the rules' tileset that no rule names, painted on the layer they read.
        let slot = map
            .tilesets
            .iter()
            .position(|t| t.tileset.name == "rules_sewers")
            .unwrap();
        let tile = Cell::new(TilesetSlot::new(slot).unwrap(), bt_core::TileId(3));
        let set = map.layers.iter_mut().find(|l| l.name == "set").unwrap();
        set.as_tile_layer_mut()
            .unwrap()
            .set_cell(IVec2::new(27, 23), tile)
            .unwrap();
        let explained = ExplainOutcome::from_request(
            &ExplainRequest::new(&map, &rules, IVec2::new(27, 23)).rules_file("rules.txt"),
        )
        .unwrap();
        let unnamed = &explained.runs[0].unnamed;
        assert_eq!(unnamed.len(), 1, "{explained:?}");
        assert!(
            unnamed[0].starts_with(
                "set has rules-sewers:3, which no rule's conditions name (they name rules-sewers:"
            ),
            "{}",
            unnamed[0]
        );
        let text = explained.report().text();
        assert!(text.contains("set has rules-sewers:3"), "{text}");
    }

    #[test]
    fn rules_are_grouped_by_what_came_of_them_each_once_at_its_best() {
        let rule = |index: usize| RuleRef {
            rule_map: 0,
            rule: index,
        };
        let check = |holds: bool, tile: Option<u32>| ConditionCheck {
            input_set: 0,
            layer: "set".into(),
            layer_present: true,
            at: IVec2::ZERO,
            condition: Condition::AnyOf,
            tiles: vec![MatchCell {
                tile: tile.map(|id| bt_automap::TableTile {
                    tileset: 0,
                    tile: bt_core::TileId(id),
                    flip: bt_core::Flip::empty(),
                }),
                mask: bt_core::Flip::all(),
            }],
            found: Cell::EMPTY,
            before: Cell::EMPTY,
            holds,
        };
        let attempt = |index: usize, outcome: AttemptOutcome, matched: bool, checks| RuleAttempt {
            rule: rule(index),
            position: IVec2::new(i32::try_from(index).unwrap(), 0),
            outcome,
            matched,
            checks,
        };
        let applied = AttemptOutcome::Applied {
            alternative: None,
            locked: Vec::new(),
        };
        let explanation = Explanation {
            cell: IVec2::ZERO,
            layers: Vec::new(),
            writes: vec![bt_automap::CellWrite {
                layer: "set".into(),
                before: Cell::EMPTY,
                after: Cell::EMPTY,
                by: Writer::Rule {
                    rule: rule(0),
                    position: IVec2::ZERO,
                },
            }],
            attempts: vec![
                // Rule 0 wrote the cell; tried elsewhere too.
                attempt(0, applied, true, Vec::new()),
                attempt(
                    0,
                    AttemptOutcome::NoMatch,
                    false,
                    vec![check(false, Some(1))],
                ),
                // Rule 1 matched but its Probability left it out.
                attempt(1, AttemptOutcome::SkippedByProbability, true, Vec::new()),
                // Rule 2: a tile of its holds and another fails.
                attempt(
                    2,
                    AttemptOutcome::NoMatch,
                    false,
                    vec![check(true, Some(3)), check(false, Some(4))],
                ),
                // Rule 3 needs a tile where the layer is empty.
                attempt(
                    3,
                    AttemptOutcome::NoMatch,
                    false,
                    vec![check(false, Some(5))],
                ),
            ],
            applied: vec![vec![1, 0, 0, 0]],
            unnamed: Vec::new(),
            added_tilesets: Vec::new(),
            problems: Vec::new(),
        };
        let groups = Groups::of(&explanation);
        let rules_of = |list: &[&RuleAttempt]| list.iter().map(|a| a.rule.rule).collect::<Vec<_>>();
        assert_eq!(rules_of(&groups.matched), [1]);
        assert_eq!(rules_of(&groups.close), [2]);
        assert_eq!(rules_of(&groups.others), [3]);
        assert_eq!(
            others_reason(&groups.others),
            "they need tiles on set, which is empty here"
        );
    }
}
