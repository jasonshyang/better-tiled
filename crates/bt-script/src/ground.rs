//! What an object stands on (`10-agents.md`, §11), as `check` and `delete objects on` read it:
//! for an object on a layer, the top-most shown tile layer below it that has a tile in the cell
//! decides. A tile a terrain covers more than half of is that terrain ([`Names::terrain_covering`]);
//! a terrain's edge tile looks further down; plain tiles are the tiles of their layer; the bottom
//! tile layer filling nine tenths of the map with one tile is a backdrop (a sky) and counts as
//! nothing.

use std::collections::HashMap;

use bt_core::{Cell, LayerId, Map};
use glam::IVec2;

use crate::names::{NamedTerrain, Names};
use crate::shapes::map_cells;

/// What an object stands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StandsOn {
    /// A terrain: the one covering more than half of the tile under it.
    Terrain(NamedTerrain),
    /// Plain tiles of a layer, by its name.
    Tiles(String),
    /// Nothing: no tile below, only terrains' edges, or a backdrop.
    Nothing,
}

impl StandsOn {
    /// What it stands on, in words: a terrain's name, `tiles of LAYER`, `nothing`.
    pub fn text(&self) -> String {
        match self {
            StandsOn::Terrain(terrain) => terrain.name.clone(),
            StandsOn::Tiles(layer) => format!("tiles of {layer}"),
            StandsOn::Nothing => "nothing".to_owned(),
        }
    }
}

/// Reads what objects of a map stand on, keeping what it measured (which tile layers are
/// backdrops) for the next question.
#[derive(Debug)]
pub struct GroundUnder<'m> {
    map: &'m Map,
    names: &'m Names,
    /// Every layer in draw order, bottom first.
    order: Vec<LayerId>,
    /// Whether a tile fills nine tenths of the bottom tile layer, by layer and tile.
    backdrops: HashMap<BackdropKey, bool>,
}

/// A tile on a layer, for the backdrop measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct BackdropKey {
    layer: LayerId,
    tile: Cell,
}

impl<'m> GroundUnder<'m> {
    /// A reader of `map`'s grounds, naming terrains with `names`.
    pub fn new(map: &'m Map, names: &'m Names) -> GroundUnder<'m> {
        GroundUnder {
            map,
            names,
            order: bt_edit::layer_order(map),
            backdrops: HashMap::new(),
        }
    }

    /// What an object on the layer `layer` standing in `cell` stands on. A layer the map lacks
    /// is taken as the top of the map, so every tile layer is below it.
    pub fn stands_on(&mut self, layer: LayerId, cell: IVec2) -> StandsOn {
        let index = self
            .order
            .iter()
            .position(|&id| id == layer)
            .unwrap_or(self.order.len());
        let below: Vec<LayerId> = self.order.iter().take(index).copied().collect();
        let shown_tile_layers: Vec<LayerId> = below
            .into_iter()
            .filter(|&id| {
                self.map
                    .layer(id)
                    .is_some_and(|l| l.as_tile_layer().is_some())
                    && self.map.effective_layer(id).is_some_and(|l| l.visible)
            })
            .collect();
        let bottom = shown_tile_layers.first().copied();
        for &id in shown_tile_layers.iter().rev() {
            let Some(layer) = self.map.layer(id) else {
                continue;
            };
            let Some(tiles) = layer.as_tile_layer() else {
                continue;
            };
            let tile = tiles.cell(cell);
            if tile.is_empty() {
                continue;
            }
            if let Some(terrain) = self.names.terrain_covering(self.map, tile) {
                return StandsOn::Terrain(terrain.clone());
            }
            if has_terrain(self.map, tile) {
                // A terrain's edge: what shows there is further down.
                continue;
            }
            if Some(id) == bottom && self.is_backdrop(id, tile) {
                return StandsOn::Nothing;
            }
            return StandsOn::Tiles(layer.name.clone());
        }
        StandsOn::Nothing
    }

    /// The top-most tile of the shown tile layers below `layer` in `cell`, with the layer it is
    /// on; none when every one is empty there.
    pub fn top_tile_below(&self, layer: LayerId, cell: IVec2) -> Option<Cell> {
        let index = self
            .order
            .iter()
            .position(|&id| id == layer)
            .unwrap_or(self.order.len());
        self.order
            .iter()
            .take(index)
            .rev()
            .filter(|&&id| self.map.effective_layer(id).is_some_and(|l| l.visible))
            .filter_map(|&id| self.map.layer(id).and_then(|l| l.as_tile_layer()))
            .map(|tiles| tiles.cell(cell))
            .find(|tile| !tile.is_empty())
    }

    /// Whether `tile` fills nine tenths of the map's cells on the layer `layer`.
    fn is_backdrop(&mut self, layer: LayerId, tile: Cell) -> bool {
        let key = BackdropKey { layer, tile };
        if let Some(&known) = self.backdrops.get(&key) {
            return known;
        }
        let area = map_cells(self.map);
        let cells = i64::from(area.w.max(0)) * i64::from(area.h.max(0));
        let same = self
            .map
            .layer(layer)
            .and_then(|l| l.as_tile_layer())
            .map_or(0, |tiles| {
                tiles.cells().filter(|placed| placed.cell == tile).count()
            });
        let same = i64::try_from(same).unwrap_or(i64::MAX);
        let backdrop = cells > 0 && same.saturating_mul(10) >= cells.saturating_mul(9);
        self.backdrops.insert(key, backdrop);
        backdrop
    }
}

