//! The space objects live in: the grid that projects and snaps them, and the tilesets their tiles
//! come from. A map's object layers live in the map's; a tile's collision shapes in the tile's
//! own, a one-cell map of the tile's grid as Tiled's collision editor sets it up
//! (`reference/tiled/src/tiled/tilecollisiondock.cpp`); a template's object in a grid of
//! one-pixel cells, as Tiled's template editor sets it up (`tiled/templatesdock.cpp`).

use bt_core::{
    Alignment, Cell, GridOrientation, Map, MapGeometry, MapObject, ObjectTemplate, Orientation,
    TileId, Tileset, TilesetGrid, TilesetRef,
};
use glam::{IVec2, UVec2};

/// Where a list of objects lives: the grid and the tilesets its tile objects name.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ObjectSpace<'a> {
    geometry: MapGeometry,
    tilesets: &'a [TilesetRef],
}

impl<'a> ObjectSpace<'a> {
    /// The space of a map's object layers: its grid and its tilesets.
    pub fn of_map(map: &'a Map) -> ObjectSpace<'a> {
        ObjectSpace {
            geometry: map.geometry(),
            tilesets: &map.tilesets,
        }
    }

    /// The space of a tile's collision shapes, as Tiled's collision editor makes it: the tile's
    /// own pixels on a one-cell orthogonal grid of the tile's size, or on an isometric grid of
    /// the tileset's grid size when the tileset's grid is isometric. Collision shapes draw no
    /// tiles, so the space has no tilesets.
    pub fn of_tile(tileset: &Tileset, tile: TileId) -> ObjectSpace<'static> {
        let cell = |size: UVec2| size.min(UVec2::splat(i32::MAX.cast_unsigned())).as_ivec2();
        let geometry = match tileset.grid {
            Some(TilesetGrid {
                orientation: GridOrientation::Isometric,
                size,
            }) => MapGeometry::new(Orientation::Isometric, cell(size), IVec2::ONE),
            Some(TilesetGrid {
                orientation: GridOrientation::Orthogonal,
                ..
            })
            | None => MapGeometry::new(
                Orientation::Orthogonal,
                cell(tileset.tile_pixel_size(tile)),
                IVec2::ONE,
            ),
        };
        ObjectSpace {
            geometry,
            tilesets: &[],
        }
    }

