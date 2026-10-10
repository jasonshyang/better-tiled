//! Tile layers' instances, cached per chunk.
//!
//! A chunk's instances are built the first time it is drawn and again only when its `Arc`
//! changes (the cache holds the `Arc`, so its address is not reused while cached), when the
//! map's tilesets are assigned other sprites, or when the grid changes. They are stored sorted
//! by Tiled's draw order, and the chunks a view shows are merged by that order, so an oversized
//! tile at a chunk boundary stacks exactly as Tiled stacks it on every orientation.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use bt_core::{Chunk, ChunkPos, DrawOrderKey, Map, MapGeometry, TileLayer};
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2};

use crate::instance::{Instance, cell_instance};
use crate::sprites::Sprites;

/// What a cached chunk's instances were built from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ChunkKey {
    chunk: usize,
    pos: ChunkPos,
    geometry: MapGeometry,
    tilesets: u64,
}

/// An instance with its place in the draw order.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct KeyedInstance {
    pub(crate) key: DrawOrderKey,
    pub(crate) instance: Instance,
}

/// A chunk's instances.
#[derive(Clone, Debug)]
struct CachedChunk {
    /// Held so the address in the key is not reused while cached.
    _chunk: Arc<Chunk>,
    instances: Vec<KeyedInstance>,
    last_used: u64,
}

impl CachedChunk {
    fn bytes(&self) -> usize {
        self.instances.len() * std::mem::size_of::<KeyedInstance>() + 64
    }
}

/// What building a chunk's instances needs: the map (its tilesets), its grid, the sprites, and
/// the map's tileset signature.
pub(crate) struct ChunkContext<'a> {
    pub(crate) map: &'a Map,
    pub(crate) geometry: MapGeometry,
    pub(crate) sprites: &'a Sprites,
    pub(crate) tilesets: u64,
}

/// The cache.
pub(crate) struct ChunkCache {
    entries: HashMap<ChunkKey, CachedChunk>,
    bytes: usize,
    budget_bytes: usize,
}

impl ChunkCache {
    pub(crate) fn new(budget_bytes: usize) -> ChunkCache {
        ChunkCache {
            entries: HashMap::new(),
            bytes: 0,
            budget_bytes,
        }
    }

    /// Appends, in Tiled's draw order, the instances of the cells of `layer` that may show in
    /// `tiles` (a rectangle of cells, already grown by the draw margins).
    pub(crate) fn collect(
        &mut self,
        layer: &TileLayer,
        tiles: IRect,
        cx: &ChunkContext<'_>,
        frame: u64,
        out: &mut Vec<Instance>,
    ) {
        let visible = visible_chunks(layer, tiles);
        let mut keys = Vec::with_capacity(visible.len());
        for found in visible {
            let key = ChunkKey {
                chunk: Arc::as_ptr(found.chunk) as usize,
                pos: found.pos,
                geometry: cx.geometry,
                tilesets: cx.tilesets,
            };
            match self.entries.get_mut(&key) {
                Some(entry) => entry.last_used = frame,
                None => {
                    let entry = CachedChunk {
                        _chunk: Arc::clone(found.chunk),
                        instances: build(found.chunk, found.pos, cx),
                        last_used: frame,
                    };
                    self.bytes += entry.bytes();
                    self.entries.insert(key.clone(), entry);
                }
            }
            keys.push(key);
        }
        let lists: Vec<&[KeyedInstance]> = keys
            .iter()
            .filter_map(|key| self.entries.get(key))
            .map(|entry| entry.instances.as_slice())
            .collect();
        merge(&lists, out);
    }

    /// Drops chunks not drawn in `frame`, oldest first, while over the budget.
    pub(crate) fn trim(&mut self, frame: u64) {
        if self.bytes <= self.budget_bytes {
            return;
        }
        let mut old: Vec<Aged> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.last_used < frame)
            .map(|(key, entry)| Aged {
                last_used: entry.last_used,
                key: key.clone(),
            })
            .collect();
        old.sort_by_key(|aged| aged.last_used);
        for aged in old {
            if self.bytes <= self.budget_bytes {
                break;
            }
            if let Some(entry) = self.entries.remove(&aged.key) {
                self.bytes = self.bytes.saturating_sub(entry.bytes());
            }
        }
    }
}

/// A cached chunk's key and when it was last drawn.
struct Aged {
    last_used: u64,
    key: ChunkKey,
}

/// A chunk of a layer and where it is.
struct FoundChunk<'a> {
    pos: ChunkPos,
    chunk: &'a Arc<Chunk>,
}

