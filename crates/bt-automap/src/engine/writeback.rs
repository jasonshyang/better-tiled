//! The end of a run, as Tiled's `AutoMapperWrapper` ends it: the cells written that differ from
//! the map are set (only those, so chunks nothing changed stay shared with undo snapshots),
//! properties merged, the new layers that hold something added on top in the order they were
//! declared, the placed objects given ids and added, the objects `DeleteTiles` removed taken out,
//! and the tilesets whose tiles were placed added to the map. A region run keeps only the cells
//! of the area it recomputes.

use std::sync::Arc;

use bt_core::{
    Cell, Layer, LayerId, LayerKind, Map, ObjectLayer, TileLayer, TilesetRef, TilesetSlot,
};
use bt_utils::IRect;
use glam::{IVec2, UVec2};

use crate::bx::Bx;
use crate::engine::RemovedObject;
use crate::engine::objects::{PlacedGroup, assign_ids};
use crate::engine::targets::{Created, TargetOrigin, Targets};
use crate::engine::{Run, TilesetBinding};
use crate::observer::Observer;
use crate::problem::Problem;
use crate::rules::{LockedOutput, Report, RuleSet};

/// What a run leaves to write back.
pub(crate) struct RunResult {
    tilesets: TilesetBinding,
    targets: Targets,
    placed: Vec<PlacedGroup>,
    removed: Vec<RemovedObject>,
    locked: Vec<LockedOutput>,
    matches: Vec<usize>,
    problems: Vec<Problem>,
    /// The cells a region run keeps; `None` keeps every cell.
    exact: Option<Bx>,
}

impl RunResult {
    /// What `run` leaves.
    pub(crate) fn from_run<O: Observer>(run: Run<'_, O>) -> RunResult {
        let exact = run.exact_area();
        RunResult {
            tilesets: run.tilesets,
            targets: run.targets,
            placed: run.placed,
            removed: run.removed,
            locked: run.locked,
            matches: run.matches,
            problems: run.problems,
            exact,
        }
    }

    /// Whether the run keeps what it wrote at `p`.
    fn keeps(&self, p: IVec2) -> bool {
        self.exact.is_none_or(|exact| exact.contains(p))
    }

    /// The cell at `p` of the run's tile layer for `name`, as the run left it; empty when the
    /// run has no such layer. Cells are in the run's numbering of tilesets.
    pub(crate) fn cell(&self, name: &str, p: IVec2) -> Cell {
        self.targets
            .tile_named(name)
            .map_or(Cell::EMPTY, |target| target.grid.cell(p))
    }

    /// The map layer the run's tile layer for `name` is, if it is one of the map's.
    pub(crate) fn map_layer(&self, name: &str) -> Option<LayerId> {
        match self.targets.tile_named(name)?.origin {
            TargetOrigin::Existing(id) => Some(id),
            TargetOrigin::New | TargetOrigin::Temporary => None,
        }
    }

    /// How the run numbered the rule set's tilesets.
    pub(crate) fn tilesets(&self) -> &TilesetBinding {
        &self.tilesets
    }
}

/// The slots of the tilesets the run added, by the slots the run gave them.
struct SlotRemap {
    /// How many tilesets the map had.
    first: usize,
    /// For each slot the run gave a tileset it would add, the slot it has in the map.
    added: Vec<Option<TilesetSlot>>,
}

impl SlotRemap {
    fn cell(&self, cell: Cell) -> Cell {
        match cell.slot() {
            Some(slot) if slot.index() >= self.first => {
                match self.added.get(slot.index() - self.first).copied().flatten() {
                    Some(new) => Cell::new(new, cell.tile()).with_flip(cell.flip()),
                    None => cell,
                }
            }
            _ => cell,
        }
    }
}

