//! The project-file record of AutoMap layers (`09-project-file.md`, §6.2): the setup, and what
//! was painted on each layer where it differs from the result as saved; and the painted map
//! rebuilt from a map as saved and its record. A layer whose checksum no longer matches its entry
//! (the map was changed elsewhere, in Tiled say) counts as all painted, with a note.
//!
//! A painted tile layer is kept whole, row by row, so restoring it needs nothing but the entry.
//! Objects the rules removed (`DeleteTiles` on an object output) are not kept, so they do not come
//! back.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{Cell, Layer, LayerId, LayerKind, Map, ObjectId, PlacedCell, TileLayer, TilesetSlot};
use bt_project::paths::tileset_key;
use bt_project::{AutoMapRecord, BindingRecord, PaintedRecord, RowCell, RunRecord, checksum, rows};
use glam::IVec2;

use super::auto_layers::AutoLayers;
use super::setup::Setup;
use crate::engine::LayerNames;
use crate::problem::{Problem, ProblemKind};

/// A map's painted state, rebuilt from the map as saved and its record.
#[derive(Clone, Debug)]
pub struct Restored {
    /// The painted map.
    pub painted: Map,
    /// The setup the record holds.
    pub setup: Setup,
    /// The entries that did not apply; each of their layers counts as all painted.
    pub notes: Vec<Problem>,
}

/// The record of AutoMap layers: `setup`, its rules file written as `rules` (relative to the
/// project file, which the caller knows), and what was painted on every layer where it differs
/// from the result.
pub fn painted_record(setup: &Setup, rules: String, layers: &AutoLayers) -> AutoMapRecord {
    let painted = layers.painted();
    let result = layers.result();
    let mut entries = Vec::new();
    result.for_each_layer(|layer, _| {
        let Some(drawn) = painted.layer(layer.id) else {
            return;
        };
        match (&layer.kind, &drawn.kind) {
            (LayerKind::Tile(made), LayerKind::Tile(painted_tiles))
                if !same_cells(made, painted_tiles) =>
            {
                entries.push(tile_entry(painted, result, layer, made, painted_tiles));
            }
            (LayerKind::Object(made), LayerKind::Object(painted_objects)) => {
                let kept: HashSet<ObjectId> =
                    painted_objects.objects.iter().map(|o| o.id).collect();
                let generated: Vec<u32> = made
                    .objects
                    .iter()
                    .filter(|o| !kept.contains(&o.id))
                    .map(|o| o.id.0)
                    .collect();
                if !generated.is_empty() {
                    entries.push(PaintedRecord {
                        layer: layer.id.0,
                        name: layer.name.clone(),
                        checksum: checksum::objects(result, &made.objects),
                        objects: true,
                        generated,
                        ..PaintedRecord::default()
                    });
                }
            }
            _ => {}
        }
    });
    AutoMapRecord {
        rules,
        seed: setup.seed,
        runs: setup
            .runs
            .iter()
            .map(|run| run_record(run, result))
            .collect(),
        painted: entries,
        next_object_id: (result.next_object_id != painted.next_object_id)
            .then_some(painted.next_object_id),
        unknown: BTreeMap::new(),
    }
}

/// The painted map rebuilt from `saved` (the map as its file holds it: the rules' result) and
/// its `record`, whose rules file is `rules` (resolved by the caller).
pub fn restore(record: &AutoMapRecord, rules: PathBuf, saved: &Map) -> Restored {
    let mut painted = saved.clone();
    let mut notes = Vec::new();
    for entry in &record.painted {
        if let Err(note) = restore_entry(&mut painted, saved, entry) {
            notes.push(note);
        }
    }
    if let Some(next) = record.next_object_id {
        painted.next_object_id = next;
    }
    let runs = record
        .runs
        .iter()
        .map(|run| {
            run.layers
                .iter()
                .fold(LayerNames::temporary(), |names, binding| {
                    names.rebound(
                        &binding.rules,
                        binding.objects,
                        Some(LayerId(binding.layer)),
                    )
                })
        })
        .collect();
    Restored {
        painted,
        setup: Setup {
            rules,
            runs,
            seed: record.seed,
        },
        notes,
    }
}

fn run_record(names: &LayerNames, map: &Map) -> RunRecord {
    let binding = |name: &str, objects: bool, layer: LayerId| BindingRecord {
        rules: name.to_owned(),
        objects,
        layer: layer.0,
        name: map.layer(layer).map(|l| l.name.clone()).unwrap_or_default(),
        unknown: BTreeMap::new(),
    };
    RunRecord {
        layers: names
            .tile_bindings()
            .iter()
            .map(|b| binding(&b.name, false, b.layer))
            .chain(
                names
                    .object_bindings()
                    .iter()
                    .map(|b| binding(&b.name, true, b.layer)),
            )
            .collect(),
        unknown: BTreeMap::new(),
    }
}

/// Whether two tile layers hold the same cells, chunks shared with each other compared by
/// address.
fn same_cells(a: &TileLayer, b: &TileLayer) -> bool {
    let positions: HashSet<_> = a.chunks().chain(b.chunks()).map(|c| c.pos).collect();
    positions
        .into_iter()
        .all(|pos| match (a.chunk(pos), b.chunk(pos)) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
            (Some(only), None) | (None, Some(only)) => only.is_empty(),
            (None, None) => true,
        })
}

