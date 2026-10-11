//! The catalog's tiles (`10-agents.md`, §10.4): each tileset's tiles by id with where each is
//! used, layer by layer, the unused ones as ranges and the blank ones (nothing drawn) noted; and
//! for a few tiles named by id, where each is and which tiles the map has next to it on each
//! side, for shores and walls placed by hand.

use std::collections::{BTreeMap, HashSet};

use bt_core::{Cell, Map, PlacedCell, TileId, Tileset, TilesetSlot};
use glam::IVec2;
use serde::{Deserialize, Serialize};

use crate::names::Names;
use crate::symbols::TileKey;
use crate::tally::{NameCount, Tally};
use crate::tile_pictures::TilePictures;

/// The sides of a cell whose neighbours are told, with their words.
const SIDES: [Side; 4] = [
    Side {
        words: "above it",
        offset: IVec2::new(0, -1),
    },
    Side {
        words: "right of it",
        offset: IVec2::new(1, 0),
    },
    Side {
        words: "below it",
        offset: IVec2::new(0, 1),
    },
    Side {
        words: "left of it",
        offset: IVec2::new(-1, 0),
    },
];

/// A side of a cell.
struct Side {
    words: &'static str,
    offset: IVec2,
}

/// A tileset's tiles, with where they are used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TilesetTiles {
    /// The tileset's name.
    pub tileset: String,
    /// How many tiles it has.
    pub tiles: usize,
    /// Its tile size in pixels: `16x16`, or `images of their own` for a collection.
    pub tile_size: String,
    /// How many of its tiles the map uses.
    pub used: usize,
    /// Where its tiles are used, layer by layer: each tile id with its cells.
    pub layers: Vec<LayerTiles>,
    /// The tiles not used, as ranges: `2-4, 6`.
    pub unused: String,
    /// The tiles that draw nothing, as ranges.
    pub blank: String,
    /// Why its images can't be read, when they can't (the blank tiles are then not known).
    pub unreadable: Option<String>,
    /// Its tiles named by their class: `stone = 7, 9-11`.
    pub classes: Vec<String>,
}

/// A layer's tiles of one tileset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerTiles {
    /// The layer.
    pub layer: String,
    /// How many cells hold them.
    pub cells: usize,
    /// Each tile id and its cells, the most first.
    pub tiles: Vec<NameCount>,
}

/// Tiles named by id: where each is and what the map has next to it, and those it lacks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileDetails {
    /// The tiles the map has.
    pub tiles: Vec<TileNeighbours>,
    /// The tiles it does not have, `TILESET:ID`.
    pub unused: Vec<String>,
}

/// A tile named by id: where it is, and what the map has next to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileNeighbours {
    /// The tile, `TILESET:ID`.
    pub tile: String,
    /// Its class; empty for none.
    pub class: String,
    /// The layers it is on, with its cells there.
    pub layers: Vec<NameCount>,
    /// Its cells, row by row.
    pub at: Vec<IVec2>,
    /// What the map has next to it on each side: tiles of its tileset by id, others by name.
    pub sides: Vec<SideTiles>,
}

/// The tiles next to a tile on one side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideTiles {
    /// The side: `above it`, `right of it`, `below it`, `left of it`.
    pub side: String,
    /// The tiles there, the most first; `nothing` for an empty cell.
    pub tiles: Vec<NameCount>,
}

/// How often each tile is used, by layer.
pub(crate) fn tile_uses(map: &Map) -> BTreeMap<TileKey, Tally> {
    let mut uses: BTreeMap<TileKey, Tally> = BTreeMap::new();
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            for placed in tiles.cells() {
                if let Some(tile) = TileKey::of(placed.cell) {
                    uses.entry(tile).or_default().add_one(layer.name.clone());
                }
            }
        }
    });
    uses
}

