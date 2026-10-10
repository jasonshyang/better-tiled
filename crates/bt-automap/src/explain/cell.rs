//! What the rules do at one cell (§8, `explain_cell`): a full run of the rules on the map as it
//! is, recorded without changing it. For every layer the rules write, the cell before and after;
//! the writes to the cell in order; every rule whose output covers the cell, at each position
//! from which it does, with what came of it and every condition checked there; how often each
//! rule applied in the whole run; and the tiles at the cell that no condition names, which the
//! rules see only as "a tile".

use std::collections::HashMap;

use bt_core::{Cell, Flip, Layer, Map};
use glam::IVec2;

use crate::engine::{self, LayerNames, RunRequest, RunResult};
use crate::explain::attempt::{AttemptOutcome, RuleAttempt};
use crate::explain::rename::LayerRenaming;
use crate::explain::tracer::{Tracer, Watch};
use crate::observer::Writer;
use crate::problem::Problem;
use crate::rule_ref::RuleRef;
use crate::rulemap::{MatchCell, TableTile};
use crate::rules::{Report, RuleSet};

/// A layer the rules write, at the explained cell.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerAtCell {
    /// The layer, by the name the rules use.
    pub layer: String,
    /// Whether the map has it (the run creates the others when it writes them).
    pub present: bool,
    /// The cell before the run.
    pub before: Cell,
    /// The cell after the run.
    pub after: Cell,
}

/// A write to the explained cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellWrite {
    /// The layer written, by the name the rules use.
    pub layer: String,
    /// What the cell held.
    pub before: Cell,
    /// What it held after.
    pub after: Cell,
    /// What wrote it.
    pub by: Writer,
}

/// A tile at the explained cell that no condition of the rules names, on a layer they read and
/// no rule map writes: the rules see it only as "a tile" (or "not one of mine").
#[derive(Clone, Debug, PartialEq)]
pub struct UnnamedTile {
    /// The layer, by the name the rules use.
    pub layer: String,
    /// The tile, in the run's numbering.
    pub tile: Cell,
    /// The tiles the rules' conditions name on that layer, in the rule set's numbering and
    /// without flips.
    pub named: Vec<TableTile>,
}

/// What the rules do at one cell ([`RuleSet::explain_cell`]). Cells of the map are in the run's
/// numbering of tilesets: the map's slots, then those of `added_tilesets`; rule tiles are in the
/// rule set's ([`RuleSet::tileset`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Explanation {
    /// The cell.
    pub cell: IVec2,
    /// Every layer the rules write, with the cell before and after the run.
    pub layers: Vec<LayerAtCell>,
    /// The writes to the cell, in order: on each layer the last one stays.
    pub writes: Vec<CellWrite>,
    /// Every rule whose output covers the cell, at each position from which it does, in the
    /// order the run tries them: tried ones with their checks, and those the run does not try
    /// there (off its lattice, outside the map, disabled, or never matching).
    pub attempts: Vec<RuleAttempt>,
    /// How often each rule applied in the whole run, by rule map and rule.
    pub applied: Vec<Vec<usize>>,
    /// The tiles at the cell no condition names.
    pub unnamed: Vec<UnnamedTile>,
    /// The rule set's tilesets ([`RuleSet::tileset`]) the run numbers after the map's, in
    /// order: a cell of slot `map.tilesets.len() + i` is a tile of `added_tilesets[i]`.
    pub added_tilesets: Vec<usize>,
    /// Why the run did not start, when it did not (its layer names are refused).
    pub problems: Vec<Problem>,
}

impl Explanation {
    /// The explanation in the map's words: every name `names` binds to a layer of `map` becomes
    /// that layer's name (an explanation of a run with these layer names).
    pub fn with_layer_names(mut self, names: &LayerNames, map: &Map) -> Explanation {
        let renaming = LayerRenaming::new(names, map);
        for layer in &mut self.layers {
            renaming.rename(&mut layer.layer);
        }
        for write in &mut self.writes {
            renaming.rename(&mut write.layer);
        }
        for check in self.attempts.iter_mut().flat_map(|a| &mut a.checks) {
            renaming.rename(&mut check.layer);
        }
        for tile in &mut self.unnamed {
            renaming.rename(&mut tile.layer);
        }
        self
    }
}

/// A full run asking what `request` asks but the area.
pub(crate) fn full_run(request: &RunRequest) -> RunRequest {
    RunRequest::builder(request.seed())
        .layer_names(request.layer_names().clone())
        .build()
}

