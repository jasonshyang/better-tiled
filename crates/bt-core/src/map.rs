//! Maps.

use std::sync::Arc;

use bt_utils::IRect;
use glam::{DVec2, IVec2, UVec2};

use crate::cell::Cell;
use crate::color::Color;
use crate::file_format::FileFormat;
use crate::file_ref::FileRef;
use crate::ids::{LayerId, ObjectId, TilesetSlot};
use crate::layer::{EffectiveLayer, Layer, LayerKind};
use crate::object::MapObject;
use crate::property::Properties;
use crate::tile::TileData;
use crate::tileset::Tileset;

/// A map.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    /// The grid's shape.
    pub orientation: Orientation,
    /// The order tiles are drawn in (honoured on orthogonal and oblique maps, as in Tiled).
    pub render_order: RenderOrder,
    /// The width in tiles: the bounds of a finite map.
    pub width: u32,
    /// The height in tiles.
    pub height: u32,
    /// The width of a grid cell in pixels.
    pub tile_width: u32,
    /// The height of a grid cell in pixels.
    pub tile_height: u32,
    /// Whether tile layers extend without bounds; every tile layer agrees.
    pub infinite: bool,
    /// The length of a hexagon's straight side, on hexagonal maps.
    pub hex_side_length: i32,
    /// Which axis is staggered, on staggered and hexagonal maps.
    pub stagger_axis: StaggerAxis,
    /// Which rows or columns are shifted, on staggered and hexagonal maps.
    pub stagger_index: StaggerIndex,
    /// The shear of oblique maps (Tiled 1.12): pixels a row shifts right (`x`) and a column shifts
    /// down (`y`).
    pub skew: IVec2,
    /// The point parallax scrolls around, in screen pixels.
    pub parallax_origin: DVec2,
    /// The colour behind the map.
    pub background_color: Option<Color>,
    /// The class; empty for none.
    pub class: String,
    /// Custom properties.
    pub properties: Properties,
    /// The tilesets, in order; the order decides global ids when the map is written.
    pub tilesets: Vec<TilesetRef>,
    /// The layers, bottom first; groups hold theirs.
    pub layers: Vec<Layer>,
    /// The id the next new layer gets.
    pub next_layer_id: u32,
    /// The id the next new object gets.
    pub next_object_id: u32,
    /// How the map's file was written.
    pub file: FileFormat,
}

impl Default for Map {
    fn default() -> Map {
        Map {
            orientation: Orientation::Orthogonal,
            render_order: RenderOrder::RightDown,
            width: 0,
            height: 0,
            tile_width: 0,
            tile_height: 0,
            infinite: false,
            hex_side_length: 0,
            stagger_axis: StaggerAxis::Y,
            stagger_index: StaggerIndex::Odd,
            skew: IVec2::ZERO,
            parallax_origin: DVec2::ZERO,
            background_color: None,
            class: String::new(),
            properties: Properties::new(),
            tilesets: Vec::new(),
            layers: Vec::new(),
            next_layer_id: 1,
            next_object_id: 1,
            file: FileFormat::default(),
        }
    }
}

/// A tileset a map uses.
#[derive(Clone, Debug, PartialEq)]
pub struct TilesetRef {
    /// The tileset's file, for an external tileset; `None` for one embedded in the map.
    pub file: Option<FileRef>,
    /// The first global id the file gave the tileset: the only trace of global ids in the model,
    /// a hint the writer keeps while it is still valid.
    pub first_gid: Option<u32>,
    /// The tileset, shared with every map that uses its file.
    pub tileset: Arc<Tileset>,
}

impl TilesetRef {
    /// A tileset embedded in the map.
    pub fn embedded(tileset: Arc<Tileset>) -> TilesetRef {
        TilesetRef {
            file: None,
            first_gid: None,
            tileset,
        }
    }
}

/// The shape of a map's grid.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Squares.
    #[default]
    Orthogonal,
    /// Diamonds in a diamond.
    Isometric,
    /// Diamonds in rows or columns shifted against each other.
    Staggered,
    /// Hexagons.
    Hexagonal,
    /// Sheared squares (Tiled 1.12).
    Oblique,
}

/// The order tiles are drawn in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum RenderOrder {
    /// Rows from the top, each from the left.
    #[default]
    RightDown,
    /// Rows from the bottom, each from the left.
    RightUp,
    /// Rows from the top, each from the right.
    LeftDown,
    /// Rows from the bottom, each from the right.
    LeftUp,
}

