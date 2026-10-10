//! Maps made of fixture tilesets and layers.

use std::sync::Arc;

use bt_core::{
    Color, ImageLayer, Layer, LayerId, LayerKind, Map, MapObject, ObjectId, ObjectLayer,
    Orientation, RenderOrder, StaggerAxis, StaggerIndex, TileLayer, Tileset, TilesetRef,
};
use glam::{IVec2, UVec2};

use super::error::FixtureError;
use super::grid::grid_cells;

/// Sets up a [`Map`]: its grid, its tilesets (embedded, in the order given) and its layers
/// (bottom first, ids from 1 in the order added).
#[derive(Clone, Debug)]
pub struct MapBuilder {
    map: Map,
    error: Option<FixtureError>,
}

impl MapBuilder {
    /// An orthogonal, finite map `size` cells across and down, of `tile` pixels each.
    pub fn new(tile: UVec2, size: UVec2) -> MapBuilder {
        MapBuilder {
            map: Map {
                tile_width: tile.x,
                tile_height: tile.y,
                width: size.x,
                height: size.y,
                ..Map::default()
            },
            error: None,
        }
    }

    /// The grid's orientation.
    pub fn orientation(mut self, orientation: Orientation) -> MapBuilder {
        self.map.orientation = orientation;
        self
    }

    /// The order tiles are drawn in on orthogonal and oblique maps.
    pub fn render_order(mut self, order: RenderOrder) -> MapBuilder {
        self.map.render_order = order;
        self
    }

    /// Which rows or columns are shifted on staggered and hexagonal maps.
    pub fn stagger(mut self, axis: StaggerAxis, index: StaggerIndex) -> MapBuilder {
        self.map.stagger_axis = axis;
        self.map.stagger_index = index;
        self
    }

    /// The length of a hexagon's straight side.
    pub fn hex_side_length(mut self, length: i32) -> MapBuilder {
        self.map.hex_side_length = length;
        self
    }

    /// The skew of an oblique map.
    pub fn skew(mut self, skew: IVec2) -> MapBuilder {
        self.map.skew = skew;
        self
    }

    /// Makes the map infinite: tile layers added after this have no bounds.
    pub fn infinite(mut self) -> MapBuilder {
        self.map.infinite = true;
        self
    }

    /// The colour behind the layers.
    pub fn background(mut self, color: Color) -> MapBuilder {
        self.map.background_color = Some(color);
        self
    }

    /// Adds a tileset, embedded; its slot is the number of tilesets added before it.
    pub fn tileset(mut self, tileset: Arc<Tileset>) -> MapBuilder {
        self.map.tilesets.push(TilesetRef::embedded(tileset));
        self
    }

    /// Adds a tile layer holding an ASCII grid's cells (see [`super::grid_cells`]) from (0, 0).
    pub fn tile_layer(self, name: &str, grid: &str) -> MapBuilder {
        self.tile_layer_at(name, IVec2::ZERO, grid)
    }

    /// Adds a tile layer holding an ASCII grid's cells with the grid's top-left at `origin`.
    pub fn tile_layer_at(mut self, name: &str, origin: IVec2, grid: &str) -> MapBuilder {
        let size = UVec2::new(self.map.width, self.map.height);
        let mut layer = if self.map.infinite {
            TileLayer::infinite(size)
        } else {
            TileLayer::finite(size)
        };
        match grid_cells(grid) {
            Ok(cells) => {
                for placed in cells {
                    let at = origin + placed.at;
                    if layer.set_cell(at, placed.cell).is_err() {
                        self.error
                            .get_or_insert(FixtureError::OutsideLayer { x: at.x, y: at.y });
                    }
                }
            }
            Err(error) => {
                self.error.get_or_insert(error);
            }
        }
        self.layer(Layer::new(LayerId(0), name, LayerKind::Tile(layer)))
    }

