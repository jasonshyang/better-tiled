//! Binding a rule set to a map for a run. Tilesets first, as Tiled's `unifyTilesets` does: a rule
//! tileset that is one of the map's (the same value or file) takes its slot; one a rule map
//! embeds may stand for a similar tileset, the map's first, else one an earlier rule map brings;
//! every other one gets a slot after the map's, and is added to the map at the end if the run
//! placed its tiles. Then the names the rules use, which the run's layer names map to layers.

use bt_core::{Cell, Flip, Layer, LayerId, LayerKind, Map, TilesetSlot};

use crate::problem::{Problem, ProblemKind};
use crate::rulemap::{MatchCell, TableTile, TilesetTable};

/// A condition's cell bound to the run's slots, ready to compare with cells read from grids.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct BoundCell {
    /// 0 for the empty cell, the slot plus one for a tile, [`UNMATCHABLE`] for a tile whose
    /// tileset has no slot.
    pub(crate) key: u32,
    /// The tile.
    pub(crate) tile: u32,
    /// The tile's flips.
    pub(crate) flip: Flip,
    /// The flips that must agree.
    pub(crate) mask: Flip,
}

/// The key of a tile whose tileset could not be given a slot: no cell has it.
pub(crate) const UNMATCHABLE: u32 = u32::MAX;

impl BoundCell {
    /// Whether a cell whose key is `key` matches (Tiled's `cellMatches`, the empty cell
    /// matching only the empty cell).
    pub(crate) fn matches(&self, key: u32, cell: Cell) -> bool {
        if self.key == 0 {
            return key == 0;
        }
        self.key == key
            && self.tile == cell.tile().0
            && (self.flip & self.mask) == (cell.flip() & self.mask)
    }

    /// Whether it is the empty cell.
    pub(crate) fn is_empty(&self) -> bool {
        self.key == 0
    }
}

/// The rule set's tilesets placed in the run's numbering.
#[derive(Clone, Debug)]
pub(crate) struct TilesetBinding {
    /// Each table tileset's slot; `None` when the map could hold no more tilesets.
    slots: Vec<Option<TilesetSlot>>,
    /// For each slot of the map, the first slot holding the same tileset (a map can list one
    /// twice).
    alias: Vec<u32>,
    /// The table tilesets the run would add to the map, in order: the first gets the slot after
    /// the map's last.
    pub(crate) added: Vec<usize>,
}

impl TilesetBinding {
    /// Binds `table` to `map`'s tilesets; a problem when the map cannot hold all the rules'
    /// tilesets.
    pub(crate) fn new(table: &TilesetTable, map: &Map) -> (TilesetBinding, Vec<Problem>) {
        let alias = (0..map.tilesets.len())
            .map(|i| {
                let first = map
                    .tilesets
                    .iter()
                    .position(|r| map.tilesets.get(i).is_some_and(|t| t.is_same_tileset_as(r)))
                    .unwrap_or(i);
                u32::try_from(first).unwrap_or(u32::MAX)
            })
            .collect();
        let mut binding = TilesetBinding {
            slots: Vec::with_capacity(table.entries().len()),
            alias,
            added: Vec::new(),
        };
        let mut problems = Vec::new();
        for (index, entry) in table.entries().iter().enumerate() {
            let reference = &entry.reference;
            let same = map
                .tilesets
                .iter()
                .position(|r| r.is_same_tileset_as(reference));
            let similar = || {
                if reference.file.is_some() {
                    return None;
                }
                let tileset = &reference.tileset;
                map.tilesets
                    .iter()
                    .position(|r| tileset.is_similar_to(&r.tileset))
                    .or_else(|| {
                        binding
                            .added
                            .iter()
                            .position(|&added| {
                                table
                                    .get(added)
                                    .is_some_and(|e| tileset.is_similar_to(&e.reference.tileset))
                            })
                            .map(|rank| map.tilesets.len() + rank)
                    })
            };
            let slot = match same.or_else(similar) {
                Some(slot) => TilesetSlot::new(slot),
                None => {
                    let slot = TilesetSlot::new(map.tilesets.len() + binding.added.len());
                    match slot {
                        Some(_) => binding.added.push(index),
                        None => problems.push(Problem::new(
                            ProblemKind::TooManyTilesets,
                            format!(
                                "the map cannot hold another tileset, so the tiles of {} are \
                                 never placed and never matched",
                                entry.label()
                            ),
                        )),
                    }
                    slot
                }
            };
            binding.slots.push(slot);
        }
        (binding, problems)
    }

    /// The key a grid's cell compares by: 0 when empty, else its tileset's first slot plus one.
    pub(crate) fn key_of(&self, cell: Cell) -> u32 {
        match cell.slot() {
            None => 0,
            Some(slot) => {
                let index = slot.index();
                let canonical = self
                    .alias
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| u32::try_from(index).unwrap_or(u32::MAX));
                canonical.saturating_add(1)
            }
        }
    }

    /// A condition's cell, bound.
    pub(crate) fn bind_match(&self, cell: &MatchCell) -> BoundCell {
        match cell.tile {
            None => BoundCell {
                key: 0,
                tile: 0,
                flip: Flip::empty(),
                mask: cell.mask,
            },
            Some(tile) => BoundCell {
                key: self.slot_of(tile.tileset).map_or(UNMATCHABLE, |slot| {
                    u32::try_from(slot.index()).map_or(UNMATCHABLE, |i| i + 1)
                }),
                tile: tile.tile.0,
                flip: tile.flip,
                mask: cell.mask,
            },
        }
    }

    /// The cell a rule writes for `tile`; `None` when its tileset has no slot.
    pub(crate) fn cell_of(&self, tile: TableTile) -> Option<Cell> {
        let slot = self.slot_of(tile.tileset)?;
        Some(Cell::new(slot, tile.tile).with_flip(tile.flip))
    }

    /// The slot of a table tileset.
    pub(crate) fn slot_of(&self, tileset: usize) -> Option<TilesetSlot> {
        self.slots.get(tileset).copied().flatten()
    }
}

