//! The invariants the model cannot enforce in a constructor, stated as checks: loaders report
//! what they find as warnings, tests assert there is nothing.

use std::collections::HashSet;
use std::fmt;

use crate::ids::{LayerId, ObjectId, TileId, TilesetSlot};
use crate::layer::{Layer, LayerKind};
use crate::map::Map;
use crate::tile_layer::TileBounds;
use crate::wang::WangSet;

/// An invariant a map breaks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Violation {
    /// Two layers have this id.
    DuplicateLayerId(LayerId),
    /// Two objects have this id.
    DuplicateObjectId(ObjectId),
    /// The next layer id is not above every layer id.
    NextLayerIdTooLow {
        /// The next id the map holds.
        next: u32,
        /// The highest id in use.
        highest: u32,
    },
    /// The next object id is not above every object id.
    NextObjectIdTooLow {
        /// The next id the map holds.
        next: u32,
        /// The highest id in use.
        highest: u32,
    },
    /// Cells of a layer name a tileset the map does not have.
    UnknownTileset {
        /// The layer.
        layer: LayerId,
        /// The missing tileset's slot.
        slot: TilesetSlot,
        /// How many cells.
        cells: usize,
    },
    /// Cells of a layer name tiles past the end of their tileset (kept, as Tiled keeps them).
    TilesOutOfRange {
        /// The layer.
        layer: LayerId,
        /// The tileset.
        slot: TilesetSlot,
        /// How many cells.
        cells: usize,
    },
    /// A tile layer's bounds do not match the map's `infinite`.
    BoundsMismatch(LayerId),
    /// Cells of a finite layer lie outside its size.
    CellsOutsideBounds {
        /// The layer.
        layer: LayerId,
        /// How many cells.
        cells: usize,
    },
    /// A terrain set holds more colours than a lane can name.
    TooManyColors {
        /// The tileset.
        slot: TilesetSlot,
        /// The set's name.
        set: String,
    },
    /// A tile's lane names a colour the set does not have.
    UnknownColor {
        /// The tileset.
        slot: TilesetSlot,
        /// The set's name.
        set: String,
        /// The tile.
        tile: TileId,
    },
    /// An object says which template fields it overrides but has no template.
    OverridesWithoutTemplate(ObjectId),
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Violation::DuplicateLayerId(id) => write!(f, "two layers have the id {id}"),
            Violation::DuplicateObjectId(id) => write!(f, "two objects have the id {id}"),
            Violation::NextLayerIdTooLow { next, highest } => write!(
                f,
                "the next layer id {next} is not above the highest in use, {highest}"
            ),
            Violation::NextObjectIdTooLow { next, highest } => write!(
                f,
                "the next object id {next} is not above the highest in use, {highest}"
            ),
            Violation::UnknownTileset { layer, slot, cells } => write!(
                f,
                "layer {layer}: {cells} cells use tileset {slot}, which the map does not have"
            ),
            Violation::TilesOutOfRange { layer, slot, cells } => write!(
                f,
                "layer {layer}: {cells} cells use tiles past the end of tileset {slot}"
            ),
            Violation::BoundsMismatch(layer) => {
                write!(f, "layer {layer} is not as infinite as its map")
            }
            Violation::CellsOutsideBounds { layer, cells } => {
                write!(f, "layer {layer}: {cells} cells lie outside the layer")
            }
            Violation::TooManyColors { slot, set } => write!(
                f,
                "tileset {slot}: terrain set “{set}” has more than {} terrains",
                WangSet::MAX_COLORS
            ),
            Violation::UnknownColor { slot, set, tile } => write!(
                f,
                "tileset {slot}: tile {tile} of terrain set “{set}” names a terrain the set does not have"
            ),
            Violation::OverridesWithoutTemplate(id) => write!(
                f,
                "object {id} overrides template fields but has no template"
            ),
        }
    }
}

impl Map {
    /// The invariants the map breaks, in a fixed order; none for a well-formed map.
    pub fn validate(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        self.validate_ids(&mut out);
        self.for_each_layer(|layer, _| self.validate_layer(layer, &mut out));
        for (index, tileset) in self.tilesets.iter().enumerate() {
            let Some(slot) = TilesetSlot::new(index) else {
                continue;
            };
            for set in &tileset.tileset.wang_sets {
                validate_wang_set(slot, set, &mut out);
            }
        }
        out
    }

