//! Terrain sets for tests, labelled in code: a set of any kind with named terrains, its tiles
//! labelled one by one or with every combination of its terrains, added to a tileset.

use bt_core::{Color, TileId, Tileset, WangColor, WangId, WangSet, WangSetType};

/// The corner lanes (top-right, bottom-right, bottom-left, top-left) as a label.
pub fn corners(c: [u8; 4]) -> WangId {
    WangId::from_lanes([0, c[0], 0, c[1], 0, c[2], 0, c[3]])
}

/// The edge lanes (top, right, bottom, left) as a label.
pub fn edges(e: [u8; 4]) -> WangId {
    WangId::from_lanes([e[0], 0, e[1], 0, e[2], 0, e[3], 0])
}

/// Every combination of `terrains` terrains (1 to `terrains`) on four lanes, in counting order:
/// `[1, 1, 1, 1]`, `[1, 1, 1, 2]`, and so on.
pub fn every_combination(terrains: u8) -> Vec<[u8; 4]> {
    let mut all = Vec::new();
    for a in 1..=terrains {
        for b in 1..=terrains {
            for c in 1..=terrains {
                for d in 1..=terrains {
                    all.push([a, b, c, d]);
                }
            }
        }
    }
    all
}

/// The tile of a set labelled with [`TerrainSetBuilder::every_combination`] that has
/// `combination`, if any.
pub fn combination_tile(terrains: u8, combination: [u8; 4]) -> Option<TileId> {
    let index = every_combination(terrains)
        .iter()
        .position(|c| *c == combination)?;
    u32::try_from(index).ok().map(TileId)
}

/// A terrain set to add to a tileset: named "Ground" until named otherwise.
#[derive(Clone, Debug)]
pub struct TerrainSetBuilder {
    set: WangSet,
}

impl TerrainSetBuilder {
    /// A set of `kind` with no terrains and no labels.
    pub fn new(kind: WangSetType) -> TerrainSetBuilder {
        TerrainSetBuilder {
            set: WangSet {
                name: "Ground".to_owned(),
                class: String::new(),
                kind,
                tile: None,
                colors: Vec::new(),
                tiles: Default::default(),
                properties: Default::default(),
            },
        }
    }

    /// The set's name.
    pub fn name(mut self, name: &str) -> TerrainSetBuilder {
        self.set.name = name.to_owned();
        self
    }

    /// Terrains with these names, weight 1, in order (the first is terrain 1).
    pub fn terrains(mut self, names: &[&str]) -> TerrainSetBuilder {
        self.set.colors = names
            .iter()
            .enumerate()
            .map(|(i, name)| WangColor {
                name: (*name).to_owned(),
                class: String::new(),
                color: {
                    let shade = u8::try_from((40 * i).min(255)).unwrap_or(u8::MAX);
                    Color::rgb(shade, 160, u8::MAX - shade)
                },
                tile: None,
                probability: 1.0,
                properties: Default::default(),
            })
            .collect();
        self
    }

    /// Tile `tile` labelled `label`.
    pub fn label(mut self, tile: u32, label: WangId) -> TerrainSetBuilder {
        self.set.tiles.insert(TileId(tile), label);
        self
    }

    /// Tiles 0, 1, 2… labelled `labels` in order.
    pub fn labels(mut self, labels: impl IntoIterator<Item = WangId>) -> TerrainSetBuilder {
        for (tile, label) in (0..).zip(labels) {
            self.set.tiles.insert(TileId(tile), label);
        }
        self
    }

    /// Tiles 0, 1, 2… labelled with every combination of the set's terrains on its lanes
    /// (corners for a corner set, edges for an edge set), in [`every_combination`]'s order.
    pub fn every_combination(self) -> TerrainSetBuilder {
        let count = u8::try_from(self.set.colors.len()).unwrap_or(u8::MAX);
        let lanes = match self.set.kind {
            WangSetType::Edge => edges,
            WangSetType::Corner | WangSetType::Mixed => corners,
        };
        let labels: Vec<WangId> = every_combination(count).into_iter().map(lanes).collect();
        self.labels(labels)
    }

    /// The set.
    pub fn build(self) -> WangSet {
        self.set
    }

    /// `tileset` with the set added after its others.
    pub fn added_to(self, mut tileset: Tileset) -> Tileset {
        tileset.wang_sets.push(self.set);
        tileset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_labels_tiles_in_counting_order() {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .terrains(&["Sand", "Grass"])
            .every_combination()
            .build();
        assert_eq!(set.tiles.len(), 16);
        assert_eq!(set.tiles[&TileId(0)], corners([1, 1, 1, 1]));
        assert_eq!(set.tiles[&TileId(1)], corners([1, 1, 1, 2]));
        assert_eq!(set.tiles[&TileId(15)], corners([2, 2, 2, 2]));
        assert_eq!(combination_tile(2, [1, 1, 1, 2]), Some(TileId(1)));
        let paths = TerrainSetBuilder::new(WangSetType::Edge)
            .terrains(&["Path", "Grass"])
            .every_combination()
            .build();
        assert_eq!(paths.tiles[&TileId(1)], edges([1, 1, 1, 2]));
    }
}
