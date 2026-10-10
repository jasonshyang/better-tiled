//! The tilesets of a map: finding one that can stand in for another, adding, replacing and
//! removing them, what a removal would lose, and swapping two tiles everywhere. Ported from
//! Tiled's `Tileset::findSimilarTileset`, `Map::replaceTileset` and `SwapTiles`
//! (`reference/tiled/src/libtiled/tileset.cpp`, `libtiled/map.cpp`, `tiled/swaptiles.cpp`).

use std::sync::Arc;

use bt_core::{Cell, LayerKind, Map, ObjectId, TilesetRef, TilesetSlot};

use crate::error::EditError;
use crate::report::RemovedTileset;

/// Where a tileset's tiles go in a map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Unified {
    /// The slot.
    pub slot: TilesetSlot,
    /// Whether the tileset was added for them.
    pub added: bool,
}

/// What removing a tileset would take with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TilesetUsage {
    /// The cells holding its tiles.
    pub cells: usize,
    /// The tile objects showing its tiles (template instances included).
    pub objects: Vec<ObjectId>,
}

/// The slot of a tileset of `map` that can stand in for `tileset`: the same tileset, the same
/// file, or for an embedded tileset one similar by Tiled's rules. A tileset with a file of its
/// own stands only for itself, as in Tiled (which never loads a file twice).
pub fn find_tileset(map: &Map, tileset: &TilesetRef) -> Option<TilesetSlot> {
    let index = map
        .tilesets
        .iter()
        .position(|r| r.is_same_tileset_as(tileset))
        .or_else(|| {
            if tileset.file.is_some() {
                return None;
            }
            map.tilesets
                .iter()
                .position(|r| r.file.is_none() && r.tileset.is_similar_to(&tileset.tileset))
        })?;
    TilesetSlot::new(index)
}

/// The slot `tileset`'s tiles take in `map`: one that can stand in for it, or a new one.
pub fn unify_tileset(map: &mut Map, tileset: &TilesetRef) -> Result<Unified, EditError> {
    if let Some(slot) = find_tileset(map, tileset) {
        return Ok(Unified { slot, added: false });
    }
    let slot = append(map, tileset.clone())?;
    Ok(Unified { slot, added: true })
}

/// Adds a tileset at the end of the list, unless the map has it already (the same tileset or
/// file), and returns its slot.
pub fn add_tileset(map: &mut Map, tileset: TilesetRef) -> Result<Unified, EditError> {
    if let Some(index) = map
        .tilesets
        .iter()
        .position(|r| r.is_same_tileset_as(&tileset))
    {
        let slot = TilesetSlot::new(index).ok_or(EditError::TooManyTilesets)?;
        return Ok(Unified { slot, added: false });
    }
    let slot = append(map, tileset)?;
    Ok(Unified { slot, added: true })
}

fn append(map: &mut Map, tileset: TilesetRef) -> Result<TilesetSlot, EditError> {
    let slot = TilesetSlot::new(map.tilesets.len()).ok_or(EditError::TooManyTilesets)?;
    map.tilesets.push(tileset);
    Ok(slot)
}

/// Swaps in another tileset for the one in `slot`; cells and tile objects keep their tile ids.
/// When the map has the new tileset already, its tiles move to that slot and `slot` goes, so a
/// map never lists a tileset twice. Returns the tileset replaced.
pub fn replace_tileset(
    map: &mut Map,
    slot: TilesetSlot,
    tileset: TilesetRef,
) -> Result<TilesetRef, EditError> {
    let old = map
        .tilesets
        .get(slot.index())
        .cloned()
        .ok_or(EditError::NoSuchTileset(slot))?;
    let elsewhere = map
        .tilesets
        .iter()
        .enumerate()
        .find(|(index, r)| *index != slot.index() && r.is_same_tileset_as(&tileset))
        .map(|(index, _)| index);
    match elsewhere.and_then(TilesetSlot::new) {
        Some(existing) => {
            let to_existing = |cell: Cell| {
                if cell.slot() == Some(slot) {
                    Cell::new(existing, cell.tile()).with_flip(cell.flip())
                } else {
                    cell
                }
            };
            rewrite_cells(map, to_existing);
            rewrite_object_tiles(map, to_existing);
            take_out_slot(map, slot);
        }
        None => {
            if let Some(entry) = map.tilesets.get_mut(slot.index()) {
                *entry = tileset;
            }
        }
    }
    Ok(old)
}