/// Whether a terrain set of the tile's tileset labels it.
fn has_terrain(map: &Map, tile: Cell) -> bool {
    map.tileset_of(tile).is_some_and(|tileset| {
        tileset
            .wang_sets
            .iter()
            .any(|set| set.tiles.contains_key(&tile.tile()))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder};
    use glam::UVec2;

    use super::*;

    /// A 4×4 map: a sky of tile 0 on the bottom layer (not a terrain tile: the atlas has more
    /// tiles than the set labels), water on the ground layer at the left column, grass/water
    /// edges at the second, and objects above.
    fn map() -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder()
            .name("ground")
            .grid(8, 4)
            .build_embedded()
            .unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(4, 4))
            .tileset(Arc::new(tileset))
            .tile_layer("sky", "20 20 20 20\n20 20 20 20\n20 20 20 20\n20 20 20 21")
            .tile_layer("ground", "15 5 . 0\n15 5 . 0\n15 5 . 0\n15 5 . 0")
            .object_layer("things", Vec::new())
            .build()
            .unwrap()
    }

    #[test]
    fn objects_stand_on_the_terrain_covering_most_of_the_tile_below() {
        let map = map();
        let names = Names::of(&map);
        let mut ground = GroundUnder::new(&map, &names);
        let things = LayerId(3);
        let on =
            |ground: &mut GroundUnder<'_>, x, y| ground.stands_on(things, IVec2::new(x, y)).text();
        assert_eq!(on(&mut ground, 0, 0), "water");
        // Tile 5 is grass at two corners and water at two: an edge, so what is below counts,
        // and the sky fills the map: nothing.
        assert_eq!(on(&mut ground, 1, 0), "nothing");
        assert_eq!(on(&mut ground, 2, 0), "nothing", "the sky is a backdrop");
        assert_eq!(on(&mut ground, 3, 0), "grass");
        // The ground layer itself sees only the sky under it.
        assert_eq!(
            ground.stands_on(LayerId(2), IVec2::new(0, 0)),
            StandsOn::Nothing
        );
        assert_eq!(
            ground
                .top_tile_below(things, IVec2::new(2, 3))
                .map(|c| c.tile().0),
            Some(20),
            "the ground layer is empty there"
        );
        assert_eq!(
            ground
                .top_tile_below(things, IVec2::new(3, 3))
                .map(|c| c.tile().0),
            Some(0)
        );
    }

    #[test]
    fn plain_tiles_are_named_by_their_layer_unless_a_backdrop() {
        let atlas = Atlas::builder().name("plain").build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(2, 2))
            .tileset(atlas.tileset)
            .tile_layer("floor", "1 1\n2 .")
            .object_layer("things", Vec::new())
            .build()
            .unwrap();
        let names = Names::of(&map);
        let mut ground = GroundUnder::new(&map, &names);
        assert_eq!(
            ground.stands_on(LayerId(2), IVec2::new(0, 0)),
            StandsOn::Tiles("floor".into())
        );
        assert_eq!(
            ground.stands_on(LayerId(2), IVec2::new(1, 1)),
            StandsOn::Nothing
        );
    }
}
