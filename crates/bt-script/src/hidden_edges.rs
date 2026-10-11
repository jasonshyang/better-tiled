//! Opaque edges over other ground (`10-agents.md`, §11): a terrain whose edge tiles draw their own
//! ground where it ends (a frozen lake whose edges show snow) hides what is under those edges, so
//! its edge tiles belong over whole ground. `check` lists the edge tiles of such terrains whose
//! top-most tile below, on the shown tile layers under theirs, is a terrain's edge, plain tiles
//! or nothing, per layer, terrain and what is under them; the edit report lists the new ones.

use std::collections::{BTreeMap, HashMap};

use bt_core::{Cell, Lane, LayerId, Map, TilesetSlot};
use bt_terrain::TerrainColor;
use glam::IVec2;

use crate::names::{NamedTerrain, Names};
use crate::opacity::{EdgeMeter, EdgeOpacity};

/// Edge tiles of one terrain on one layer over one kind of ground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HiddenEdges {
    /// The layer the edge tiles are on.
    pub(crate) layer: String,
    /// The terrain whose edges they are.
    pub(crate) terrain: String,
    /// What is under them: `nothing`, `tiles of LAYER`, or a terrain's edges in words.
    pub(crate) under: String,
    /// Their cells, row by row.
    pub(crate) cells: Vec<IVec2>,
}

/// A terrain of a set of a tileset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TerrainKey {
    slot: TilesetSlot,
    set: usize,
    color: TerrainColor,
}

/// Where a group of hidden edges is: the layer, the terrain and what is under them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GroupKey {
    order: usize,
    layer: String,
    terrain: String,
    under: String,
}

/// The tile below a cell, on the top-most shown tile layer under a layer that has one there.
#[derive(Clone, Copy, Debug)]
struct TileBelow {
    layer: LayerId,
    tile: Cell,
}

/// The opaque edge tiles of `map` lying over anything but whole ground, measuring terrains'
/// edges with `meter`, top layer first.
pub(crate) fn hidden_edges(map: &Map, names: &Names, meter: &mut EdgeMeter) -> Vec<HiddenEdges> {
    let shown: Vec<LayerId> = bt_edit::layer_order(map)
        .into_iter()
        .filter(|&id| {
            map.layer(id).is_some_and(|l| l.as_tile_layer().is_some())
                && map.effective_layer(id).is_some_and(|l| l.visible)
        })
        .collect();
    let mut opaque: HashMap<TerrainKey, bool> = HashMap::new();
    let mut groups: BTreeMap<GroupKey, Vec<IVec2>> = BTreeMap::new();
    for (index, &id) in shown.iter().enumerate() {
        let Some(layer) = map.layer(id) else {
            continue;
        };
        let Some(tiles) = layer.as_tile_layer() else {
            continue;
        };
        for placed in tiles.cells() {
            let Some(key) = edge_terrain(map, placed.cell) else {
                continue;
            };
            let Some(terrain) = names.terrain_of(key.slot, key.set, key.color) else {
                continue;
            };
            let is_opaque = *opaque
                .entry(key)
                .or_insert_with(|| meter.measure(map, terrain) == EdgeOpacity::Opaque);
            if !is_opaque {
                continue;
            }
            let below = tile_below(map, shown.get(..index).unwrap_or(&[]), placed.at);
            let Some(under) = ground_under(map, names, below) else {
                continue;
            };
            groups
                .entry(GroupKey {
                    order: shown.len() - index,
                    layer: layer.name.clone(),
                    terrain: terrain_name(terrain),
                    under,
                })
                .or_default()
                .push(placed.at);
        }
    }
    groups
        .into_iter()
        .map(|(key, mut cells)| {
            cells.sort_by_key(|c| (c.y, c.x));
            HiddenEdges {
                layer: key.layer,
                terrain: key.terrain,
                under: key.under,
                cells,
            }
        })
        .collect()
}

/// The terrain a tile is an edge of: in the first of its tileset's terrain sets that labels it,
/// one terrain on some of the lanes the set uses and nothing on the others.
fn edge_terrain(map: &Map, tile: Cell) -> Option<TerrainKey> {
    let slot = tile.slot()?;
    let tileset = map.tileset(slot)?;
    tileset
        .wang_sets
        .iter()
        .enumerate()
        .find_map(|(index, set)| {
            let label = set.tiles.get(&tile.tile())?;
            let lanes: Vec<u8> = Lane::ALL
                .into_iter()
                .filter(|&lane| set.kind.uses(lane))
                .map(|lane| label.lane(lane))
                .collect();
            let color = lanes.iter().copied().find(|&color| color != 0)?;
            let edge = lanes.contains(&0) && lanes.iter().all(|&c| c == 0 || c == color);
            edge.then_some(TerrainKey {
                slot,
                set: index,
                color: TerrainColor(color),
            })
        })
}