/// The bottom-most layer named `name` whose kind `wanted` accepts, as Tiled's `findLayer`
/// finds it: depth-first in drawing order, inside groups too.
pub(crate) fn find_layer<'m>(
    map: &'m Map,
    name: &str,
    wanted: fn(&LayerKind) -> bool,
) -> Option<&'m Layer> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name && wanted(&layer.kind) {
            found = Some(layer);
        }
    });
    found
}

/// Whether a layer holds cells.
pub(crate) fn is_tiles(kind: &LayerKind) -> bool {
    matches!(kind, LayerKind::Tile(_))
}

/// Whether a layer holds objects.
pub(crate) fn is_objects(kind: &LayerKind) -> bool {
    matches!(kind, LayerKind::Object(_))
}

/// Whether the layer `id` or a group it is in is locked.
pub(crate) fn is_locked(map: &Map, id: LayerId) -> bool {
    map.effective_layer(id).is_some_and(|e| e.locked)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{FileRef, TileId, Tileset, TilesetRef};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    /// A drawn atlas; atlases of different names have different grids, so they are not
    /// similar (the drawing depends on the grid only).
    fn atlas(name: &str) -> Arc<Tileset> {
        let columns = match name {
            "grass" => 4,
            "water" => 3,
            _ => 2,
        };
        Atlas::builder()
            .name(name)
            .grid(columns, 2)
            .build_embedded()
            .unwrap()
            .tileset
    }

    fn external(tileset: &Arc<Tileset>, path: &str) -> TilesetRef {
        TilesetRef {
            file: Some(FileRef::new(path, path)),
            first_gid: None,
            tileset: Arc::clone(tileset),
        }
    }

    #[test]
    fn rule_tilesets_bind_as_tiled_unifies_them() {
        let grass = atlas("grass");
        let water = atlas("water");
        let map = Map {
            tilesets: vec![
                external(&grass, "/maps/grass.tsx"),
                TilesetRef::embedded(Arc::clone(&water)),
            ],
            ..Map::default()
        };
        let mut table = TilesetTable::new();
        // The same value, an embedded copy of the map's embedded tileset, a file of the rules'
        // own, and an embedded copy of that file.
        let same = table.insert_tileset(&TilesetRef::embedded(Arc::clone(&grass)), 0);
        let copy = table.insert_tileset(&TilesetRef::embedded(Arc::new(Tileset::clone(&water))), 0);
        let lava = atlas("lava");
        let own = table.insert_tileset(&external(&lava, "/rules/lava.tsx"), 1);
        let own_copy =
            table.insert_tileset(&TilesetRef::embedded(Arc::new(Tileset::clone(&lava))), 1);
        let (binding, problems) = TilesetBinding::new(&table, &map);
        assert!(problems.is_empty());
        let slot = |i| binding.slot_of(i).map(TilesetSlot::index);
        assert_eq!(
            [slot(same), slot(copy), slot(own), slot(own_copy)],
            [Some(0), Some(1), Some(2), Some(2)]
        );
        assert_eq!(binding.added, [own]);
    }

    #[test]
    fn a_file_is_never_unified_with_a_similar_tileset() {
        let grass = atlas("grass");
        let map = Map {
            tilesets: vec![TilesetRef::embedded(Arc::clone(&grass))],
            ..Map::default()
        };
        let mut table = TilesetTable::new();
        let copy = table.insert_tileset(
            &external(&Arc::new(Tileset::clone(&grass)), "/rules/grass.tsx"),
            0,
        );
        let (binding, _) = TilesetBinding::new(&table, &map);
        assert_eq!(binding.slot_of(copy).map(TilesetSlot::index), Some(1));
    }

    #[test]
    fn cells_compare_by_tileset_tile_and_the_flips_that_must_agree() {
        let grass = atlas("grass");
        let map = Map {
            tilesets: vec![
                TilesetRef::embedded(Arc::clone(&grass)),
                TilesetRef::embedded(Arc::clone(&grass)),
            ],
            ..Map::default()
        };
        let mut table = TilesetTable::new();
        let index = table.insert_tileset(&TilesetRef::embedded(Arc::clone(&grass)), 0);
        let (binding, _) = TilesetBinding::new(&table, &map);
        let wanted = binding.bind_match(&MatchCell {
            tile: Some(TableTile {
                tileset: index,
                tile: TileId(3),
                flip: Flip::H,
            }),
            mask: Flip::all() - Flip::V,
        });
        let cell = |slot: usize, flip: Flip| {
            Cell::new(TilesetSlot::new(slot).unwrap(), TileId(3)).with_flip(flip)
        };
        let check = |c: Cell| wanted.matches(binding.key_of(c), c);
        assert!(check(cell(0, Flip::H)));
        assert!(check(cell(0, Flip::H | Flip::V)), "V is ignored");
        assert!(!check(cell(0, Flip::empty())));
        assert!(
            check(cell(1, Flip::H)),
            "the second slot lists the same tileset"
        );
        let empty = binding.bind_match(&MatchCell::EMPTY);
        assert!(empty.matches(binding.key_of(Cell::EMPTY), Cell::EMPTY));
        assert!(!empty.matches(binding.key_of(cell(0, Flip::H)), cell(0, Flip::H)));
    }
}