/// The tiles of the tileset in `slot`, with where they are used.
pub(crate) fn tileset_tiles(
    map: &Map,
    names: &Names,
    slot: TilesetSlot,
    uses: &BTreeMap<TileKey, Tally>,
    pictures: &mut TilePictures,
) -> Option<TilesetTiles> {
    let tileset = map.tileset(slot)?;
    let name = names.tileset_name(slot).unwrap_or("tileset").to_owned();
    let ids = tileset.tile_ids();
    let mut by_layer: BTreeMap<String, Tally> = BTreeMap::new();
    let mut unused: Vec<u32> = Vec::new();
    let mut used = 0;
    for &id in &ids {
        match uses.get(&TileKey { slot, id }) {
            Some(layers) => {
                used += 1;
                for count in layers.sorted() {
                    by_layer
                        .entry(count.name)
                        .or_default()
                        .add(id.0.to_string(), count.count);
                }
            }
            None => unused.push(id.0),
        }
    }
    let layers = by_layer
        .into_iter()
        .map(|(layer, tally)| {
            let tiles = tally.sorted();
            LayerTiles {
                layer,
                cells: tiles.iter().map(|t| t.count).sum(),
                tiles,
            }
        })
        .collect();
    let blanks = blank_tiles(tileset, &name, &ids, pictures);
    let classes = names
        .tile_classes()
        .iter()
        .filter_map(|group| {
            let ids: Vec<u32> = group
                .tiles
                .iter()
                .filter(|t| t.slot() == Some(slot))
                .map(|t| t.tile().0)
                .collect();
            (!ids.is_empty()).then(|| format!("{} = {}", group.name, ranges_text(&ids)))
        })
        .collect();
    Some(TilesetTiles {
        tileset: name,
        tiles: ids.len(),
        tile_size: if tileset.is_collection() {
            "images of their own".to_owned()
        } else {
            format!("{}x{} px", tileset.tile_width, tileset.tile_height)
        },
        used,
        layers,
        unused: ranges_text(&unused),
        blank: ranges_text(&blanks.blank),
        unreadable: blanks.unreadable,
        classes,
    })
}

/// A tileset's tiles that draw nothing.
struct Blanks {
    /// Their ids.
    blank: Vec<u32>,
    /// Why the images can't be read, when they can't.
    unreadable: Option<String>,
}

/// The tiles whose pixels are all see-through.
fn blank_tiles(
    tileset: &Tileset,
    name: &str,
    ids: &[TileId],
    pictures: &mut TilePictures,
) -> Blanks {
    let mut blank = Vec::new();
    for &id in ids {
        match pictures.tile(tileset, name, id) {
            Ok(Some(pixels)) => {
                let drawn = (0..pixels.rect.h).any(|y| {
                    (0..pixels.rect.w).any(|x| pixels.pixel(x, y).is_some_and(|[_, _, _, a]| a > 0))
                });
                if !drawn {
                    blank.push(id.0);
                }
            }
            Ok(None) => {}
            Err(error) => {
                return Blanks {
                    blank: Vec::new(),
                    unreadable: Some(error.to_string()),
                };
            }
        }
    }
    Blanks {
        blank,
        unreadable: None,
    }
}