/// What removing the tileset in `slot` would take with it.
pub fn tileset_usage(map: &Map, slot: TilesetSlot) -> Result<TilesetUsage, EditError> {
    if map.tilesets.get(slot.index()).is_none() {
        return Err(EditError::NoSuchTileset(slot));
    }
    let mut usage = TilesetUsage::default();
    map.for_each_layer(|layer, _| match &layer.kind {
        LayerKind::Tile(tiles) => {
            usage.cells += tiles
                .cells()
                .filter(|p| p.cell.slot() == Some(slot))
                .count();
        }
        LayerKind::Object(objects) => usage.objects.extend(
            objects
                .objects
                .iter()
                .filter(|o| o.tile.is_some_and(|cell| cell.slot() == Some(slot)))
                .map(|o| o.id),
        ),
        LayerKind::Image(_) | LayerKind::Group(_) => {}
    });
    Ok(usage)
}

/// Removes the tileset in `slot` with its cells and tile objects; the tilesets after it move up
/// a slot, in every cell and object.
pub fn remove_tileset(map: &mut Map, slot: TilesetSlot) -> Result<RemovedTileset, EditError> {
    let usage = tileset_usage(map, slot)?;
    let tileset = map
        .tilesets
        .get(slot.index())
        .cloned()
        .ok_or(EditError::NoSuchTileset(slot))?;
    let cells = rewrite_cells(map, |cell| {
        if cell.slot() == Some(slot) {
            Cell::EMPTY
        } else {
            cell
        }
    });
    if !usage.objects.is_empty() {
        map.for_each_layer_mut(|layer| {
            if let Some(objects) = layer.as_object_layer_mut()
                && objects
                    .objects
                    .iter()
                    .any(|o| usage.objects.contains(&o.id))
            {
                Arc::make_mut(&mut objects.objects).retain(|o| !usage.objects.contains(&o.id));
            }
        });
    }
    take_out_slot(map, slot);
    Ok(RemovedTileset {
        tileset,
        cells,
        objects: usage.objects,
    })
}

/// Takes `slot` out of the list, moving the cells and objects of later tilesets up one slot.
/// Nothing may refer to `slot` itself any more.
fn take_out_slot(map: &mut Map, slot: TilesetSlot) {
    let moved_up = |cell: Cell| match cell.slot() {
        Some(s) if s > slot => TilesetSlot::new(s.index() - 1)
            .map_or(cell, |up| Cell::new(up, cell.tile()).with_flip(cell.flip())),
        _ => cell,
    };
    rewrite_cells(map, moved_up);
    rewrite_object_tiles(map, moved_up);
    map.tilesets.remove(slot.index());
}

/// Exchanges two tiles everywhere in the map: cells keep their flips, and a tile object that
/// was the size of its tile takes the size of the other (Tiled's `SwapTiles`). The tiles are
/// named by cells; their flips do not matter. Returns how many cells and objects changed.
pub fn swap_tiles(map: &mut Map, a: Cell, b: Cell) -> Result<usize, EditError> {
    let size_of = |map: &Map, cell: Cell| -> Result<glam::DVec2, EditError> {
        let slot = cell.slot().ok_or(EditError::EmptyCell)?;
        let tileset = map.tileset(slot).ok_or(EditError::NoSuchTileset(slot))?;
        if !tileset.has_tile(cell.tile()) {
            return Err(EditError::NoSuchTile {
                slot,
                tile: cell.tile(),
            });
        }
        Ok(tileset.tile_pixel_size(cell.tile()).as_dvec2())
    };
    let (size_a, size_b) = (size_of(map, a)?, size_of(map, b)?);
    let swapped = |cell: Cell| {
        if cell.same_tile(a) {
            b.with_flip(cell.flip())
        } else if cell.same_tile(b) {
            a.with_flip(cell.flip())
        } else {
            cell
        }
    };
    let mut changed = rewrite_cells(map, swapped);
    map.for_each_layer_mut(|layer| {
        let Some(objects) = layer.as_object_layer_mut() else {
            return;
        };
        let uses =
            |o: &bt_core::MapObject| o.tile.is_some_and(|t| t.same_tile(a) || t.same_tile(b));
        if !objects.objects.iter().any(uses) {
            return;
        }
        for object in Arc::make_mut(&mut objects.objects).iter_mut() {
            let Some(tile) = object.tile else { continue };
            let (from, to) = if tile.same_tile(a) {
                (size_a, size_b)
            } else if tile.same_tile(b) {
                (size_b, size_a)
            } else {
                continue;
            };
            object.tile = Some(swapped(tile));
            if from != to && object.size == from {
                object.size = to;
            }
            changed += 1;
        }
    });
    Ok(changed)
}