/// The entry of a tile layer: its painted cells, whole, row by row from the leftmost.
fn tile_entry(
    painted: &Map,
    result: &Map,
    layer: &Layer,
    made: &TileLayer,
    painted_tiles: &TileLayer,
) -> PaintedRecord {
    let mut tilesets: Vec<String> = Vec::new();
    let mut by_row: BTreeMap<i32, Vec<PlacedCell>> = BTreeMap::new();
    for placed in painted_tiles.cells() {
        by_row.entry(placed.at.y).or_default().push(placed);
    }
    let x = painted_tiles.cells().map(|p| p.at.x).min().unwrap_or(0);
    let mut rows_text = BTreeMap::new();
    for (y, mut cells) in by_row {
        cells.sort_by_key(|placed| placed.at.x);
        let mut row: Vec<RowCell> = Vec::new();
        for placed in cells {
            let column = usize::try_from(placed.at.x - x).unwrap_or(0);
            row.resize(column, RowCell::Empty);
            row.push(row_cell(painted, placed.cell, &mut tilesets));
        }
        rows_text.insert(y, rows::encode(&row));
    }
    PaintedRecord {
        layer: layer.id.0,
        name: layer.name.clone(),
        checksum: checksum::tile_layer(result, made),
        tilesets,
        x,
        rows: rows_text,
        ..PaintedRecord::default()
    }
}

/// A painted cell as a row cell, its tileset named by key in `tilesets` (added when new).
fn row_cell(map: &Map, cell: Cell, tilesets: &mut Vec<String>) -> RowCell {
    let Some(reference) = cell.slot().and_then(|slot| map.tilesets.get(slot.index())) else {
        return RowCell::Empty;
    };
    let key = tileset_key(reference);
    let tileset = match tilesets.iter().position(|k| *k == key) {
        Some(index) => index,
        None => {
            tilesets.push(key);
            tilesets.len() - 1
        }
    };
    RowCell::Tile {
        tileset,
        tile: cell.tile(),
        flip: cell.flip(),
    }
}

/// Applies one entry to the painted map, or says why it does not apply.
fn restore_entry(painted: &mut Map, saved: &Map, entry: &PaintedRecord) -> Result<(), Problem> {
    let id = LayerId(entry.layer);
    let name = &entry.name;
    let layer = saved.layer(id).ok_or_else(|| {
        note(format!(
            "the layer {name} whose painted cells better-tiled.json keeps is gone"
        ))
    })?;
    match (&layer.kind, entry.objects) {
        (LayerKind::Tile(tiles), false) => {
            if checksum::tile_layer(saved, tiles) != entry.checksum {
                return Err(changed_elsewhere(name));
            }
            let rebuilt = painted_tiles(saved, tiles, entry)?;
            if let Some(LayerKind::Tile(target)) = painted.layer_mut(id).map(|l| &mut l.kind) {
                *target = rebuilt;
            }
            Ok(())
        }
        (LayerKind::Object(objects), true) => {
            if checksum::objects(saved, &objects.objects) != entry.checksum {
                return Err(changed_elsewhere(name));
            }
            let generated: HashSet<ObjectId> =
                entry.generated.iter().map(|&id| ObjectId(id)).collect();
            if let Some(LayerKind::Object(target)) = painted.layer_mut(id).map(|l| &mut l.kind) {
                Arc::make_mut(&mut target.objects).retain(|o| !generated.contains(&o.id));
            }
            Ok(())
        }
        _ => Err(note(format!(
            "the layer {name} is no longer the kind of layer better-tiled.json kept its painted \
             state for, so all of it counts as painted"
        ))),
    }
}

/// The painted cells of an entry, on a layer like `tiles` (its size, finite or not).
fn painted_tiles(
    saved: &Map,
    tiles: &TileLayer,
    entry: &PaintedRecord,
) -> Result<TileLayer, Problem> {
    let name = &entry.name;
    let slots = entry
        .tilesets
        .iter()
        .map(|key| {
            saved
                .tilesets
                .iter()
                .position(|t| tileset_key(t) == *key)
                .and_then(TilesetSlot::new)
                .ok_or_else(|| {
                    note(format!(
                        "the layer {name} was painted with the tileset {key}, which the map no \
                         longer has, so all of it counts as painted"
                    ))
                })
        })
        .collect::<Result<Vec<TilesetSlot>, Problem>>()?;
    let mut rebuilt = tiles.clone();
    rebuilt.rewrite_cells(|_| Cell::EMPTY);
    rebuilt.prune_empty_chunks();
    for (y, text) in &entry.rows {
        let cells = rows::decode(text).map_err(|error| {
            note(format!(
                "row {y} of the layer {name} in better-tiled.json cannot be read ({error}), so \
                 all of the layer counts as painted"
            ))
        })?;
        for (column, cell) in cells.iter().enumerate() {
            let RowCell::Tile {
                tileset,
                tile,
                flip,
            } = *cell
            else {
                continue;
            };
            let slot = slots.get(tileset).copied().ok_or_else(|| {
                note(format!(
                    "row {y} of the layer {name} in better-tiled.json names a tileset it does \
                     not list, so all of the layer counts as painted"
                ))
            })?;
            let x = entry.x + i32::try_from(column).unwrap_or(i32::MAX);
            rebuilt
                .set_cell(IVec2::new(x, *y), Cell::new(slot, tile).with_flip(flip))
                .map_err(|error| {
                    note(format!(
                        "a painted cell of the layer {name} does not fit it ({error}), so all of \
                         the layer counts as painted"
                    ))
                })?;
        }
    }
    Ok(rebuilt)
}

fn changed_elsewhere(name: &str) -> Problem {
    note(format!(
        "the layer {name} was changed outside better-tiled since its painted cells were kept, \
         so all of it counts as painted"
    ))
}

fn note(text: String) -> Problem {
    Problem::new(ProblemKind::PaintedNotRestored, text)
}