/// The layer's chunks meeting `tiles`: by walking the range or the layer's chunks, whichever is
/// shorter.
fn visible_chunks(layer: &TileLayer, tiles: IRect) -> Vec<FoundChunk<'_>> {
    if tiles.w <= 0 || tiles.h <= 0 {
        return Vec::new();
    }
    let first = ChunkPos::of(tiles.min());
    let last = ChunkPos::of(tiles.max() - IVec2::ONE);
    let across = i64::from(last.x - first.x + 1);
    let down = i64::from(last.y - first.y + 1);
    let range_size = across.saturating_mul(down);
    let in_range =
        |pos: ChunkPos| pos.x >= first.x && pos.x <= last.x && pos.y >= first.y && pos.y <= last.y;
    let layer_chunks = layer.chunks().count() as i64;
    if range_size <= layer_chunks {
        let mut found = Vec::new();
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                let pos = ChunkPos { x, y };
                if let Some(chunk) = layer.chunk(pos) {
                    found.push(FoundChunk { pos, chunk });
                }
            }
        }
        found
    } else {
        layer
            .chunks()
            .filter(|c| in_range(c.pos))
            .map(|c| FoundChunk {
                pos: c.pos,
                chunk: c.chunk,
            })
            .collect()
    }
}

/// A chunk's instances, sorted by draw order.
fn build(chunk: &Chunk, pos: ChunkPos, cx: &ChunkContext<'_>) -> Vec<KeyedInstance> {
    let mut out: Vec<KeyedInstance> = chunk
        .placed_cells(pos)
        .map(|placed| {
            let tileset = placed
                .cell
                .slot()
                .and_then(|slot| cx.map.tilesets.get(slot.index()));
            let sprite = tileset.and_then(|t| cx.sprites.sprite_of(&t.tileset, placed.cell.tile()));
            KeyedInstance {
                key: cx.geometry.draw_order_key(placed.at),
                instance: cell_instance(
                    &cx.geometry,
                    tileset.map(|t| t.tileset.as_ref()),
                    placed.cell,
                    placed.at,
                    sprite,
                ),
            }
        })
        .collect();
    out.sort_by_key(|keyed| keyed.key);
    out
}

/// Merges sorted lists by key, copying runs that stay ahead at once.
fn merge(lists: &[&[KeyedInstance]], out: &mut Vec<Instance>) {
    match lists {
        [] => {}
        [only] => out.extend(only.iter().map(|keyed| keyed.instance)),
        _ => {
            let total: usize = lists.iter().map(|list| list.len()).sum();
            out.reserve(total);
            let mut heap: BinaryHeap<Reverse<Head>> = lists
                .iter()
                .enumerate()
                .filter_map(|(list, items)| {
                    items.first().map(|first| {
                        Reverse(Head {
                            key: first.key,
                            list,
                            at: 0,
                        })
                    })
                })
                .collect();
            while let Some(Reverse(head)) = heap.pop() {
                let Some(items) = lists.get(head.list) else {
                    continue;
                };
                // Copy this list's run up to the next list's head.
                let limit = heap.peek().map(|next| next.0.key);
                let mut at = head.at;
                while let Some(item) = items.get(at) {
                    if limit.is_some_and(|limit| item.key > limit) {
                        break;
                    }
                    out.push(item.instance);
                    at += 1;
                }
                if let Some(next) = items.get(at) {
                    heap.push(Reverse(Head {
                        key: next.key,
                        list: head.list,
                        at,
                    }));
                }
            }
        }
    }
}

/// The next item of one of the merged lists.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Head {
    key: DrawOrderKey,
    list: usize,
    at: usize,
}

/// The cells of a layer at `offset` that may draw into `view` (world pixels): the cells under
/// it, grown by how far the map's tile images reach beyond their cell.
pub(crate) fn tiles_for_view(
    geometry: &MapGeometry,
    map: &Map,
    view: DRect,
    offset: DVec2,
) -> IRect {
    let reach = draw_reach(map);
    let local = view.translated(-offset).expanded(DVec2::splat(reach));
    let tiles = geometry.tiles_in_rect(local);
    // A finite map's cells are all there are.
    if map.infinite {
        tiles
    } else {
        tiles.intersection(IRect::from_min_size(
            IVec2::ZERO,
            IVec2::new(map.width as i32, map.height as i32),
        ))
    }
}

/// How far beyond its cell a tile image of the map's tilesets can reach, in world pixels.
fn draw_reach(map: &Map) -> f64 {
    let grid = f64::from(map.tile_width.max(map.tile_height));
    map.tilesets
        .iter()
        .map(|t| {
            let tileset = &t.tileset;
            let size = f64::from(tileset.tile_width.max(tileset.tile_height));
            let offset = f64::from(tileset.tile_offset.abs().max_element());
            (size - grid).max(0.0) + offset
        })
        .fold(0.0, f64::max)
        + grid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyed(major: i64, minor: i64, tag: u32) -> KeyedInstance {
        KeyedInstance {
            key: DrawOrderKey { major, minor },
            instance: Instance {
                sprite: tag,
                ..Instance::default()
            },
        }
    }

    #[test]
    fn merging_follows_the_keys_across_lists() {
        let a = [
            keyed(0, 0, 1),
            keyed(0, 1, 2),
            keyed(1, 0, 5),
            keyed(2, 0, 7),
        ];
        let b = [keyed(0, 16, 3), keyed(1, 16, 6)];
        let c = [keyed(0, 32, 4), keyed(2, 32, 8)];
        let mut out = Vec::new();
        merge(&[&a, &b, &c], &mut out);
        let order: Vec<u32> = out.iter().map(|i| i.sprite).collect();
        assert_eq!(order, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        out.clear();
        merge(&[&a], &mut out);
        assert_eq!(out.len(), 4);
    }
}