    fn validate_ids(&self, out: &mut Vec<Violation>) {
        let mut layer_ids = HashSet::new();
        let mut object_ids = HashSet::new();
        let (mut highest_layer, mut highest_object) = (0, 0);
        self.for_each_layer(|layer, _| {
            if !layer_ids.insert(layer.id) {
                out.push(Violation::DuplicateLayerId(layer.id));
            }
            highest_layer = highest_layer.max(layer.id.0);
            for object in layer
                .as_object_layer()
                .map(|o| o.objects.as_slice())
                .unwrap_or_default()
            {
                if !object_ids.insert(object.id) {
                    out.push(Violation::DuplicateObjectId(object.id));
                }
                highest_object = highest_object.max(object.id.0);
                if object.template.is_none() && !object.overrides.is_empty() {
                    out.push(Violation::OverridesWithoutTemplate(object.id));
                }
            }
        });
        if !layer_ids.is_empty() && self.next_layer_id <= highest_layer {
            out.push(Violation::NextLayerIdTooLow {
                next: self.next_layer_id,
                highest: highest_layer,
            });
        }
        if !object_ids.is_empty() && self.next_object_id <= highest_object {
            out.push(Violation::NextObjectIdTooLow {
                next: self.next_object_id,
                highest: highest_object,
            });
        }
    }

    fn validate_layer(&self, layer: &Layer, out: &mut Vec<Violation>) {
        let LayerKind::Tile(tiles) = &layer.kind else {
            return;
        };
        let expected = if self.infinite {
            TileBounds::Infinite
        } else {
            TileBounds::Finite
        };
        if tiles.bounds() != expected {
            out.push(Violation::BoundsMismatch(layer.id));
        }
        let mut unknown = std::collections::BTreeMap::<TilesetSlot, usize>::new();
        let mut out_of_range = std::collections::BTreeMap::<TilesetSlot, usize>::new();
        let mut outside = 0;
        for placed in tiles.cells() {
            if !tiles.contains(placed.at) {
                outside += 1;
            }
            let Some(slot) = placed.cell.slot() else {
                continue;
            };
            match self.tileset(slot) {
                None => *unknown.entry(slot).or_default() += 1,
                Some(tileset) if !tileset.has_tile(placed.cell.tile()) => {
                    *out_of_range.entry(slot).or_default() += 1
                }
                Some(_) => {}
            }
        }
        out.extend(
            unknown
                .into_iter()
                .map(|(slot, cells)| Violation::UnknownTileset {
                    layer: layer.id,
                    slot,
                    cells,
                }),
        );
        out.extend(
            out_of_range
                .into_iter()
                .map(|(slot, cells)| Violation::TilesOutOfRange {
                    layer: layer.id,
                    slot,
                    cells,
                }),
        );
        if outside > 0 {
            out.push(Violation::CellsOutsideBounds {
                layer: layer.id,
                cells: outside,
            });
        }
    }
}

