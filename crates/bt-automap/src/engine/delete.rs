//! `DeleteTiles`, as Tiled's `autoMap` does it before matching: the cells of the region where any
//! of the rule map's input layers holds a tile are erased from every tile layer it writes, and
//! the objects meeting those cells are removed from the object layers it writes. Unlike Tiled,
//! locks are honoured here too (unless the rule map sets `IgnoreLock`). A region run erases
//! within its window and removes the objects meeting the erased cells of the area it
//! recomputes.

use bt_core::Cell;
use bt_utils::hash::CellHashSet;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::engine::targets::TargetOrigin;
use crate::engine::{RemovedObject, RuleMapRun, Run};
use crate::object_cells::objects_in_region;
use crate::observer::{Observer, Writer};

impl<O: Observer> Run<'_, O> {
    /// Erases the input cells of `run`'s region from its output layers.
    pub(crate) fn delete_tiles(&mut self, run: &RuleMapRun<'_>) {
        let mut inputs: CellHashSet<IVec2> = CellHashSet::default();
        let limit = run.region.bounding().map(Bx::rect);
        for grid in run.inputs.iter().flatten() {
            if let Some(target) = self.targets.tiles.get(*grid) {
                target.grid.for_each_cell_within(limit, |p, _| {
                    if run.region.contains(p) {
                        inputs.insert(p);
                    }
                });
            }
        }
        if inputs.is_empty() {
            return;
        }
        let mut erased: Vec<IVec2> = inputs.iter().copied().collect();
        erased.sort_unstable_by_key(|p| (p.y, p.x));
        let ignore_lock = run.rule_map.options.rule_defaults.ignore_lock;
        let by = Writer::DeleteTiles {
            rule_map: run.index,
        };
        for &grid in &run.tile_outputs {
            let Some(target) = self.targets.tiles.get(grid) else {
                continue;
            };
            if target.locked && !ignore_lock {
                if let TargetOrigin::Existing(id) = target.origin {
                    let name = target.name.clone();
                    self.record_locked(run.index, id, &name);
                }
                continue;
            }
            let filled: Vec<IVec2> = erased
                .iter()
                .copied()
                .filter(|&p| !target.grid.cell(p).is_empty())
                .collect();
            for p in filled {
                self.write_cell(grid, p, Cell::EMPTY, by, None, None);
            }
        }
        let exact = self.exact_area();
        let region = CellSet::from_cells(
            erased
                .into_iter()
                .filter(|&p| exact.is_none_or(|exact| exact.contains(p))),
        );
        for &target_index in &run.object_outputs {
            let Some(target) = self.targets.objects.get(target_index) else {
                continue;
            };
            let TargetOrigin::Existing(id) = target.origin else {
                continue;
            };
            if target.locked && !ignore_lock {
                let name = target.name.clone();
                self.record_locked(run.index, id, &name);
                continue;
            }
            let Some(objects) = self.map.layer(id).and_then(|l| l.as_object_layer()) else {
                continue;
            };
            for object in objects_in_region(self.map, &objects.objects, &region) {
                let removed = RemovedObject {
                    layer: id,
                    object: object.id,
                };
                if !self.removed.contains(&removed) {
                    self.removed.push(removed);
                }
            }
        }
    }
}
