//! Resize Map, Crop to Selection and Autocrop (Tiled's `MapDocument::resizeMap` and
//! `autocropMap`, `reference/tiled/src/tiled/mapdocument.cpp`; `ResizeTileLayer` and
//! `TileLayer::resize`, `tiled/resizetilelayer.cpp` and `libtiled/tilelayer.cpp`;
//! `MapDocumentActionHandler::cropToSelection`, `tiled/mapdocumentactionhandler.cpp`).

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{LayerId, LayerKind, Map, ObjectId, TileBounds, TileLayer};
use bt_utils::IRect;
use glam::{DVec2, IVec2, UVec2};

use super::outside::objects_outside;
use crate::error::EditError;
use crate::report::ResizeReport;
use crate::tile_layers::{bounds_of, cells_rect, map_size, resized_tile_layer};

/// Resize Map: the map becomes `size` cells, its content moved by `offset` cells. Every tile
/// layer keeps the cells that land inside the new size; on an infinite map too, as in Tiled.
/// Objects and image layers move by the offset converted to pixels through the map's grid
/// (where the new top-left cell was), so on isometric, staggered and hexagonal maps they move
/// with the tiles; on staggered and hexagonal maps an odd offset along the staggered axis
/// changes which rows are shifted, and objects follow the new top-left cell as in Tiled. Groups
/// keep their offsets. With `remove_objects_outside`, objects whose rectangle on screen does not
/// touch the new map are removed and reported. Nothing happens when the size and place stay and
/// no object would go.
pub fn resize_map(
    map: &mut Map,
    size: UVec2,
    offset: IVec2,
    remove_objects_outside: bool,
) -> Result<ResizeReport, EditError> {
    let area = IRect::from_min_size(IVec2::ZERO.saturating_sub(offset), cells_rect(size).size());
    let removed = if remove_objects_outside {
        objects_outside(map, area)
    } else {
        Vec::new()
    };
    if size == map_size(map) && offset == IVec2::ZERO && removed.is_empty() {
        return Ok(ResizeReport::default());
    }
    resize_with(map, size, offset, removed, bounds_of(map))
}

/// Crop to Selection: the map becomes `rect`, the bounds of the selection, and objects outside it
/// are removed. An empty rectangle crops nothing.
pub fn crop_to(map: &mut Map, rect: IRect) -> Result<ResizeReport, EditError> {
    if rect.is_empty() {
        return Ok(ResizeReport::default());
    }
    let offset = IVec2::ZERO.saturating_sub(rect.min());
    resize_map(map, rect.size().as_uvec2(), offset, true)
}

/// Autocrop over every tile layer: the map is cropped to the cells holding tiles on any of them,
/// objects outside going. A map without tiles stays as it is.
pub fn autocrop(map: &mut Map) -> Result<ResizeReport, EditError> {
    match used_bounds(map) {
        Some(bounds) => crop_to(map, bounds),
        None => Ok(ResizeReport::default()),
    }
}

/// Tiled's Autocrop: the map is cropped to the cells holding tiles on the tile layer `layer`,
/// objects outside going. An empty layer leaves the map as it is.
pub fn autocrop_to_layer(map: &mut Map, layer: LayerId) -> Result<ResizeReport, EditError> {
    let tiles = map
        .layer(layer)
        .ok_or(EditError::NoSuchLayer(layer))?
        .as_tile_layer()
        .ok_or(EditError::NotATileLayer(layer))?;
    match tiles.used_bounds() {
        Some(bounds) => crop_to(map, bounds),
        None => Ok(ResizeReport::default()),
    }
}

/// The smallest rectangle holding every tile of every tile layer.
pub(crate) fn used_bounds(map: &Map) -> Option<IRect> {
    let mut bounds: Option<IRect> = None;
    map.for_each_layer(|layer, _| {
        if let Some(used) = layer.as_tile_layer().and_then(TileLayer::used_bounds) {
            bounds = Some(bounds.map_or(used, |b| b.union(used)));
        }
    });
    bounds
}