/// Adds to `map` the tilesets whose tiles the run placed, in the order the rule set has them.
fn add_tilesets(
    set: &RuleSet,
    map: &mut Map,
    result: &RunResult,
    report: &mut Report,
) -> SlotRemap {
    let first = map.tilesets.len();
    let mut used = vec![false; result.tilesets.added.len()];
    let mut mark = |cell: Cell| {
        if let Some(slot) = cell.slot()
            && let Some(rank) = slot.index().checked_sub(first)
            && let Some(flag) = used.get_mut(rank)
        {
            *flag = true;
        }
    };
    for target in &result.targets.tiles {
        match target.origin {
            TargetOrigin::Existing(_) => {
                for placed in target.grid.written() {
                    if result.keeps(placed.at) {
                        mark(placed.cell);
                    }
                }
            }
            TargetOrigin::New => target.grid.for_each_cell(|p, cell| {
                if result.keeps(p) {
                    mark(cell);
                }
            }),
            TargetOrigin::Temporary => {}
        }
    }
    for group in &result.placed {
        for placed in &group.objects {
            let temporary = result
                .targets
                .objects
                .get(placed.target)
                .is_none_or(|t| t.origin == TargetOrigin::Temporary);
            if !temporary && let Some(cell) = placed.object.tile {
                mark(cell);
            }
        }
    }
    let mut added = Vec::with_capacity(used.len());
    for (&entry, &is_used) in result.tilesets.added.iter().zip(&used) {
        if !is_used {
            added.push(None);
            continue;
        }
        let Some(table_entry) = set.table.get(entry) else {
            added.push(None);
            continue;
        };
        let slot = TilesetSlot::new(map.tilesets.len());
        if let Some(slot) = slot {
            map.tilesets.push(TilesetRef {
                file: table_entry.reference.file.clone(),
                first_gid: None,
                tileset: Arc::clone(&table_entry.reference.tileset),
            });
            report.tilesets_added.push(slot);
        }
        added.push(slot);
    }
    SlotRemap { first, added }
}

fn include(changed: &mut Option<IRect>, p: IVec2) {
    let cell = IRect {
        x: p.x,
        y: p.y,
        w: 1,
        h: 1,
    };
    *changed = Some(changed.map_or(cell, |c| c.union(cell)));
}