/// The positions from which a write at `offset` lands on `cell`: one, or on a finite map whose
/// rule map wraps (`WrapBorder`), every one around the map whose input box (of size `extent`)
/// reaches the map grown by `radius`, as a run tries them.
fn positions_landing(map: &Map, cell: IVec2, offset: IVec2, wrap: Option<WrapReach>) -> Vec<IVec2> {
    let straight = cell - offset;
    let Some(reach) = wrap.filter(|_| !map.infinite) else {
        return vec![straight];
    };
    let size = IVec2::new(
        i32::try_from(map.width).unwrap_or(i32::MAX),
        i32::try_from(map.height).unwrap_or(i32::MAX),
    );
    if size.cmple(IVec2::ZERO).any() {
        return vec![straight];
    }
    let low = -reach.extent - IVec2::splat(reach.radius);
    let high = size - IVec2::ONE + IVec2::splat(reach.radius);
    let turns = (reach.extent + IVec2::splat(reach.radius.max(0))) / size + IVec2::splat(2);
    let mut positions = vec![straight];
    for j in -turns.y..=turns.y {
        for i in -turns.x..=turns.x {
            let p = straight + IVec2::new(i, j) * size;
            if (i, j) != (0, 0) && p.cmpge(low).all() && p.cmple(high).all() {
                positions.push(p);
            }
        }
    }
    positions
}

/// How far a wrapping rule map's rule reaches past a finite map: its input box's far corner
/// and the rule map's `AutomappingRadius`.
#[derive(Copy, Clone, Debug)]
struct WrapReach {
    extent: IVec2,
    radius: i32,
}

/// For every rule of a used rule map whose output writes `cell` from some position, those
/// positions, sorted by row then column.
fn positions_writing(set: &RuleSet, map: &Map, cell: IVec2) -> HashMap<RuleRef, Vec<IVec2>> {
    let mut watched = HashMap::new();
    for (rule_map_index, rule_map) in set.rule_maps().iter().enumerate() {
        if !rule_map.is_used() {
            continue;
        }
        for (rule_index, rule) in rule_map.rules.iter().enumerate() {
            let wrap = rule_map.options.wrap_border.then_some(WrapReach {
                extent: rule.input_box.max,
                radius: rule_map.options.radius,
            });
            let mut positions: Vec<IVec2> = rule
                .fixed
                .iter()
                .chain(rule.alternatives.iter().map(|a| &a.set))
                .flat_map(|output| &output.tiles)
                .flat_map(|tiles| &tiles.writes)
                .flat_map(|write| positions_landing(map, cell, write.offset, wrap))
                .collect();
            if positions.is_empty() {
                continue;
            }
            positions.sort_by_key(|p| (p.y, p.x));
            positions.dedup();
            watched.insert(
                RuleRef {
                    rule_map: rule_map_index,
                    rule: rule_index,
                },
                positions,
            );
        }
    }
    watched
}

/// The attempts of the watched rules the run never tries: disabled ones, and those that can
/// never match.
fn untried_attempts(set: &RuleSet, watched: &HashMap<RuleRef, Vec<IVec2>>) -> Vec<RuleAttempt> {
    let mut attempts = Vec::new();
    for (&rule, positions) in watched {
        let Some(compiled) = set
            .rule_maps()
            .get(rule.rule_map)
            .and_then(|r| r.rules.get(rule.rule))
        else {
            continue;
        };
        let outcome = if compiled.options.disabled {
            AttemptOutcome::Disabled
        } else if compiled.never_matches().is_some() {
            AttemptOutcome::CanNeverMatch
        } else {
            continue;
        };
        attempts.extend(positions.iter().map(|&position| RuleAttempt {
            rule,
            position,
            outcome: outcome.clone(),
            matched: false,
            checks: Vec::new(),
        }));
    }
    attempts
}

/// The tiles the rules' conditions name on each layer they read and no used rule map writes.
struct NamedTiles<'s> {
    layer: &'s str,
    tiles: Vec<TableTile>,
}