/// Resizes the map to `size` with its content moved by `offset`, its tile layers made with
/// `bounds`, and `removed` taken out of its object layers. The tile layers are made before
/// anything changes, so a refusal leaves the map as it was.
pub(crate) fn resize_with(
    map: &mut Map,
    size: UVec2,
    offset: IVec2,
    removed: Vec<ObjectId>,
    bounds: TileBounds,
) -> Result<ResizeReport, EditError> {
    let geometry = map.geometry();
    let pixel_offset =
        geometry.tile_to_pixel(DVec2::ZERO) - geometry.tile_to_pixel(-offset.as_dvec2());
    let mut resized: Vec<TileLayer> = Vec::new();
    let mut refusal = None;
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer() {
            match resized_tile_layer(tiles, size, offset, bounds) {
                Ok(made) => resized.push(made),
                Err(error) => refusal = Some(error),
            }
        }
    });
    if let Some(error) = refusal {
        return Err(error);
    }
    let gone: HashSet<ObjectId> = removed.iter().copied().collect();
    let mut resized = resized.into_iter();
    map.for_each_layer_mut(|layer| match &mut layer.kind {
        LayerKind::Tile(tiles) => {
            if let Some(made) = resized.next() {
                *tiles = made;
            }
        }
        LayerKind::Object(objects) => {
            let touched = pixel_offset != DVec2::ZERO && !objects.objects.is_empty()
                || objects.objects.iter().any(|o| gone.contains(&o.id));
            if touched {
                let list = Arc::make_mut(&mut objects.objects);
                list.retain(|o| !gone.contains(&o.id));
                for object in list {
                    object.pos += pixel_offset;
                }
            }
        }
        LayerKind::Image(_) => layer.offset += pixel_offset,
        LayerKind::Group(_) => {}
    });
    map.width = size.x;
    map.height = size.y;
    Ok(ResizeReport {
        removed_objects: removed,
        tile_offset: offset,
        pixel_offset,
    })
}

#[cfg(test)]
mod tests {
    use bt_core::{ImageLayer, Layer, MapObject, Orientation, StaggerAxis, StaggerIndex};
    use bt_test_data::fixtures::{Atlas, MapBuilder};

    use super::*;
    use crate::testutil::{grid_of, group, rect_object, tile_layer, trimmed};