    /// The space of a template's object, as Tiled's template editor makes it: an orthogonal
    /// grid of one-pixel cells, with the template's tilesets for its tile.
    pub fn of_template(template: &'a ObjectTemplate) -> ObjectSpace<'a> {
        ObjectSpace {
            geometry: MapGeometry::new(Orientation::Orthogonal, IVec2::ONE, IVec2::ONE),
            tilesets: &template.tilesets,
        }
    }

    /// A space from its parts, for operations that hold a map's layers apart from its tilesets.
    pub(crate) fn from_parts(geometry: MapGeometry, tilesets: &'a [TilesetRef]) -> ObjectSpace<'a> {
        ObjectSpace { geometry, tilesets }
    }

    /// The grid.
    pub fn geometry(&self) -> &MapGeometry {
        &self.geometry
    }

    /// The tileset of a cell, when the space has it.
    pub(crate) fn tileset_of(&self, cell: Cell) -> Option<&'a Tileset> {
        self.tilesets
            .get(cell.slot()?.index())
            .map(|r| r.tileset.as_ref())
    }

    /// Tiled's alignment of an object (`MapObject::alignment`): a tile object's tileset's, else
    /// the grid's default for tile objects; top-left for every other object. A tile object
    /// whose tileset the space lacks takes the default, as the renderer draws it.
    pub(crate) fn alignment_of(&self, object: &MapObject) -> Alignment {
        let Some(cell) = object.tile else {
            return self.geometry.object_alignment(None);
        };
        let fallback = Tileset::default();
        self.geometry
            .object_alignment(Some(self.tileset_of(cell).unwrap_or(&fallback)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{TileData, TilesetSlot};
    use bt_test_data::fixtures::Atlas;
    use glam::DVec2;

    use super::*;

    #[test]
    fn a_tiles_space_is_its_own_grid() {
        let atlas = Atlas::builder().tile_size(32, 48).build_embedded().unwrap();
        let space = ObjectSpace::of_tile(&atlas.tileset, TileId(0));
        let geometry = space.geometry();
        assert_eq!(geometry.orientation, Orientation::Orthogonal);
        assert_eq!((geometry.tile_width, geometry.tile_height), (32, 48));
        assert!(
            space
                .tileset_of(Cell::new(TilesetSlot::new(0).unwrap(), TileId(0)))
                .is_none(),
            "collision shapes draw no tiles"
        );
        let mut isometric = atlas.tileset.as_ref().clone();
        isometric.grid = Some(TilesetGrid {
            orientation: GridOrientation::Isometric,
            size: UVec2::new(64, 32),
        });
        isometric.tiles.insert(TileId(1), TileData::default());
        let space = ObjectSpace::of_tile(&isometric, TileId(1));
        assert_eq!(space.geometry().orientation, Orientation::Isometric);
        assert_eq!(
            space.geometry().pixel_to_screen(DVec2::ZERO),
            DVec2::new(32.0, 0.0),
            "the top corner of the one cell"
        );
    }

    #[test]
    fn tile_objects_align_by_their_tileset_or_the_grid() {
        let mut tileset = Atlas::builder().build_embedded().unwrap().tileset;
        let mut map = Map {
            tilesets: vec![TilesetRef::embedded(Arc::clone(&tileset))],
            ..Map::default()
        };
        let tile = MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            ..MapObject::default()
        };
        let space = ObjectSpace::of_map(&map);
        assert_eq!(space.alignment_of(&tile), Alignment::BottomLeft);
        assert_eq!(
            space.alignment_of(&MapObject::default()),
            Alignment::TopLeft
        );
        Arc::make_mut(&mut tileset).object_alignment = Alignment::Center;
        map.tilesets = vec![TilesetRef::embedded(tileset)];
        assert_eq!(
            ObjectSpace::of_map(&map).alignment_of(&tile),
            Alignment::Center
        );
        map.orientation = Orientation::Isometric;
        map.tilesets.clear();
        assert_eq!(
            ObjectSpace::of_map(&map).alignment_of(&tile),
            Alignment::Bottom,
            "a missing tileset leaves the grid's default"
        );
    }

    #[test]
    fn a_templates_object_lives_on_one_pixel_cells_with_its_tilesets() {
        use bt_core::{ObjectId, ObjectLayer};
        use bt_utils::DRect;

        use crate::objects::bounds::object_bounds;
        use crate::objects::convert::reset_tile_object_size_in;
        use crate::objects::snap::{SnapMode, snap};

        let atlas = Atlas::builder().tile_size(24, 40).build_embedded().unwrap();
        let template = ObjectTemplate {
            object: Some(MapObject {
                tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(3))),
                size: DVec2::new(48.0, 80.0),
                ..MapObject::default()
            }),
            tilesets: vec![TilesetRef::embedded(atlas.tileset)],
            ..ObjectTemplate::default()
        };
        let space = ObjectSpace::of_template(&template);
        // The template's object, as a list of one.
        let mut objects = ObjectLayer {
            objects: Arc::new(template.object.clone().into_iter().collect()),
            ..ObjectLayer::default()
        };
        assert_eq!(
            object_bounds(&space, &objects.objects[0]),
            DRect::from_min_size(DVec2::new(0.0, -80.0), DVec2::new(48.0, 80.0))
        );
        reset_tile_object_size_in(&mut objects, &space, &[ObjectId(0)]).unwrap();
        assert_eq!(objects.objects[0].size, DVec2::new(24.0, 40.0));
        assert_eq!(
            snap(space.geometry(), DVec2::new(3.4, 5.6), SnapMode::Grid),
            DVec2::new(3.0, 6.0),
            "the grid is whole pixels"
        );
    }
}
