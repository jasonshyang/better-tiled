//! Making a map infinite or finite (the Infinite map property, Tiled's `ChangeMapInfinite`,
//! `reference/tiled/src/tiled/changemapproperty.h`, with the crop to the tiles that Tiled's
//! property editor once did before turning a map finite, through `MapDocument::resizeMap`).

use bt_core::{LayerKind, Map, TileBounds, TileLayer};
use glam::{IVec2, UVec2};

use super::resize::{resize_with, used_bounds};
use crate::error::EditError;
use crate::report::ResizeReport;
use crate::tile_layers::{cells_rect, map_size, rebounded_tile_layer};

/// Makes the map infinite: every tile layer keeps its cells and loses its bounds. Returns whether
/// the map changed (it was finite).
pub fn make_infinite(map: &mut Map) -> Result<bool, EditError> {
    if map.infinite {
        return Ok(false);
    }
    let mut unbounded: Vec<TileLayer> = Vec::new();
    let mut refusal = None;
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            match rebounded_tile_layer(tiles, tiles.size(), TileBounds::Infinite) {
                Ok(made) => unbounded.push(made),
                Err(error) => refusal = Some(error),
            }
        }
    });
    if let Some(error) = refusal {
        return Err(error);
    }
    let mut unbounded = unbounded.into_iter();
    map.for_each_layer_mut(|layer| {
        if let LayerKind::Tile(tiles) = &mut layer.kind
            && let Some(made) = unbounded.next()
        {
            *tiles = made;
        }
    });
    map.infinite = true;
    Ok(true)
}

/// Makes the map finite, cropped to its tiles: the map becomes the smallest rectangle holding
/// every tile of every tile layer, its content moved so that rectangle starts at the first cell,
/// objects and image layers moving with the tiles as in a resize (none is removed). A map
/// without tiles keeps the size it declares, at least one cell. Reports how far things moved;
/// nothing for a map that is finite already.
pub fn make_finite(map: &mut Map) -> Result<ResizeReport, EditError> {
    if !map.infinite {
        return Ok(ResizeReport::default());
    }
    let area = used_bounds(map).unwrap_or_else(|| cells_rect(map_size(map).max(UVec2::ONE)));
    let size = area.size().max(IVec2::ONE).as_uvec2();
    let report = resize_with(map, size, -area.min(), Vec::new(), TileBounds::Finite)?;
    map.infinite = false;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use bt_core::{ImageLayer, LayerId, TileId};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::IRect;
    use glam::DVec2;

    use super::*;
    use crate::testutil::{grid_of, group, rect_object, tile_layer, trimmed};

    fn assert_consistent(map: &Map) {
        assert_eq!(map.validate(), []);
        map.for_each_layer(|layer, _| {
            if let Some(tiles) = layer.as_tile_layer() {
                assert_eq!(tiles.size(), map_size(map));
                assert_eq!(tiles.is_infinite(), map.infinite);
            }
        });
    }

    #[test]
    fn infinite_and_back_keeps_the_cells() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 . 2\n. 3 .")
            .layer(group("group", vec![tile_layer("inner", ". . .\n4 . .")]))
            .build()
            .unwrap();
        let finite = map.clone();
        assert_eq!(make_infinite(&mut map), Ok(true));
        assert!(map.infinite);
        assert_consistent(&map);
        assert_eq!(make_infinite(&mut map), Ok(false), "already infinite");
        let report = make_finite(&mut map).unwrap();
        assert_eq!(report.tile_offset, IVec2::ZERO);
        assert_eq!(map, finite, "the tiles span the map: the same map again");
        assert_eq!(make_finite(&mut map), Ok(ResizeReport::default()));
    }

    #[test]
    fn finite_crops_to_the_tiles_and_moves_the_rest_with_them() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(10, 10))
            .infinite()
            .tileset(atlas.tileset)
            .tile_layer_at("ground", IVec2::new(-5, -2), "1")
            .tile_layer_at("far", IVec2::new(4, 7), "2")
            .object_layer("things", vec![rect_object(0.0, 0.0, 1.0, 1.0)])
            .image_layer("picture", ImageLayer::default())
            .build()
            .unwrap();
        let report = make_finite(&mut map).unwrap();
        assert_eq!(report.tile_offset, IVec2::new(5, 2));
        assert_eq!(report.pixel_offset, DVec2::new(80.0, 32.0));
        assert!(report.removed_objects.is_empty(), "nothing is removed");
        assert!(!map.infinite);
        assert_eq!((map.width, map.height), (10, 10));
        let ground = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        assert_eq!(ground.cell(IVec2::ZERO).tile(), TileId(1));
        let far = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
        assert_eq!(far.cell(IVec2::new(9, 9)).tile(), TileId(2));
        let objects = &map
            .layer(LayerId(3))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        assert_eq!(objects[0].pos, DVec2::new(80.0, 32.0));
        assert_eq!(
            map.layer(LayerId(4)).unwrap().offset,
            DVec2::new(80.0, 32.0)
        );
        assert_consistent(&map);
    }

    #[test]
    fn a_map_without_tiles_keeps_its_size() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(7, 3))
            .infinite()
            .tile_layer("empty", ".")
            .build()
            .unwrap();
        make_finite(&mut map).unwrap();
        assert_eq!((map.width, map.height), (7, 3));
        assert_consistent(&map);
        let mut nothing = MapBuilder::new(UVec2::splat(16), UVec2::ZERO)
            .infinite()
            .build()
            .unwrap();
        make_finite(&mut nothing).unwrap();
        assert_eq!((nothing.width, nothing.height), (1, 1), "at least one cell");
        let all = IRect {
            x: 0,
            y: 0,
            w: 7,
            h: 3,
        };
        let empty = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        assert_eq!(
            grid_of(empty, all),
            trimmed(". . . . . . .\n. . . . . . .\n. . . . . . .")
        );
    }
}