    /// 4×3 cells of 16 pixels: `1:ground 2:group[3:inner 4:picture] 5:things`, the group and the
    /// picture with offsets of their own, three objects.
    fn scene() -> Map {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let picture = Layer {
            offset: DVec2::new(1.0, 2.0),
            ..Layer::new(
                LayerId(0),
                "picture",
                LayerKind::Image(ImageLayer::default()),
            )
        };
        let inner = tile_layer("inner", ". . . .\n. 4 . .\n. . . .");
        let held = Layer {
            offset: DVec2::new(5.0, 5.0),
            ..group("group", vec![inner, picture])
        };
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 3))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 2 . .\n. . . .\n. . . 3")
            .layer(held)
            .object_layer(
                "things",
                vec![
                    rect_object(8.0, 8.0, 8.0, 8.0),
                    rect_object(40.0, 30.0, 4.0, 4.0),
                    rect_object(60.0, 0.0, 4.0, 4.0),
                ],
            )
            .build()
            .unwrap()
    }

    fn cells(map: &Map, id: u32) -> String {
        let tiles = map.layer(LayerId(id)).unwrap().as_tile_layer().unwrap();
        grid_of(tiles, cells_rect(map_size(map)))
    }

    fn positions(map: &Map) -> Vec<DVec2> {
        let mut positions = Vec::new();
        map.for_each_layer(|layer, _| {
            if let Some(objects) = layer.as_object_layer() {
                positions.extend(objects.objects.iter().map(|o| o.pos));
            }
        });
        positions
    }

    /// Every tile layer has the map's size and bounds, and the map breaks no invariant.
    fn assert_consistent(map: &Map) {
        assert_eq!(map.validate(), []);
        map.for_each_layer(|layer, _| {
            if let Some(tiles) = layer.as_tile_layer() {
                assert_eq!(tiles.size(), map_size(map), "layer {}", layer.id);
                assert_eq!(tiles.is_infinite(), map.infinite, "layer {}", layer.id);
            }
        });
    }

    #[test]
    fn resizing_moves_tiles_objects_and_images_and_leaves_groups() {
        let mut map = scene();
        let report = resize_map(&mut map, UVec2::new(6, 5), IVec2::new(2, 1), false).unwrap();
        assert_eq!(
            report,
            ResizeReport {
                removed_objects: Vec::new(),
                tile_offset: IVec2::new(2, 1),
                pixel_offset: DVec2::new(32.0, 16.0),
            }
        );
        assert_eq!((map.width, map.height), (6, 5));
        assert_eq!(
            cells(&map, 1),
            trimmed(
                "
                . . . . . .
                . . 1 2 . .
                . . . . . .
                . . . . . 3
                . . . . . .
                "
            )
        );
        assert_eq!(
            map.layer(LayerId(3))
                .unwrap()
                .as_tile_layer()
                .unwrap()
                .cell(IVec2::new(3, 2))
                .tile()
                .0,
            4
        );
        assert_eq!(
            map.layer(LayerId(4)).unwrap().offset,
            DVec2::new(33.0, 18.0)
        );
        assert_eq!(
            map.layer(LayerId(2)).unwrap().offset,
            DVec2::new(5.0, 5.0),
            "groups stay"
        );
        assert_eq!(
            positions(&map),
            [
                DVec2::new(40.0, 24.0),
                DVec2::new(72.0, 46.0),
                DVec2::new(92.0, 16.0)
            ]
        );
        assert_consistent(&map);
    }

    #[test]
    fn cropping_cuts_tiles_and_removes_objects_outside() {
        let mut map = scene();
        let crop = IRect {
            x: 1,
            y: 0,
            w: 2,
            h: 2,
        };
        let report = crop_to(&mut map, crop).unwrap();
        // The first object touches the new left edge, the second the bottom; the third is out.
        assert_eq!(report.removed_objects, [ObjectId(3)]);
        assert_eq!(report.tile_offset, IVec2::new(-1, 0));
        assert_eq!(report.pixel_offset, DVec2::new(-16.0, 0.0));
        assert_eq!(cells(&map, 1), trimmed("2 .\n. ."));
        assert_eq!(cells(&map, 3), trimmed(". .\n4 ."));
        assert_eq!(
            positions(&map),
            [DVec2::new(-8.0, 8.0), DVec2::new(24.0, 30.0)]
        );
        assert_consistent(&map);
        let before = map.clone();
        assert_eq!(
            crop_to(
                &mut map,
                IRect {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 5
                }
            ),
            Ok(ResizeReport::default())
        );
        assert_eq!(map, before, "an empty rectangle crops nothing");
    }

    #[test]
    fn nothing_changes_unless_something_would() {
        let mut map = scene();
        let before = map.clone();
        let report = resize_map(&mut map, UVec2::new(4, 3), IVec2::ZERO, false).unwrap();
        assert_eq!((report, &map), (ResizeReport::default(), &before));
        // Keeping the size and place still removes what is outside, as Tiled's crop does.
        let things = map
            .layer_mut(LayerId(5))
            .unwrap()
            .as_object_layer_mut()
            .unwrap();
        Arc::make_mut(&mut things.objects).push(MapObject {
            id: ObjectId(4),
            ..rect_object(100.0, 100.0, 1.0, 1.0)
        });
        map.next_object_id = 5;
        let report = resize_map(&mut map, UVec2::new(4, 3), IVec2::ZERO, true).unwrap();
        assert_eq!(report.removed_objects, [ObjectId(4)]);
        assert_eq!(positions(&map).len(), 3);
    }

    #[test]
    fn autocrop_takes_the_tiles_of_every_layer_or_of_one() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(10, 8))
            .tileset(atlas.tileset)
            .tile_layer_at("a", IVec2::new(3, 4), "1")
            .tile_layer_at("b", IVec2::new(7, 5), "2")
            .tile_layer("empty", ".")
            .object_layer("things", vec![rect_object(0.0, 0.0, 4.0, 4.0)])
            .build()
            .unwrap();
        let mut one = map.clone();
        let report = autocrop(&mut map).unwrap();
        assert_eq!(report.tile_offset, IVec2::new(-3, -4));
        assert_eq!(report.removed_objects, [ObjectId(1)]);
        assert_eq!((map.width, map.height), (5, 2));
        assert_eq!(cells(&map, 1), trimmed("1 . . . .\n. . . . ."));
        assert_eq!(cells(&map, 2), trimmed(". . . . .\n. . . . 2"));
        assert_consistent(&map);
        autocrop_to_layer(&mut one, LayerId(2)).unwrap();
        assert_eq!((one.width, one.height), (1, 1));
        assert_eq!(cells(&one, 2), "2");
        assert_consistent(&one);
        let before = one.clone();
        assert_eq!(
            autocrop_to_layer(&mut one, LayerId(3)),
            Ok(ResizeReport::default())
        );
        assert_eq!(one, before, "an empty layer crops nothing");
        assert_eq!(
            autocrop_to_layer(&mut one, LayerId(4)),
            Err(EditError::NotATileLayer(LayerId(4)))
        );
        assert_eq!(
            autocrop_to_layer(&mut one, LayerId(9)),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
        let mut bare = MapBuilder::new(UVec2::splat(16), UVec2::new(3, 3))
            .tile_layer("empty", ". . .")
            .build()
            .unwrap();
        assert_eq!(autocrop(&mut bare), Ok(ResizeReport::default()));
        assert_eq!((bare.width, bare.height), (3, 3));
    }

    #[test]
    fn infinite_maps_are_cut_to_the_new_size_too() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 4))
            .infinite()
            .tileset(atlas.tileset)
            .tile_layer_at("ground", IVec2::new(-3, 2), "1")
            .tile_layer_at("far", IVec2::new(10, 1), "2")
            .build()
            .unwrap();
        resize_map(&mut map, UVec2::new(5, 5), IVec2::new(3, 0), false).unwrap();
        assert!(map.infinite);
        let ground = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        assert_eq!(ground.cell(IVec2::new(0, 2)).tile().0, 1);
        let far = map.layer(LayerId(2)).unwrap().as_tile_layer().unwrap();
        assert_eq!(far.cells().count(), 0, "outside the new size, as in Tiled");
        assert_consistent(&map);
    }

    /// The grids of every orientation, 4×4 cells.
    fn grids() -> Vec<MapBuilder> {
        let mut grids = vec![
            MapBuilder::new(UVec2::splat(16), UVec2::splat(4)),
            MapBuilder::new(UVec2::new(32, 16), UVec2::splat(4))
                .orientation(Orientation::Isometric),
            MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
                .orientation(Orientation::Oblique)
                .skew(IVec2::new(4, 0)),
        ];
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            for index in [StaggerIndex::Odd, StaggerIndex::Even] {
                grids.push(
                    MapBuilder::new(UVec2::new(64, 32), UVec2::splat(4))
                        .orientation(Orientation::Staggered)
                        .stagger(axis, index),
                );
                grids.push(
                    MapBuilder::new(UVec2::splat(32), UVec2::splat(4))
                        .orientation(Orientation::Hexagonal)
                        .hex_side_length(16)
                        .stagger(axis, index),
                );
            }
        }
        grids
    }

    #[test]
    fn objects_stay_on_their_cells_on_every_grid() {
        for builder in grids() {
            for offset in [
                IVec2::new(2, 2),
                IVec2::new(-2, 4),
                IVec2::new(1, 2),
                IVec2::new(2, 1),
            ] {
                let mut map = builder.clone().build().unwrap();
                let staggered = matches!(
                    map.orientation,
                    Orientation::Staggered | Orientation::Hexagonal
                );
                // An odd step along the staggered axis changes which rows are shifted.
                let along = match map.stagger_axis {
                    StaggerAxis::X => offset.x,
                    StaggerAxis::Y => offset.y,
                };
                if staggered && along % 2 != 0 {
                    continue;
                }
                let geometry = map.geometry();
                let cell = IVec2::new(1, 2);
                let at = geometry.tile_to_pixel(cell.as_dvec2());
                map.layers.push(Layer::new(
                    LayerId(1),
                    "things",
                    LayerKind::Object(bt_core::ObjectLayer {
                        objects: Arc::new(vec![MapObject {
                            id: ObjectId(1),
                            pos: at,
                            ..MapObject::default()
                        }]),
                        ..bt_core::ObjectLayer::default()
                    }),
                ));
                map.next_layer_id = 2;
                map.next_object_id = 2;
                resize_map(&mut map, UVec2::splat(8), offset, false).unwrap();
                let expected = map.geometry().tile_to_pixel((cell + offset).as_dvec2());
                assert_eq!(
                    positions(&map),
                    [expected],
                    "{:?} {:?} {:?} by {offset}",
                    map.orientation,
                    map.stagger_axis,
                    map.stagger_index
                );
            }
        }
    }

    #[test]
    fn isometric_hexagonal_and_staggered_offsets_in_pixels() {
        let pixel_offset = |builder: MapBuilder, offset: IVec2| {
            let mut map = builder.build().unwrap();
            resize_map(&mut map, UVec2::splat(8), offset, false)
                .unwrap()
                .pixel_offset
        };
        let iso = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(4))
            .orientation(Orientation::Isometric);
        // Isometric pixels are squares of the tile height along the grid's axes.
        assert_eq!(pixel_offset(iso, IVec2::new(2, 1)), DVec2::new(32.0, 16.0));
        let hex = MapBuilder::new(UVec2::splat(32), UVec2::splat(4))
            .orientation(Orientation::Hexagonal)
            .hex_side_length(16)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd);
        // Rows are 24 pixels apart, columns 32; an odd row shifts half a column.
        assert_eq!(
            pixel_offset(hex.clone(), IVec2::new(1, 2)),
            DVec2::new(32.0, 48.0)
        );
        assert_eq!(pixel_offset(hex, IVec2::new(0, 1)), DVec2::new(-16.0, 24.0));
        let staggered = MapBuilder::new(UVec2::new(64, 32), UVec2::splat(4))
            .orientation(Orientation::Staggered)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd);
        assert_eq!(
            pixel_offset(staggered.clone(), IVec2::new(1, 2)),
            DVec2::new(64.0, 32.0)
        );
        assert_eq!(
            pixel_offset(staggered, IVec2::new(0, 1)),
            DVec2::new(-32.0, 16.0)
        );
    }
}