fn validate_wang_set(slot: TilesetSlot, set: &WangSet, out: &mut Vec<Violation>) {
    if set.colors.len() > WangSet::MAX_COLORS {
        out.push(Violation::TooManyColors {
            slot,
            set: set.name.clone(),
        });
    }
    for (&tile, id) in &set.tiles {
        if id
            .lanes()
            .iter()
            .any(|&lane| usize::from(lane) > set.colors.len())
        {
            out.push(Violation::UnknownColor {
                slot,
                set: set.name.clone(),
                tile,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::{IVec2, UVec2};

    use super::*;
    use crate::cell::Cell;
    use crate::color::Color;
    use crate::file_ref::FileRef;
    use crate::map::TilesetRef;
    use crate::object::{MapObject, ObjectOverrides};
    use crate::object_layer::ObjectLayer;
    use crate::tile_layer::TileLayer;
    use crate::tileset::Tileset;
    use crate::wang::{WangColor, WangId, WangSetType};

    fn objects(id: u32, ids: &[u32]) -> Layer {
        let objects = ids
            .iter()
            .map(|&id| MapObject {
                id: ObjectId(id),
                ..MapObject::default()
            })
            .collect();
        Layer::new(
            LayerId(id),
            "o",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(objects),
                ..ObjectLayer::default()
            }),
        )
    }

    #[test]
    fn a_well_formed_map_breaks_nothing() {
        let map = Map {
            layers: vec![objects(1, &[1, 2])],
            next_layer_id: 2,
            next_object_id: 3,
            ..Map::default()
        };
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn ids_must_be_unique_and_below_the_next_ones() {
        let mut stray = objects(1, &[4]);
        if let LayerKind::Object(layer) = &mut stray.kind {
            Arc::make_mut(&mut layer.objects).push(MapObject {
                id: ObjectId(5),
                overrides: ObjectOverrides::NAME,
                ..MapObject::default()
            });
        }
        let map = Map {
            layers: vec![objects(1, &[1, 4]), stray],
            next_layer_id: 1,
            next_object_id: 4,
            ..Map::default()
        };
        assert_eq!(
            map.validate(),
            [
                Violation::DuplicateLayerId(LayerId(1)),
                Violation::DuplicateObjectId(ObjectId(4)),
                Violation::OverridesWithoutTemplate(ObjectId(5)),
                Violation::NextLayerIdTooLow {
                    next: 1,
                    highest: 1
                },
                Violation::NextObjectIdTooLow {
                    next: 4,
                    highest: 5
                },
            ]
        );
    }

    #[test]
    fn cells_must_name_tilesets_and_tiles_of_the_map() {
        let image =
            crate::image_ref::ImageRef::file(FileRef::new("a.png", "/a.png"), UVec2::new(32, 16));
        let atlas = Tileset {
            tile_width: 16,
            tile_height: 16,
            image: crate::tileset::TilesetImage::Atlas {
                image,
                columns: 2,
                tile_count: 2,
            },
            ..Tileset::default()
        };
        let mut layer = TileLayer::finite(UVec2::new(4, 4));
        let slot = |i| TilesetSlot::new(i).unwrap();
        layer
            .set_cell(IVec2::new(0, 0), Cell::new(slot(0), TileId(1)))
            .unwrap();
        layer
            .set_cell(IVec2::new(1, 0), Cell::new(slot(0), TileId(5)))
            .unwrap();
        layer
            .set_cell(IVec2::new(2, 0), Cell::new(slot(3), TileId(0)))
            .unwrap();
        let map = Map {
            infinite: true,
            tilesets: vec![TilesetRef::embedded(Arc::new(atlas))],
            layers: vec![Layer::new(LayerId(1), "t", LayerKind::Tile(layer))],
            next_layer_id: 2,
            ..Map::default()
        };
        assert_eq!(
            map.validate(),
            [
                Violation::BoundsMismatch(LayerId(1)),
                Violation::UnknownTileset {
                    layer: LayerId(1),
                    slot: slot(3),
                    cells: 1
                },
                Violation::TilesOutOfRange {
                    layer: LayerId(1),
                    slot: slot(0),
                    cells: 1
                },
            ]
        );
    }

    #[test]
    fn terrain_lanes_must_name_colours_of_their_set() {
        let color = WangColor {
            name: "grass".into(),
            class: String::new(),
            color: Color::BLACK,
            tile: None,
            probability: 1.0,
            properties: Default::default(),
        };
        let set = WangSet {
            name: "ground".into(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: None,
            colors: vec![color],
            tiles: [
                (TileId(0), WangId::from_lanes([0, 1, 0, 1, 0, 1, 0, 1])),
                (TileId(1), WangId::from_lanes([0, 2, 0, 1, 0, 1, 0, 1])),
            ]
            .into(),
            properties: Default::default(),
        };
        let tileset = Tileset {
            wang_sets: vec![set],
            ..Tileset::default()
        };
        let map = Map {
            tilesets: vec![TilesetRef::embedded(Arc::new(tileset))],
            ..Map::default()
        };
        let violations = map.validate();
        assert_eq!(
            violations,
            [Violation::UnknownColor {
                slot: TilesetSlot::new(0).unwrap(),
                set: "ground".into(),
                tile: TileId(1)
            }]
        );
        assert_eq!(
            violations[0].to_string(),
            "tileset 0: tile 1 of terrain set “ground” names a terrain the set does not have"
        );
    }
}
