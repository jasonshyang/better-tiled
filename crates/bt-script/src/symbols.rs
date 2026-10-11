//! The characters `bt view` draws a map with (`10-agents.md`, §10.2), the same in every view of
//! one map, before and after edits: they are given from what the map's tilesets and layers are,
//! not from what one view shows. Terrains whose names say what they are get a hint (`~` water,
//! `"` grass, …), the rest the next free ground symbol in the order of the tilesets' terrains;
//! by tile each tile has one, by layer each layer; objects take capitals, by the first free
//! letter of their type or layer. What is beyond the symbols there are shows as `?`.

use std::collections::HashMap;

use bt_core::{Cell, LayerId, Map, TileId, TilesetSlot};
use bt_terrain::TerrainColor;
use bt_utils::text::slug;

use crate::names::Names;

/// The symbol of an empty cell, or of one that shows only a terrain's edge.
pub(crate) const NOTHING: char = '.';

/// The symbol of what there are no more symbols for.
pub(crate) const TOO_MANY: char = '?';

/// Symbols for terrains whose names say what they are, by the words that say it.
const HINTS: [Hint; 9] = [
    Hint {
        words: &[
            "water", "lake", "sea", "river", "ocean", "pond", "ice", "frozen",
        ],
        symbol: '~',
    },
    Hint {
        words: &["grass", "meadow", "lawn"],
        symbol: '"',
    },
    Hint {
        words: &["sand", "desert", "beach"],
        symbol: ':',
    },
    Hint {
        words: &["dirt", "path", "road", "earth", "mud", "soil"],
        symbol: ',',
    },
    Hint {
        words: &["snow"],
        symbol: '*',
    },
    Hint {
        words: &[
            "wall", "cliff", "platform", "rock", "stone", "brick", "mountain",
        ],
        symbol: '#',
    },
    Hint {
        words: &["lava", "fire"],
        symbol: '%',
    },
    Hint {
        words: &["tree", "forest", "bush", "hedge"],
        symbol: '&',
    },
    Hint {
        words: &["wood", "bridge", "plank", "floor", "deck"],
        symbol: '=',
    },
];

/// The ground symbols given in order to what has no hint: never a hint's sign, `.`, `?` or a
/// capital.
const GROUND: &str = "abcdefghijklmnopqrstuvwxyz0123456789+-/\\|<>^!$;'";

/// The object symbols.
const CAPITALS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Words that name a sign, and the sign.
struct Hint {
    words: &'static [&'static str],
    symbol: char,
}

/// A terrain: a colour of a set of a tileset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TerrainKey {
    pub(crate) slot: TilesetSlot,
    pub(crate) set: usize,
    pub(crate) color: u8,
}

/// A tile, unflipped, in an order: its tileset's slot, then its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct TileKey {
    pub(crate) slot: TilesetSlot,
    pub(crate) id: TileId,
}

impl TileKey {
    /// The tile of a cell, unflipped; `None` for the empty cell.
    pub(crate) fn of(cell: Cell) -> Option<TileKey> {
        Some(TileKey {
            slot: cell.slot()?,
            id: cell.tile(),
        })
    }

    /// The tile as a cell.
    pub(crate) fn cell(self) -> Cell {
        Cell::new(self.slot, self.id)
    }
}

/// What a cell shows in a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Shown {
    /// By terrain: the terrain covering more than half of the cell.
    Terrain(TerrainKey),
    /// By terrain: tiles of a layer that are no terrain's.
    OtherTiles(LayerId),
    /// By tile: a tile, unflipped.
    Tile(TileKey),
    /// By layer: the top layer with a tile there.
    Layer(LayerId),
}

/// The symbols of one map.
#[derive(Clone, Debug, Default)]
pub(crate) struct Symbols {
    ground: HashMap<Shown, char>,
}

impl Symbols {
    /// The symbols of everything `map` can show: its terrains, its tile layers, and its tiles
    /// (every tile of its tilesets when there are few enough, else every tile it uses).
    pub(crate) fn of(map: &Map, names: &Names) -> Symbols {
        let mut taken: Vec<char> = Vec::new();
        let mut ground = HashMap::new();
        // Terrains: hints first, in the order of the tilesets' terrains, then the rest.
        let terrains: Vec<NamedSight> = names
            .terrains()
            .iter()
            .map(|t| NamedSight {
                shown: Shown::Terrain(TerrainKey {
                    slot: t.slot,
                    set: t.set,
                    color: t.color.0,
                }),
                name: t.name.clone(),
            })
            .collect();
        assign_hinted(&terrains, &mut taken, &mut ground);
        assign_rest(&terrains, &mut taken, &mut ground);
        // Layers: their hint, else their first free letter, else the next free symbol; tiles
        // of a layer that are no terrain's take the layer's symbol by terrain too.
        let mut layers: Vec<&bt_core::Layer> = Vec::new();
        map.for_each_layer(|layer, _| {
            if layer.as_tile_layer().is_some() {
                layers.push(layer);
            }
        });
        let mut layer_taken = taken.clone();
        for layer in layers {
            let name = &layer.name;
            let symbol = hint_for(name, &layer_taken)
                .or_else(|| first_free_letter(name, &layer_taken))
                .or_else(|| next_free(GROUND, &layer_taken))
                .unwrap_or(TOO_MANY);
            if symbol != TOO_MANY {
                layer_taken.push(symbol);
            }
            ground.insert(Shown::OtherTiles(layer.id), symbol);
            ground.insert(Shown::Layer(layer.id), symbol);
        }
        // Tiles, in the order of their tilesets and ids.
        let mut tile_taken: Vec<char> = Vec::new();
        for tile in tiles_given_symbols(map) {
            let symbol = next_free(GROUND, &tile_taken).unwrap_or(TOO_MANY);
            if symbol != TOO_MANY {
                tile_taken.push(symbol);
            }
            ground.insert(Shown::Tile(tile), symbol);
        }
        Symbols { ground }
    }