/// Where each of `tiles` is, and which tiles the map has next to it on each side.
pub(crate) fn tile_neighbours(map: &Map, names: &Names, tiles: &[Cell]) -> TileDetails {
    let wanted: HashSet<TileKey> = tiles.iter().filter_map(|t| TileKey::of(*t)).collect();
    #[derive(Default)]
    struct Seen {
        layers: Tally,
        at: Vec<IVec2>,
        sides: [Tally; 4],
    }
    let mut seen: BTreeMap<TileKey, Seen> = BTreeMap::new();
    map.for_each_layer(|layer, _| {
        let Some(cells) = layer.as_tile_layer() else {
            return;
        };
        let mut found: Vec<PlacedCell> = cells
            .cells()
            .filter(|placed| TileKey::of(placed.cell).is_some_and(|t| wanted.contains(&t)))
            .collect();
        found.sort_by_key(|placed| (placed.at.y, placed.at.x));
        for placed in found {
            let (Some(tile), at) = (TileKey::of(placed.cell), placed.at) else {
                continue;
            };
            let entry = seen.entry(tile).or_default();
            entry.layers.add_one(layer.name.clone());
            entry.at.push(at);
            for (side, tally) in SIDES.iter().zip(entry.sides.iter_mut()) {
                let next = cells.cell(at + side.offset);
                let text = match next.slot() {
                    None => "nothing".to_owned(),
                    Some(slot) if slot == tile.slot => {
                        let flips = crate::objects::flips_text(next.flip());
                        if flips.is_empty() {
                            next.tile().to_string()
                        } else {
                            format!("{} flipped {flips}", next.tile())
                        }
                    }
                    Some(_) => names.tile_text(next),
                };
                tally.add_one(text);
            }
        }
    });
    let mut listed = Vec::new();
    let mut unused = Vec::new();
    for &tile in tiles {
        let text = match tile.slot().and_then(|slot| names.tileset_name(slot)) {
            Some(tileset) => format!("{tileset}:{}", tile.tile()),
            None => names.tile_text(tile),
        };
        let Some(entry) = TileKey::of(tile).and_then(|key| seen.get(&key)) else {
            unused.push(text);
            continue;
        };
        listed.push(TileNeighbours {
            tile: text,
            class: map
                .tile_data(tile)
                .map_or_else(String::new, |data| data.class.clone()),
            layers: entry.layers.sorted(),
            at: entry.at.clone(),
            sides: SIDES
                .iter()
                .zip(&entry.sides)
                .map(|(side, tally)| SideTiles {
                    side: side.words.to_owned(),
                    tiles: tally.sorted(),
                })
                .collect(),
        });
    }
    TileDetails {
        tiles: listed,
        unused,
    }
}

/// Ids as ranges: `2-4, 6, 9-12`.
pub(crate) fn ranges_text(ids: &[u32]) -> String {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut start: Option<u32> = None;
    let mut last = 0u32;
    for id in sorted {
        match start {
            Some(_) if id == last + 1 => {}
            Some(first) => {
                parts.push(range_text(first, last));
                start = Some(id);
            }
            None => start = Some(id),
        }
        last = id;
    }
    if let Some(first) = start {
        parts.push(range_text(first, last));
    }
    parts.join(", ")
}

fn range_text(first: u32, last: u32) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    #[test]
    fn tiles_say_where_they_are_used_and_what_is_next_to_them() {
        let atlas = Atlas::builder().name("shore").build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(Arc::clone(&atlas.tileset))
            .tile_layer("ground", "1 2 2 3\n1 1 . 3h")
            .build()
            .unwrap();
        let names = Names::of(&map);
        let uses = tile_uses(&map);
        let slot = TilesetSlot::new(0).unwrap();
        let tiles = tileset_tiles(&map, &names, slot, &uses, &mut TilePictures::new()).unwrap();
        assert_eq!((tiles.tiles, tiles.used), (16, 3));
        assert_eq!(tiles.unused, "0, 4-15");
        assert_eq!(
            tiles.layers[0].tiles[0],
            NameCount {
                name: "1".into(),
                count: 3
            }
        );
        assert_eq!(tiles.blank, "", "every tile of a drawn atlas is drawn");
        let details = tile_neighbours(
            &map,
            &names,
            &[Cell::new(slot, TileId(3)), Cell::new(slot, TileId(9))],
        );
        assert_eq!(details.unused, ["shore:9"]);
        let three = &details.tiles[0];
        assert_eq!(three.at, [IVec2::new(3, 0), IVec2::new(3, 1)]);
        assert_eq!(
            three.sides[3].tiles,
            [
                NameCount {
                    name: "2".into(),
                    count: 1
                },
                NameCount {
                    name: "nothing".into(),
                    count: 1
                }
            ]
        );
        assert_eq!(
            three.sides[2].tiles[0].name, "3 flipped h",
            "below the first, the second"
        );
        assert_eq!(ranges_text(&[9, 2, 3, 4, 6, 10]), "2-4, 6, 9-10");
    }
}