/// Writes what the run did into `map` and fills `report`.
pub(crate) fn write_back(set: &RuleSet, map: &mut Map, mut result: RunResult, report: &mut Report) {
    report.problems.append(&mut result.problems);
    report.rule_map_matches = result.matches.clone();
    report.matches = result.matches.iter().sum();
    report.locked_outputs = std::mem::take(&mut result.locked);
    for locked in &report.locked_outputs {
        report.problems.push(Problem::new(
            crate::problem::ProblemKind::LockedOutput,
            format!(
                "layer “{}” is locked, so rule map {} wrote nothing there {} time{} \
                 (IgnoreLock lets rules write to it)",
                locked.name,
                locked.rule_map + 1,
                locked.matches,
                if locked.matches == 1 { "" } else { "s" }
            ),
        ));
    }
    let remap = add_tilesets(set, map, &result, report);
    let mut changed: Option<IRect> = None;

    // Existing tile layers: the cells written that differ.
    for target in &result.targets.tiles {
        let TargetOrigin::Existing(id) = target.origin else {
            continue;
        };
        let properties = target.properties.clone();
        let Some(layer) = map.layer_mut(id) else {
            continue;
        };
        if let Some(properties) = properties
            && properties != layer.properties
        {
            layer.properties = properties;
        }
        let Some(tiles) = layer.as_tile_layer_mut() else {
            continue;
        };
        let mut written = target.grid.written();
        written.retain(|placed| result.keeps(placed.at));
        written.sort_unstable_by_key(|placed| (placed.at.y, placed.at.x));
        for placed in written {
            let cell = remap.cell(placed.cell);
            if tiles.cell(placed.at) != cell && tiles.set_cell(placed.at, cell).unwrap_or(false) {
                include(&mut changed, placed.at);
            }
        }
        tiles.prune_empty_chunks();
    }

    // New layers that hold something, on top, in the order they were declared.
    let size = UVec2::new(map.width, map.height);
    let mut new_object_layers: Vec<Option<LayerId>> = vec![None; result.targets.objects.len()];
    for created in result.targets.created.clone() {
        match created {
            Created::Tiles(index) => {
                let Some(target) = result.targets.tiles.get(index) else {
                    continue;
                };
                let mut tiles = if map.infinite {
                    TileLayer::infinite(size)
                } else {
                    TileLayer::finite(size)
                };
                target.grid.for_each_cell(|p, cell| {
                    if result.keeps(p) && tiles.set_cell(p, remap.cell(cell)).unwrap_or(false) {
                        include(&mut changed, p);
                    }
                });
                if tiles.chunks().next().is_none() {
                    continue;
                }
                let id = LayerId(map.next_layer_id);
                map.next_layer_id = map.next_layer_id.saturating_add(1);
                let mut layer = Layer::new(id, target.name.clone(), LayerKind::Tile(tiles));
                if let Some(properties) = &target.properties {
                    layer.properties = properties.clone();
                }
                map.layers.push(layer);
                report.created_layers.push(id);
            }
            Created::Objects(index) => {
                let Some(target) = result.targets.objects.get(index) else {
                    continue;
                };
                if !target.placed {
                    continue;
                }
                let id = LayerId(map.next_layer_id);
                map.next_layer_id = map.next_layer_id.saturating_add(1);
                let mut layer = Layer::new(
                    id,
                    target.name.clone(),
                    LayerKind::Object(ObjectLayer::default()),
                );
                if let Some(properties) = &target.properties {
                    layer.properties = properties.clone();
                }
                map.layers.push(layer);
                report.created_layers.push(id);
                if let Some(slot) = new_object_layers.get_mut(index) {
                    *slot = Some(id);
                }
            }
        }
    }

    // Properties of existing object layers.
    for target in &result.targets.objects {
        if let (TargetOrigin::Existing(id), Some(properties)) = (target.origin, &target.properties)
            && let Some(layer) = map.layer_mut(id)
            && layer.properties != *properties
        {
            layer.properties = properties.clone();
        }
    }

    // Placed objects, group by group, with new ids.
    let mut next_object_id = map.next_object_id;
    for mut group in std::mem::take(&mut result.placed) {
        group.objects.retain(|placed| {
            result
                .targets
                .objects
                .get(placed.target)
                .is_some_and(|t| t.origin != TargetOrigin::Temporary)
        });
        if group.objects.is_empty() {
            continue;
        }
        report
            .objects_added
            .extend(assign_ids(&mut group, &mut next_object_id));
        for placed in group.objects {
            let layer_id = match result.targets.objects.get(placed.target).map(|t| t.origin) {
                Some(TargetOrigin::Existing(id)) => Some(id),
                Some(TargetOrigin::New) => new_object_layers.get(placed.target).copied().flatten(),
                _ => None,
            };
            let Some(objects) = layer_id
                .and_then(|id| map.layer_mut(id))
                .and_then(Layer::as_object_layer_mut)
            else {
                continue;
            };
            let mut object = placed.object;
            object.tile = object.tile.map(|cell| remap.cell(cell));
            Arc::make_mut(&mut objects.objects).push(object);
        }
    }
    map.next_object_id = next_object_id;

    // Objects DeleteTiles removed.
    for removed in &result.removed {
        if let Some(objects) = map
            .layer_mut(removed.layer)
            .and_then(Layer::as_object_layer_mut)
            && objects.objects.iter().any(|o| o.id == removed.object)
        {
            Arc::make_mut(&mut objects.objects).retain(|o| o.id != removed.object);
            report.objects_removed.push(removed.object);
        }
    }
    report.changed = changed;
}