fn named_tiles(set: &RuleSet) -> Vec<NamedTiles<'_>> {
    let used = || set.rule_maps().iter().filter(|r| r.is_used());
    let written: Vec<&str> = used()
        .flat_map(|r| r.tile_outputs())
        .map(String::as_str)
        .collect();
    let mut named: Vec<NamedTiles<'_>> = Vec::new();
    for rule_map in used() {
        for rule in &rule_map.rules {
            for input in &rule.input_sets {
                for layer in &input.layers {
                    let Some(name) = rule_map.input_names.get(layer.target) else {
                        continue;
                    };
                    if written.contains(&name.as_str()) {
                        continue;
                    }
                    let entry = match named.iter().position(|n| n.layer == name) {
                        Some(i) => named.get_mut(i),
                        None => {
                            named.push(NamedTiles {
                                layer: name,
                                tiles: Vec::new(),
                            });
                            named.last_mut()
                        }
                    };
                    let Some(entry) = entry else {
                        continue;
                    };
                    for position in &layer.positions {
                        let cells = input.any_of(position).iter().chain(input.none_of(position));
                        for tile in cells.filter_map(|m| m.tile) {
                            let plain = TableTile {
                                flip: Flip::empty(),
                                ..tile
                            };
                            if !entry.tiles.contains(&plain) {
                                entry.tiles.push(plain);
                            }
                        }
                    }
                }
            }
        }
    }
    named
}

/// The tiles at `cell` of `map` that no condition names, on the layers the run read there.
fn unnamed_tiles(set: &RuleSet, map: &Map, cell: IVec2, result: &RunResult) -> Vec<UnnamedTile> {
    let binding = result.tilesets();
    let mut unnamed = Vec::new();
    for named in named_tiles(set) {
        let Some(tile) = result
            .map_layer(named.layer)
            .and_then(|id| map.layer(id))
            .and_then(Layer::as_tile_layer)
            .map(|tiles| tiles.cell(cell))
            .filter(|tile| !tile.is_empty())
        else {
            continue;
        };
        let key = binding.key_of(tile);
        let is_named = named.tiles.iter().any(|&t| {
            let bound = binding.bind_match(&MatchCell {
                tile: Some(t),
                mask: Flip::empty(),
            });
            bound.key == key && bound.tile == tile.tile().0
        });
        if !is_named {
            unnamed.push(UnnamedTile {
                layer: named.layer.to_owned(),
                tile,
                named: named.tiles,
            });
        }
    }
    unnamed
}

/// Every layer the rules write, at `cell`, before and after the run.
fn layers_at(set: &RuleSet, map: &Map, cell: IVec2, result: &RunResult) -> Vec<LayerAtCell> {
    let mut names: Vec<&str> = Vec::new();
    for rule_map in set.rule_maps().iter().filter(|r| r.is_used()) {
        for name in rule_map.tile_outputs() {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
    }
    names
        .into_iter()
        .map(|name| {
            let layer = result.map_layer(name);
            let before = layer
                .and_then(|id| map.layer(id))
                .and_then(Layer::as_tile_layer)
                .map_or(Cell::EMPTY, |tiles| tiles.cell(cell));
            LayerAtCell {
                layer: name.to_owned(),
                present: layer.is_some(),
                before,
                after: result.cell(name, cell),
            }
        })
        .collect()
}

impl RuleSet {
    /// What the rules do at `cell` when they run on `map` now with `seed` (a full run that
    /// leaves `map` as it is), finding layers by name as Tiled does.
    pub fn explain_cell(&self, map: &Map, cell: IVec2, seed: u64) -> Explanation {
        self.explain_cell_with(map, cell, &RunRequest::builder(seed).build())
    }

    /// [`RuleSet::explain_cell`] for a run with `request`'s seed and layer names (its area is
    /// ignored: an explanation is of a full run).
    pub fn explain_cell_with(&self, map: &Map, cell: IVec2, request: &RunRequest) -> Explanation {
        let request = full_run(request);
        let watched = positions_writing(self, map, cell);
        let mut attempts = untried_attempts(self, &watched);
        let mut tracer = Tracer::new(self, Watch::Positions(watched), Some(cell));
        let mut report = Report::default();
        let result = engine::trace(self, map, &request, &mut tracer, &mut report);
        let traced = tracer.finish();
        let mut tried = traced.attempts;
        tried.append(&mut attempts);
        // Stable: the run's order within each rule.
        tried.sort_by_key(|a| (a.rule.rule_map, a.rule.rule));
        let writes = traced
            .writes
            .into_iter()
            .map(|w| CellWrite {
                layer: w.layer,
                before: w.before,
                after: w.after,
                by: w.by,
            })
            .collect();
        let result = result.as_ref();
        Explanation {
            cell,
            layers: result.map_or_else(Vec::new, |r| layers_at(self, map, cell, r)),
            writes,
            attempts: tried,
            applied: traced.applied,
            unnamed: result.map_or_else(Vec::new, |r| unnamed_tiles(self, map, cell, r)),
            added_tilesets: result.map_or_else(Vec::new, |r| r.tilesets().added.clone()),
            problems: report.problems,
        }
    }
}