    /// The symbol of what a cell shows.
    pub(crate) fn of_shown(&self, shown: Shown) -> char {
        self.ground.get(&shown).copied().unwrap_or(TOO_MANY)
    }

    /// The symbol of a terrain of a set of a tileset.
    pub(crate) fn of_terrain(&self, slot: TilesetSlot, set: usize, color: TerrainColor) -> char {
        self.of_shown(Shown::Terrain(TerrainKey {
            slot,
            set,
            color: color.0,
        }))
    }
}

/// The tiles that get symbols, in order: every tile of the tilesets when there are no more of
/// them than ground symbols, else the tiles the map's layers use.
fn tiles_given_symbols(map: &Map) -> Vec<TileKey> {
    let every: Vec<TileKey> = map
        .tilesets
        .iter()
        .enumerate()
        .filter_map(|(index, used)| TilesetSlot::new(index).map(|slot| (slot, used)))
        .flat_map(|(slot, used)| {
            used.tileset
                .tile_ids()
                .into_iter()
                .map(move |id| TileKey { slot, id })
        })
        .collect();
    if every.len() <= GROUND.chars().count() {
        return every;
    }
    let mut used: Vec<TileKey> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            for placed in tiles.cells() {
                if let Some(tile) = TileKey::of(placed.cell)
                    && seen.insert(tile)
                {
                    used.push(tile);
                }
            }
        }
    });
    used.sort();
    used
}

/// What a cell can show, with its name.
struct NamedSight {
    shown: Shown,
    name: String,
}

/// Gives each named thing its hint's sign when that is free, in order.
fn assign_hinted(named: &[NamedSight], taken: &mut Vec<char>, symbols: &mut HashMap<Shown, char>) {
    for sight in named {
        if let Some(symbol) = hint_for(&sight.name, taken) {
            taken.push(symbol);
            symbols.insert(sight.shown, symbol);
        }
    }
}

/// Gives each named thing without a symbol the next free ground symbol, in order.
fn assign_rest(named: &[NamedSight], taken: &mut Vec<char>, symbols: &mut HashMap<Shown, char>) {
    for sight in named {
        if symbols.contains_key(&sight.shown) {
            continue;
        }
        let symbol = next_free(GROUND, taken).unwrap_or(TOO_MANY);
        if symbol != TOO_MANY {
            taken.push(symbol);
        }
        symbols.insert(sight.shown, symbol);
    }
}

/// The free sign of the first hint whose words the name holds.
fn hint_for(name: &str, taken: &[char]) -> Option<char> {
    let name = slug(name);
    HINTS
        .iter()
        .find(|hint| {
            !taken.contains(&hint.symbol) && hint.words.iter().any(|word| name.contains(word))
        })
        .map(|hint| hint.symbol)
}

/// The first letter of `name` (lowercase) that is free.
fn first_free_letter(name: &str, taken: &[char]) -> Option<char> {
    name.chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .find(|c| !taken.contains(c))
}

/// The first symbol of `alphabet` that is free.
fn next_free(alphabet: &str, taken: &[char]) -> Option<char> {
    alphabet.chars().find(|c| !taken.contains(c))
}

/// Capital letters for groups of objects, each the first free letter of its word, else the
/// next free capital, else `?`.
pub(crate) fn object_symbols(words: &[String]) -> Vec<char> {
    let mut taken: Vec<char> = Vec::new();
    words
        .iter()
        .map(|word| {
            let symbol = word
                .chars()
                .filter(char::is_ascii_alphabetic)
                .map(|c| c.to_ascii_uppercase())
                .find(|c| !taken.contains(c))
                .or_else(|| next_free(CAPITALS, &taken))
                .unwrap_or(TOO_MANY);
            if symbol != TOO_MANY {
                taken.push(symbol);
            }
            symbol
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder};
    use glam::UVec2;

    use super::*;

    #[test]
    fn terrains_take_their_hints_then_the_next_free_symbols_in_order() {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&[
                "Meadow",
                "Lava",
                "Deep Water",
                "Shallow Water",
                "Ash",
                "Stone Wall",
            ]);
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = Arc::new(set.added_to(Tileset::clone(&atlas.tileset)));
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(tileset)
            .tile_layer("ground", "")
            .tile_layer("walls", "")
            .build()
            .unwrap();
        let names = Names::of(&map);
        let symbols = Symbols::of(&map, &names);
        let slot = TilesetSlot::new(0).unwrap();
        let of = |color: u8| symbols.of_terrain(slot, 0, TerrainColor(color));
        assert_eq!(of(1), '"', "meadow");
        assert_eq!(of(2), '%');
        assert_eq!(of(3), '~', "deep water");
        assert_eq!(of(4), 'a', "the second water takes the next free symbol");
        assert_eq!(of(5), 'b');
        assert_eq!(of(6), '#');
        // Layers take a hint, else their first free letter.
        assert_eq!(symbols.of_shown(Shown::Layer(LayerId(1))), 'g');
        assert_eq!(
            symbols.of_shown(Shown::Layer(LayerId(2))),
            'w',
            "# is the wall terrain's"
        );
        assert_eq!(
            object_symbols(&["spawn".into(), "sprites".into(), "props".into()]),
            ['S', 'P', 'R']
        );
        assert_eq!(
            symbols.of_shown(Shown::Tile(TileKey {
                slot,
                id: TileId(0)
            })),
            'a'
        );
    }
}