/// Whether a tile is one terrain on every lane its set uses.
fn is_whole(map: &Map, tile: Cell) -> bool {
    let Some(tileset) = tile.slot().and_then(|slot| map.tileset(slot)) else {
        return false;
    };
    tileset.wang_sets.iter().any(|set| {
        set.tiles.get(&tile.tile()).is_some_and(|label| {
            let mut lanes = Lane::ALL
                .into_iter()
                .filter(|&lane| set.kind.uses(lane))
                .map(|lane| label.lane(lane));
            lanes
                .next()
                .is_some_and(|first| first != 0 && lanes.all(|c| c == first))
        })
    })
}

/// The top-most tile in `cell` of the layers `below` (bottom first).
fn tile_below(map: &Map, below: &[LayerId], cell: IVec2) -> Option<TileBelow> {
    below.iter().rev().find_map(|&id| {
        let tile = map.layer(id)?.as_tile_layer()?.cell(cell);
        (!tile.is_empty()).then_some(TileBelow { layer: id, tile })
    })
}

/// What an edge tile lies over when it is not whole ground: `nothing`, `tiles of LAYER`, or a
/// terrain's edges in words; none over whole ground.
fn ground_under(map: &Map, names: &Names, below: Option<TileBelow>) -> Option<String> {
    let Some(below) = below else {
        return Some("nothing".to_owned());
    };
    if is_whole(map, below.tile) {
        return None;
    }
    if names.terrain_covering(map, below.tile).is_some() || has_terrain(map, below.tile) {
        return Some(names.tile_label(map, below.tile));
    }
    let layer = map
        .layer(below.layer)
        .map_or_else(String::new, |l| l.name.clone());
    Some(format!("tiles of {layer}"))
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

/// A terrain's name as reports write it.
fn terrain_name(terrain: &NamedTerrain) -> String {
    terrain.name.clone()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder};
    use glam::UVec2;

    use super::*;

    /// A ground layer of `ground` rows under an `ice` layer of `ice` rows, the ice's edges
    /// opaque: the atlas draws every tile solid.
    fn map(ground: &str, ice: &str) -> Map {
        let land = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let frost = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Frost")
            .terrains(&["Ice"])
            .label(16, bt_test_data::fixtures::corners([1, 1, 1, 1]))
            .label(17, bt_test_data::fixtures::corners([1, 1, 0, 0]))
            .label(18, bt_test_data::fixtures::corners([0, 0, 1, 1]));
        let atlas = Atlas::builder()
            .name("ground")
            .grid(8, 4)
            .build_embedded()
            .unwrap();
        let tileset = frost.added_to(land.added_to(Tileset::clone(&atlas.tileset)));
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", ground)
            .tile_layer("ice", ice)
            .build()
            .unwrap()
    }

    #[test]
    fn opaque_edges_over_whole_ground_are_fine_and_over_anything_else_are_listed() {
        let grass = bt_test_data::fixtures::combination_tile(2, [1, 1, 1, 1])
            .unwrap()
            .0;
        let edge = bt_test_data::fixtures::combination_tile(2, [1, 1, 2, 2])
            .unwrap()
            .0;
        let ground = format!("{grass} {grass} {edge} .\n{grass} {grass} {grass} {grass}");
        let shown = map(&ground, "17 17 17 17\n16 18 . .");
        let names = Names::of(&shown);
        let mut meter = EdgeMeter::new();
        let found = hidden_edges(&shown, &names, &mut meter);
        let summary: Vec<String> = found
            .iter()
            .map(|h| format!("{} {} over {}: {:?}", h.layer, h.terrain, h.under, h.cells))
            .collect();
        assert_eq!(
            summary,
            [
                "ice ice over grass/water edges: [IVec2(2, 0)]",
                "ice ice over nothing: [IVec2(3, 0)]",
            ],
            "edges over whole grass are fine, the whole ice tile is no edge"
        );
    }
}