/// Passes every cell of every tile layer through `rewrite`; how many changed.
pub(crate) fn rewrite_cells(map: &mut Map, rewrite: impl Fn(Cell) -> Cell) -> usize {
    let mut changed = 0;
    map.for_each_layer_mut(|layer| {
        if let Some(tiles) = layer.as_tile_layer_mut() {
            changed += tiles.rewrite_cells(&rewrite);
            tiles.prune_empty_chunks();
        }
    });
    changed
}

/// Passes every tile object's tile through `rewrite`; the objects that changed. Object lists
/// that do not change stay shared.
pub(crate) fn rewrite_object_tiles(map: &mut Map, rewrite: impl Fn(Cell) -> Cell) -> Vec<ObjectId> {
    let mut changed = Vec::new();
    map.for_each_layer_mut(|layer| {
        let Some(objects) = layer.as_object_layer_mut() else {
            return;
        };
        let touched = objects
            .objects
            .iter()
            .any(|o| o.tile.is_some_and(|t| rewrite(t) != t));
        if !touched {
            return;
        }
        for object in Arc::make_mut(&mut objects.objects).iter_mut() {
            if let Some(tile) = object.tile {
                let new = rewrite(tile);
                if new != tile {
                    object.tile = Some(new);
                    changed.push(object.id);
                }
            }
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use bt_core::{FileRef, MapObject, TileId, Tileset};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::IRect;
    use glam::{DVec2, UVec2};

    use super::*;
    use crate::testutil::{grid_of, tile, tile_of, trimmed};

    fn atlas(name: &str) -> Arc<Tileset> {
        Atlas::builder()
            .name(name)
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

    fn all(map: &Map, layer: usize) -> String {
        let tiles = map.layers[layer].as_tile_layer().unwrap();
        grid_of(
            tiles,
            IRect {
                x: 0,
                y: 0,
                w: map.width as i32,
                h: map.height as i32,
            },
        )
    }

    #[test]
    fn a_similar_embedded_tileset_stands_in_and_a_file_only_for_itself() {
        let grass = atlas("grass");
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(2))
            .tileset(Arc::clone(&grass))
            .build()
            .unwrap();
        // Another value with the same image and grid: Tiled's "similar" tileset.
        let copy = TilesetRef::embedded(Arc::new(grass.as_ref().clone()));
        assert_eq!(find_tileset(&map, &copy).map(TilesetSlot::index), Some(0));
        let mut other = grass.as_ref().clone();
        other.margin = 1;
        assert_eq!(
            find_tileset(&map, &TilesetRef::embedded(Arc::new(other))),
            None
        );
        let file = external(&Arc::new(grass.as_ref().clone()), "/maps/grass.tsx");
        assert_eq!(
            find_tileset(&map, &file),
            None,
            "a file is not an embedded tileset"
        );
    }

    #[test]
    fn unifying_adds_a_tileset_once() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(2))
            .tileset(atlas("grass"))
            .build()
            .unwrap();
        let water = external(&atlas("water"), "/maps/water.tsx");
        let first = unify_tileset(&mut map, &water).unwrap();
        assert_eq!((first.slot.index(), first.added), (1, true));
        // The same file read again is the same tileset.
        let again = external(&atlas("water"), "/maps/water.tsx");
        assert_eq!(
            unify_tileset(&mut map, &again).unwrap(),
            Unified {
                slot: first.slot,
                added: false
            }
        );
        assert!(!add_tileset(&mut map, again).unwrap().added);
        assert_eq!(map.tilesets.len(), 2);
    }

    #[test]
    fn removing_a_tileset_empties_its_cells_and_moves_later_ones_up() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(atlas("a"))
            .tileset(atlas("b"))
            .tileset(atlas("c"))
            .tile_layer("ground", "1 2@1 3@2h")
            .object_layer(
                "things",
                vec![
                    MapObject {
                        tile: Some(tile_of(1, 4)),
                        ..MapObject::default()
                    },
                    MapObject {
                        tile: Some(tile_of(2, 5)),
                        ..MapObject::default()
                    },
                ],
            )
            .build()
            .unwrap();
        let usage = tileset_usage(&map, TilesetSlot::new(1).unwrap()).unwrap();
        assert_eq!(
            usage,
            TilesetUsage {
                cells: 1,
                objects: vec![ObjectId(1)]
            }
        );
        let removed = remove_tileset(&mut map, TilesetSlot::new(1).unwrap()).unwrap();
        assert_eq!((removed.cells, removed.objects), (1, vec![ObjectId(1)]));
        assert_eq!(removed.tileset.tileset.name, "b");
        assert_eq!(all(&map, 0), "1 . 3@1h");
        let objects = &map.layers[1].as_object_layer().unwrap().objects;
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].tile, Some(tile_of(1, 5)));
        assert!(map.validate().is_empty(), "{:?}", map.validate());
        assert_eq!(
            remove_tileset(&mut map, TilesetSlot::new(5).unwrap()),
            Err(EditError::NoSuchTileset(TilesetSlot::new(5).unwrap()))
        );
    }

    #[test]
    fn replacing_with_a_tileset_the_map_has_merges_the_slots() {
        let b = atlas("b");
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
            .tileset(atlas("a"))
            .tileset(Arc::clone(&b))
            .tile_layer("ground", "1 2@1")
            .build()
            .unwrap();
        let old = replace_tileset(
            &mut map,
            TilesetSlot::new(0).unwrap(),
            TilesetRef::embedded(Arc::clone(&b)),
        )
        .unwrap();
        assert_eq!(old.tileset.name, "a");
        assert_eq!(map.tilesets.len(), 1);
        assert_eq!(all(&map, 0), "1 2", "both now in the one slot b has");
        let c = TilesetRef::embedded(atlas("c"));
        replace_tileset(&mut map, TilesetSlot::new(0).unwrap(), c).unwrap();
        assert_eq!(map.tilesets[0].tileset.name, "c");
        assert_eq!(all(&map, 0), "1 2", "ids are kept");
    }

    #[test]
    fn swapping_tiles_keeps_flips_and_fits_tile_objects() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
            .tileset(atlas("a"))
            .tile_layer("ground", "1h 2 3")
            .object_layer(
                "things",
                vec![MapObject {
                    tile: Some(tile(2)),
                    size: DVec2::new(16.0, 16.0),
                    ..MapObject::default()
                }],
            )
            .build()
            .unwrap();
        let changed = swap_tiles(&mut map, tile(1), tile(2)).unwrap();
        assert_eq!(changed, 3);
        assert_eq!(all(&map, 0), trimmed("2h 1 3"));
        let objects = &map.layers[1].as_object_layer().unwrap().objects;
        assert_eq!(objects[0].tile, Some(tile(1)));
        assert_eq!(
            swap_tiles(&mut map, tile(1), tile(99)),
            Err(EditError::NoSuchTile {
                slot: TilesetSlot::new(0).unwrap(),
                tile: TileId(99)
            })
        );
        assert_eq!(
            swap_tiles(&mut map, Cell::EMPTY, tile(1)),
            Err(EditError::EmptyCell)
        );
    }
}
