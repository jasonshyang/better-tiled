//! Applying a match, as Tiled's `applyRule` and `copyMapRegion` do: the random output chosen
//! (by weight; one choice draws nothing), `NoOverlappingOutput` checked against the rule's own
//! footprint so far in this pass, then the fixed output and the chosen one copied: tiles and
//! erasures written (wrapped or dropped at a finite map's edge), the output layers' other
//! properties merged into their targets, objects placed. A locked target is left alone unless
//! the rule has `IgnoreLock`, and the report says so.

use std::collections::{BTreeSet, HashMap};

use bt_core::{Cell, LayerId, MapObject};
use glam::{IVec2, UVec2};

use crate::cell_set::CellSet;
use crate::engine::anchor::{Anchor, AnchorIndex, AnchorTile, MatchArea, ScanKey, on_lattice};
use crate::engine::objects::{PlacedGroup, PlacedObject, pixel_offset};
use crate::engine::targets::TargetOrigin;
use crate::engine::{BoundRule, RuleMapRun, Run};
use crate::observer::{Observer, Outcome, Write, Writer};
use crate::rng::{self, Purpose};
use crate::rule_ref::RuleRef;
use crate::rulemap::{OutputSet, RuleOptions};
use crate::rules::LockedOutput;

/// A layer a footprint is on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FootprintLayer {
    /// A tile layer of the run.
    Tiles(usize),
    /// An object layer of the run.
    Objects(usize),
}

/// What one rule wrote so far in a pass, per layer, for `NoOverlappingOutput`.
#[derive(Clone, Debug, Default)]
pub(crate) struct OverlapState {
    applied: HashMap<FootprintLayer, CellSet>,
}

/// The positions still to try for a rule matched in order, kept up to date as it writes its
/// anchor tiles.
pub(crate) struct Pending<'p> {
    /// The position being tried.
    pub(crate) current: ScanKey,
    /// The positions after it still to try.
    pub(crate) keys: BTreeSet<ScanKey>,
    /// The rule's anchors.
    pub(crate) anchors: &'p [Anchor],
    /// Where the rule may be tried.
    pub(crate) area: &'p MatchArea,
    /// The rule's options, for its lattice.
    pub(crate) options: RuleOptions,
}

impl Pending<'_> {
    /// Adds the positions where `tile`, written at `at` on `grid`, is the rule's anchor and
    /// which the scan has not passed.
    fn note_write(&mut self, grid: usize, tile: AnchorTile, at: IVec2) {
        for anchor in self.anchors {
            if anchor.grid != grid || !anchor.tiles.contains(&tile) {
                continue;
            }
            let p = at - anchor.offset;
            if on_lattice(p, &self.options)
                && let Some(key) = self.area.key(p)
                && key > self.current
            {
                self.keys.insert(key);
            }
        }
    }
}

/// Where a cell written at `p` lands on a layer of `size` (`None` on an infinite map): wrapped
/// with `WrapBorder`, else nowhere when outside.
fn landing(size: Option<UVec2>, p: IVec2, wrap: bool) -> Option<IVec2> {
    let Some(size) = size else {
        return Some(p);
    };
    let w = i32::try_from(size.x).ok().filter(|&w| w > 0)?;
    let h = i32::try_from(size.y).ok().filter(|&h| h > 0)?;
    if wrap {
        return Some(IVec2::new(p.x.rem_euclid(w), p.y.rem_euclid(h)));
    }
    (p.x >= 0 && p.y >= 0 && p.x < w && p.y < h).then_some(p)
}