/// The axis staggered and hexagonal maps shift along.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum StaggerAxis {
    /// Columns are shifted down.
    X,
    /// Rows are shifted right.
    #[default]
    Y,
}

/// Which rows or columns of a staggered or hexagonal map are shifted.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum StaggerIndex {
    /// The odd ones.
    #[default]
    Odd,
    /// The even ones.
    Even,
}

impl Map {
    /// The size of a grid cell.
    pub fn tile_size(&self) -> UVec2 {
        UVec2::new(self.tile_width, self.tile_height)
    }

    /// The tileset in `slot`.
    pub fn tileset(&self, slot: TilesetSlot) -> Option<&Tileset> {
        self.tilesets.get(slot.index()).map(|r| r.tileset.as_ref())
    }

    /// The tileset of a cell's tile.
    pub fn tileset_of(&self, cell: Cell) -> Option<&Tileset> {
        self.tileset(cell.slot()?)
    }

    /// The data of a cell's tile, if its tileset has any.
    pub fn tile_data(&self, cell: Cell) -> Option<&TileData> {
        self.tileset_of(cell)?.tile(cell.tile())
    }

    /// The slot of a tileset, found by identity.
    pub fn slot_of(&self, tileset: &Arc<Tileset>) -> Option<TilesetSlot> {
        self.tilesets
            .iter()
            .position(|r| Arc::ptr_eq(&r.tileset, tileset))
            .and_then(TilesetSlot::new)
    }

    /// Visits every layer depth-first in draw order, parents before their children, with its
    /// depth in the tree (0 at the top level).
    pub fn for_each_layer<'m>(&'m self, mut visit: impl FnMut(&'m Layer, usize)) {
        fn walk<'m>(layers: &'m [Layer], depth: usize, visit: &mut dyn FnMut(&'m Layer, usize)) {
            for layer in layers {
                visit(layer, depth);
                walk(layer.children(), depth + 1, visit);
            }
        }
        walk(&self.layers, 0, &mut visit);
    }

