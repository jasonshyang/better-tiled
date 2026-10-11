//! What changed in a tile layer's cells: how many, where, what they hold now, how many tiles
//! were placed in empty cells or cleared, and each terrain's cells before and after, counting a
//! cell as a terrain's when the terrain covers more than half of its tile
//! ([`Names::terrain_covering`]), as `bt view` shows it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bt_core::{Cell, ChunkPos, Map, TileLayer};
use bt_utils::IRect;
use bt_utils::text::plural;
use glam::IVec2;
use serde::{Deserialize, Serialize};

use crate::names::Names;
use crate::regions::CellsShowing;
use crate::words::{RectJson, region_text};

/// The most kinds of what changed cells hold that a line lists.
const KINDS_LISTED: usize = 5;

/// The most terrains whose counts a line lists.
const TERRAINS_LISTED: usize = 4;

/// What changed in a tile layer's cells.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellChanges {
    /// How many cells changed.
    pub count: usize,
    /// The rectangle holding them.
    #[serde(with = "RectJson")]
    pub bounds: IRect,
    /// What the changed cells hold now, the most first: a terrain, `grass/water edges`, a tile,
    /// or `nothing` for the cells cleared.
    pub became: Vec<CellsShowing>,
    /// How many of them were empty and hold a tile now.
    pub placed: usize,
    /// How many of them held a tile and are empty now.
    pub cleared: usize,
    /// The terrains whose cells on the layer changed in number, with their cells before and
    /// after.
    pub terrains: Vec<TerrainCells>,
}

/// A terrain's cells on a layer before and after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainCells {
    /// The terrain.
    pub terrain: String,
    /// Its cells before.
    pub before: usize,
    /// Its cells after.
    pub after: usize,
}

/// One cell that changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChangedCell {
    /// Where.
    pub(crate) at: IVec2,
    /// What it held.
    pub(crate) was: Cell,
    /// What it holds.
    pub(crate) now: Cell,
}

impl CellChanges {
    /// What changed from `before` (none for a new layer) to `after`, both of a layer the maps
    /// share; `None` when no cell did. `names_before` and `names_after` name the maps' terrains.
    pub(crate) fn between(
        maps: MapsCompared<'_>,
        before: Option<&TileLayer>,
        after: &TileLayer,
    ) -> Option<CellChanges> {
        let changed = changed_cells(before, after);
        let bounds = IRect::bounding(changed.iter().map(|c| c.at))?;
        let mut became: BTreeMap<String, usize> = BTreeMap::new();
        let mut placed = 0;
        let mut cleared = 0;
        for cell in &changed {
            let what = if cell.now.is_empty() {
                cleared += 1;
                "nothing".to_owned()
            } else {
                if cell.was.is_empty() {
                    placed += 1;
                }
                maps.names_after.tile_label(maps.after, cell.now)
            };
            *became.entry(what).or_default() += 1;
        }
        let mut became: Vec<CellsShowing> = became
            .into_iter()
            .map(|(what, cells)| CellsShowing { what, cells })
            .collect();
        became.sort_by(|a, b| b.cells.cmp(&a.cells).then_with(|| a.what.cmp(&b.what)));
        let was = before.map(|layer| terrain_counts(maps.before, maps.names_before, layer));
        let now = terrain_counts(maps.after, maps.names_after, after);
        let was = was.unwrap_or_default();
        let names: BTreeSet<&String> = was.keys().chain(now.keys()).collect();
        let terrains = names
            .into_iter()
            .filter_map(|terrain| {
                let before = was.get(terrain).copied().unwrap_or(0);
                let after = now.get(terrain).copied().unwrap_or(0);
                (before != after).then(|| TerrainCells {
                    terrain: terrain.clone(),
                    before,
                    after,
                })
            })
            .collect();
        Some(CellChanges {
            count: changed.len(),
            bounds,
            became,
            placed,
            cleared,
            terrains,
        })
    }

    /// The change in words, after the layer's name: `30 cells changed in 2,1 6x5: water ×12,
    /// grass/water edges ×18; grass 96 → 66 cells, water 0 → 12 cells`.
    pub(crate) fn text(&self) -> String {
        let shown: Vec<String> = self
            .became
            .iter()
            .take(KINDS_LISTED)
            .map(|kind| format!("{} ×{}", kind.what, kind.cells))
            .collect();
        let mut kinds = shown.join(", ");
        if self.became.len() > KINDS_LISTED {
            kinds.push_str(&format!(
                ", {} more kinds",
                self.became.len() - KINDS_LISTED
            ));
        }
        let mut text = format!(
            "{} changed in {}: {kinds}",
            plural(self.count, "cell", "cells"),
            region_text(self.bounds)
        );
        let totals: Vec<String> = self
            .terrains
            .iter()
            .take(TERRAINS_LISTED)
            .map(|t| format!("{} {} → {} cells", t.terrain, t.before, t.after))
            .collect();
        if !totals.is_empty() {
            text.push_str(&format!("; {}", totals.join(", ")));
        }
        text
    }
}

/// The two maps compared, with their names.
#[derive(Clone, Copy)]
pub(crate) struct MapsCompared<'m> {
    pub(crate) before: &'m Map,
    pub(crate) after: &'m Map,
    pub(crate) names_before: &'m Names,
    pub(crate) names_after: &'m Names,
}

/// The cells that differ between two versions of a layer (none for a new layer), row by row;
/// chunks the two share are skipped unread.
pub(crate) fn changed_cells(before: Option<&TileLayer>, after: &TileLayer) -> Vec<ChangedCell> {
    let mut positions: BTreeSet<ChunkPos> = after.chunks().map(|c| c.pos).collect();
    if let Some(before) = before {
        positions.extend(before.chunks().map(|c| c.pos));
    }
    let mut changed = Vec::new();
    for pos in positions {
        let old = before.and_then(|layer| layer.chunk(pos));
        let new = after.chunk(pos);
        if let (Some(old), Some(new)) = (old, new)
            && Arc::ptr_eq(old, new)
        {
            continue;
        }
        for at in pos.rect().points() {
            let was = before.map_or(Cell::EMPTY, |layer| layer.cell(at));
            let now = after.cell(at);
            if was != now {
                changed.push(ChangedCell { at, was, now });
            }
        }
    }
    changed.sort_by_key(|c| (c.at.y, c.at.x));
    changed
}

/// How many cells of a layer each terrain covers more than half of.
fn terrain_counts(map: &Map, names: &Names, layer: &TileLayer) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for placed in layer.cells() {
        if let Some(terrain) = names.terrain_covering(map, placed.cell) {
            *counts.entry(terrain.name.clone()).or_default() += 1;
        }
    }
    counts
}