impl<O: Observer> Run<'_, O> {
    /// Records that a lock kept rule map `rule_map`'s output out of `layer`.
    pub(crate) fn record_locked(&mut self, rule_map: usize, layer: LayerId, name: &str) {
        match self
            .locked
            .iter_mut()
            .find(|l| l.rule_map == rule_map && l.layer == layer)
        {
            Some(locked) => locked.matches += 1,
            None => self.locked.push(LockedOutput {
                rule_map,
                layer,
                name: name.to_owned(),
                matches: 1,
            }),
        }
    }

    /// Writes `cell` at `at` on tile target `grid`, telling the observer and keeping the anchor
    /// index and the pending positions up to date.
    pub(crate) fn write_cell(
        &mut self,
        grid: usize,
        at: IVec2,
        cell: Cell,
        by: Writer,
        index: Option<&mut AnchorIndex>,
        pending: Option<&mut Pending<'_>>,
    ) {
        let Some(target) = self.targets.tiles.get_mut(grid) else {
            return;
        };
        let before = target.grid.set_cell(at, cell);
        if O::ACTIVE {
            self.observer.write(&Write {
                layer: &target.name,
                at,
                before,
                after: cell,
                by,
            });
        }
        if index.is_none() && pending.is_none() {
            return;
        }
        let tile = AnchorTile {
            key: self.tilesets.key_of(cell),
            tile: cell.tile().0,
        };
        if let Some(index) = index {
            index.record_write(grid, tile, at);
        }
        if let Some(pending) = pending {
            pending.note_write(grid, tile, at);
        }
    }

    /// Applies `rule` at `p`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the pieces of a pass a match updates"
    )]
    pub(crate) fn apply_match(
        &mut self,
        run: &RuleMapRun<'_>,
        rule: &BoundRule<'_>,
        p: IVec2,
        overlaps: &mut OverlapState,
        tracked: &mut Option<CellSet>,
        mut index: Option<&mut AnchorIndex>,
        mut pending: Option<&mut Pending<'_>>,
    ) {
        let rule_ref = RuleRef {
            rule_map: run.index,
            rule: rule.index,
        };
        let compiled = rule.rule;
        let alternative = if compiled.picker.is_empty() {
            None
        } else {
            let mut generator = rng::generator(self.seed, rule_ref, p, Purpose::Output);
            compiled
                .picker
                .pick(&mut generator)
                .and_then(|&i| compiled.alternatives.get(i))
        };
        if compiled.options.no_overlapping_output {
            let mut footprints: HashMap<FootprintLayer, CellSet> = HashMap::new();
            for set in compiled.fixed.iter().chain(alternative.map(|a| &a.set)) {
                for output in &set.tiles {
                    if let Some(&grid) = run.tile_outputs.get(output.target) {
                        footprints
                            .entry(FootprintLayer::Tiles(grid))
                            .or_default()
                            .insert_set(&output.footprint.translated(p));
                    }
                }
                for output in &set.objects {
                    if let Some(&target) = run.object_outputs.get(output.target) {
                        footprints
                            .entry(FootprintLayer::Objects(target))
                            .or_default()
                            .insert_set(&output.footprint.translated(p));
                    }
                }
            }
            let overlapping = footprints.iter().any(|(layer, cells)| {
                overlaps
                    .applied
                    .get(layer)
                    .is_some_and(|done| done.intersects(cells))
            });
            if overlapping {
                if O::ACTIVE {
                    self.attempted(rule_ref, p, Outcome::Overlapping);
                }
                return;
            }
            for (layer, cells) in footprints {
                overlaps
                    .applied
                    .entry(layer)
                    .or_default()
                    .insert_set(&cells);
            }
        }
        let mut locked = Vec::new();
        for set in compiled.fixed.iter().chain(alternative.map(|a| &a.set)) {
            self.copy_output(
                run,
                rule,
                set,
                p,
                &mut locked,
                index.as_deref_mut(),
                pending.as_deref_mut(),
            );
        }
        if let Some(tracked) = tracked {
            tracked.insert_set(&compiled.output_cells.translated(p));
        }
        if let Some(count) = self.matches.get_mut(run.index) {
            *count += 1;
        }
        if O::ACTIVE {
            let outcome = Outcome::Applied {
                alternative: alternative.map(|a| a.set.index.as_str()),
                locked,
            };
            self.attempted(rule_ref, p, outcome);
        }
    }

    /// Copies one output set of `rule` to position `p`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the pieces of a pass a match updates"
    )]
    fn copy_output(
        &mut self,
        run: &RuleMapRun<'_>,
        rule: &BoundRule<'_>,
        set: &OutputSet,
        p: IVec2,
        locked: &mut Vec<LayerId>,
        mut index: Option<&mut AnchorIndex>,
        mut pending: Option<&mut Pending<'_>>,
    ) {
        let compiled = rule.rule;
        let by = Writer::Rule {
            rule: RuleRef {
                rule_map: run.index,
                rule: rule.index,
            },
            position: p,
        };
        for output in &set.tiles {
            let Some(&grid) = run.tile_outputs.get(output.target) else {
                continue;
            };
            let Some(target) = self.targets.tiles.get(grid) else {
                continue;
            };
            if target.locked && !compiled.options.ignore_lock {
                if let TargetOrigin::Existing(id) = target.origin {
                    let name = target.name.clone();
                    self.record_locked(run.index, id, &name);
                    locked.push(id);
                }
                continue;
            }
            let size = target.size;
            for write in &output.writes {
                let Some(at) = landing(size, p + write.offset, run.wrap) else {
                    continue;
                };
                let cell = match write.tile {
                    None => Cell::EMPTY,
                    Some(tile) => match self.tilesets.cell_of(tile) {
                        Some(cell) => cell,
                        None => continue,
                    },
                };
                self.write_cell(
                    grid,
                    at,
                    cell,
                    by,
                    index.as_deref_mut(),
                    pending.as_deref_mut(),
                );
            }
            self.merge_tile_properties(run, grid, output.layer);
        }
        if set.objects.is_empty() {
            return;
        }
        let offset = pixel_offset(self.map, compiled.output_origin, p - compiled.origin);
        let mut group = PlacedGroup::default();
        for output in &set.objects {
            let Some(&target_index) = run.object_outputs.get(output.target) else {
                continue;
            };
            let Some(target) = self.targets.objects.get(target_index) else {
                continue;
            };
            if target.locked && !compiled.options.ignore_lock {
                if let TargetOrigin::Existing(id) = target.origin {
                    let name = target.name.clone();
                    self.record_locked(run.index, id, &name);
                    locked.push(id);
                }
                continue;
            }
            for placed in &output.objects {
                let tile = match placed.tile {
                    None => None,
                    Some(tile) => match self.tilesets.cell_of(tile) {
                        Some(cell) => Some(cell),
                        None => continue,
                    },
                };
                group.objects.push(PlacedObject {
                    target: target_index,
                    object: MapObject {
                        pos: placed.object.pos + offset,
                        tile,
                        ..placed.object.clone()
                    },
                });
            }
            if let Some(target) = self.targets.objects.get_mut(target_index) {
                target.placed = true;
            }
            self.merge_object_properties(run, target_index, output.layer);
        }
        if !group.objects.is_empty() {
            self.placed.push(group);
        }
    }

    /// Merges an output layer's properties into tile target `grid`.
    fn merge_tile_properties(&mut self, run: &RuleMapRun<'_>, grid: usize, layer: usize) {
        let Some(output) = run.rule_map.output_layers.get(layer) else {
            return;
        };
        if output.properties.is_empty() {
            return;
        }
        let Some(target) = self.targets.tiles.get_mut(grid) else {
            return;
        };
        let current = target
            .properties
            .get_or_insert_with(|| match target.origin {
                TargetOrigin::Existing(id) => self
                    .map
                    .layer(id)
                    .map(|l| l.properties.clone())
                    .unwrap_or_default(),
                TargetOrigin::New | TargetOrigin::Temporary => bt_core::Properties::new(),
            });
        for (name, value) in &output.properties {
            current.insert(name.clone(), value.clone());
        }
    }

    /// Merges an output layer's properties into object target `target`.
    fn merge_object_properties(&mut self, run: &RuleMapRun<'_>, target: usize, layer: usize) {
        let Some(output) = run.rule_map.output_layers.get(layer) else {
            return;
        };
        if output.properties.is_empty() {
            return;
        }
        let Some(target) = self.targets.objects.get_mut(target) else {
            return;
        };
        let current = target
            .properties
            .get_or_insert_with(|| match target.origin {
                TargetOrigin::Existing(id) => self
                    .map
                    .layer(id)
                    .map(|l| l.properties.clone())
                    .unwrap_or_default(),
                TargetOrigin::New | TargetOrigin::Temporary => bt_core::Properties::new(),
            });
        for (name, value) in &output.properties {
            current.insert(name.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_wrap_or_stop_at_a_finite_maps_edge() {
        let size = Some(UVec2::new(4, 3));
        assert_eq!(landing(size, IVec2::new(-1, 0), false), None);
        assert_eq!(
            landing(size, IVec2::new(-1, 0), true),
            Some(IVec2::new(3, 0))
        );
        assert_eq!(
            landing(size, IVec2::new(5, 4), true),
            Some(IVec2::new(1, 1))
        );
        assert_eq!(
            landing(size, IVec2::new(3, 2), false),
            Some(IVec2::new(3, 2))
        );
        assert_eq!(
            landing(None, IVec2::new(-9, 9), false),
            Some(IVec2::new(-9, 9))
        );
    }
}