    /// The layer with `id`, anywhere in the tree.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        fn find(layers: &[Layer], id: LayerId) -> Option<&Layer> {
            layers.iter().find_map(|layer| {
                if layer.id == id {
                    Some(layer)
                } else {
                    find(layer.children(), id)
                }
            })
        }
        find(&self.layers, id)
    }

    /// The layer with `id`, anywhere in the tree, to change.
    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        fn find(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
            for layer in layers {
                if layer.id == id {
                    return Some(layer);
                }
                if let Some(children) = layer.children_mut()
                    && let Some(found) = find(children, id)
                {
                    return Some(found);
                }
            }
            None
        }
        find(&mut self.layers, id)
    }

    /// The ids from the top-level group down to the layer with `id`, that layer last.
    pub fn layer_path(&self, id: LayerId) -> Option<Vec<LayerId>> {
        fn find(layers: &[Layer], id: LayerId, path: &mut Vec<LayerId>) -> bool {
            for layer in layers {
                path.push(layer.id);
                if layer.id == id || find(layer.children(), id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        find(&self.layers, id, &mut path).then_some(path)
    }

    /// The layer's attributes combined with its groups'; `None` for an id the map does not have,
    /// which is neither visible nor locked: the caller decides what a missing layer means.
    pub fn effective_layer(&self, id: LayerId) -> Option<EffectiveLayer> {
        fn find(layers: &[Layer], id: LayerId, above: EffectiveLayer) -> Option<EffectiveLayer> {
            layers.iter().find_map(|layer| {
                let here = above.combined_with(layer);
                if layer.id == id {
                    Some(here)
                } else {
                    find(layer.children(), id, here)
                }
            })
        }
        find(&self.layers, id, EffectiveLayer::ROOT)
    }

    /// The object with `id`, on any object layer.
    pub fn object(&self, id: ObjectId) -> Option<&MapObject> {
        let mut found = None;
        self.for_each_layer(|layer, _| {
            if found.is_none()
                && let Some(objects) = layer.as_object_layer()
            {
                found = objects.objects.iter().find(|o| o.id == id);
            }
        });
        found
    }

    /// The object layer holding the object with `id`.
    pub fn object_layer_of(&self, id: ObjectId) -> Option<LayerId> {
        let mut found = None;
        self.for_each_layer(|layer, _| {
            if found.is_none()
                && layer
                    .as_object_layer()
                    .is_some_and(|o| o.objects.iter().any(|o| o.id == id))
            {
                found = Some(layer.id);
            }
        });
        found
    }

    /// The cells the map covers (Tiled's `tileBoundingRect`): its size for a finite map, the
    /// union of the tile layers' chunks for an infinite one, and at least one cell.
    pub fn tile_bounding_rect(&self) -> IRect {
        if !self.infinite {
            let size =
                UVec2::new(self.width, self.height).min(UVec2::splat(i32::MAX.cast_unsigned()));
            return IRect::from_min_size(IVec2::ZERO, size.as_ivec2());
        }
        let mut bounds: Option<IRect> = None;
        self.for_each_layer(|layer, _| {
            if let LayerKind::Tile(tiles) = &layer.kind
                && let Some(chunks) = tiles.chunk_bounds()
            {
                bounds = Some(bounds.map_or(chunks, |b| b.union(chunks)));
            }
        });
        bounds.unwrap_or(IRect::from_min_size(IVec2::ZERO, IVec2::ONE))
    }

    /// The map's rectangle on screen (Tiled's `mapBoundingRect`): what a picture of it covers.
    pub fn bounding_rect(&self) -> IRect {
        self.geometry().bounding_rect(self.tile_bounding_rect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object_layer::ObjectLayer;
    use crate::tile_layer::TileLayer;

    fn object_layer(id: u32, objects: &[u32]) -> Layer {
        let objects = objects
            .iter()
            .map(|&id| MapObject {
                id: ObjectId(id),
                ..MapObject::default()
            })
            .collect();
        Layer::new(
            LayerId(id),
            "objects",
            LayerKind::Object(ObjectLayer {
                objects: Arc::new(objects),
                ..ObjectLayer::default()
            }),
        )
    }

    fn sample() -> Map {
        let inner = Layer {
            visible: false,
            ..object_layer(3, &[7, 8])
        };
        let group = Layer {
            locked: true,
            opacity: 0.5,
            ..Layer::new(LayerId(2), "group", LayerKind::Group(vec![inner]))
        };
        let ground = Layer::new(
            LayerId(1),
            "ground",
            LayerKind::Tile(TileLayer::finite(UVec2::new(4, 3))),
        );
        Map {
            width: 4,
            height: 3,
            tile_width: 16,
            tile_height: 16,
            layers: vec![ground, group],
            ..Map::default()
        }
    }

    #[test]
    fn layers_are_found_anywhere_in_the_tree() {
        let mut map = sample();
        assert_eq!(
            map.layer(LayerId(3)).map(|l| l.name.as_str()),
            Some("objects")
        );
        assert_eq!(map.layer(LayerId(9)), None);
        assert_eq!(
            map.layer_path(LayerId(3)),
            Some(vec![LayerId(2), LayerId(3)])
        );
        assert_eq!(map.layer_path(LayerId(1)), Some(vec![LayerId(1)]));
        assert_eq!(map.layer_path(LayerId(9)), None);
        map.layer_mut(LayerId(3)).unwrap().name = "renamed".into();
        assert_eq!(map.layer(LayerId(3)).unwrap().name, "renamed");
        let mut visited = Vec::new();
        map.for_each_layer(|layer, depth| visited.push((layer.id.0, depth)));
        assert_eq!(visited, [(1, 0), (2, 0), (3, 1)]);
    }

    #[test]
    fn effective_attributes_come_from_the_groups() {
        let map = sample();
        let inner = map.effective_layer(LayerId(3)).unwrap();
        assert!(!inner.visible && inner.locked && inner.opacity == 0.5);
        assert!(
            map.effective_layer(LayerId(1))
                .is_some_and(|l| l.visible && !l.locked)
        );
        assert_eq!(
            map.effective_layer(LayerId(9)),
            None,
            "an unknown layer is neither visible nor locked"
        );
    }

    #[test]
    fn objects_and_bounds() {
        let map = sample();
        assert_eq!(map.object(ObjectId(8)).map(|o| o.id), Some(ObjectId(8)));
        assert_eq!(map.object_layer_of(ObjectId(8)), Some(LayerId(3)));
        assert_eq!(map.object(ObjectId(9)), None);
        assert_eq!(
            map.tile_bounding_rect(),
            IRect {
                x: 0,
                y: 0,
                w: 4,
                h: 3
            }
        );
        assert_eq!(
            map.bounding_rect(),
            IRect {
                x: 0,
                y: 0,
                w: 64,
                h: 48
            }
        );
    }
}