    /// Adds an object layer; objects with id 0 get the next free ids.
    pub fn object_layer(self, name: &str, objects: Vec<MapObject>) -> MapBuilder {
        let layer = ObjectLayer {
            objects: Arc::new(objects),
            ..ObjectLayer::default()
        };
        self.layer(Layer::new(LayerId(0), name, LayerKind::Object(layer)))
    }

    /// Adds an image layer.
    pub fn image_layer(self, name: &str, image: ImageLayer) -> MapBuilder {
        self.layer(Layer::new(LayerId(0), name, LayerKind::Image(image)))
    }

    /// Adds a layer as it is (a group, or one with attributes set); a layer or object with id 0
    /// gets the next free id when the map is built.
    pub fn layer(mut self, layer: Layer) -> MapBuilder {
        self.map.layers.push(layer);
        self
    }

    /// The map, with ids given and the next ids above them; the first mistake in a grid or a
    /// cell outside its layer is an error.
    pub fn build(mut self) -> Result<Map, FixtureError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut next_layer = 1;
        let mut next_object = 1;
        number(&mut self.map.layers, &mut next_layer, &mut next_object);
        self.map.next_layer_id = next_layer;
        self.map.next_object_id = next_object;
        Ok(self.map)
    }
}

/// Gives every layer and object with id 0 the next id, in the order they come.
fn number(layers: &mut [Layer], next_layer: &mut u32, next_object: &mut u32) {
    for layer in layers {
        if layer.id.0 == 0 {
            layer.id = LayerId(*next_layer);
        }
        *next_layer = (*next_layer).max(layer.id.0 + 1);
        match &mut layer.kind {
            LayerKind::Object(objects) => {
                for object in Arc::make_mut(&mut objects.objects) {
                    if object.id.0 == 0 {
                        object.id = ObjectId(*next_object);
                    }
                    *next_object = (*next_object).max(object.id.0 + 1);
                }
            }
            LayerKind::Group(children) => number(children, next_layer, next_object),
            LayerKind::Tile(_) | LayerKind::Image(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Cell, TileId, TilesetSlot};

    use super::*;
    use crate::fixtures::Atlas;

    #[test]
    fn maps_number_their_layers_and_objects() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(3, 2))
            .tileset(atlas.tileset.clone())
            .tile_layer("ground", "0 1 2\n. . 3v")
            .object_layer(
                "things",
                vec![
                    MapObject::default(),
                    MapObject {
                        id: ObjectId(7),
                        ..MapObject::default()
                    },
                ],
            )
            .build()
            .unwrap();
        assert_eq!(map.layers[0].id, LayerId(1));
        assert_eq!(map.layers[1].id, LayerId(2));
        assert_eq!(map.next_layer_id, 3);
        let objects = &map.layers[1].as_object_layer().unwrap().objects;
        assert_eq!(objects[0].id, ObjectId(1));
        assert_eq!(map.next_object_id, 8);
        let ground = map.layers[0].as_tile_layer().unwrap();
        assert_eq!(
            ground.cell(IVec2::new(2, 1)),
            Cell::new(TilesetSlot::new(0).unwrap(), TileId(3)).with_flip(bt_core::Flip::V)
        );
    }

    #[test]
    fn mistakes_in_grids_fail_the_build() {
        let small = MapBuilder::new(UVec2::new(16, 16), UVec2::new(1, 1));
        assert!(matches!(
            small.clone().tile_layer("ground", "0 1").build(),
            Err(FixtureError::OutsideLayer { x: 1, y: 0 })
        ));
        assert!(matches!(
            small.tile_layer("ground", "0 q").build(),
            Err(FixtureError::UnknownToken { .. })
        ));
        let infinite = MapBuilder::new(UVec2::new(16, 16), UVec2::new(1, 1))
            .infinite()
            .tile_layer_at("far", IVec2::new(-40, 90), "1 2")
            .build()
            .unwrap();
        assert_eq!(
            infinite.layers[0]
                .as_tile_layer()
                .unwrap()
                .cell(IVec2::new(-39, 90))
                .tile(),
            TileId(2)
        );
    }
}
