//! Offset Map: shifting the content of layers by whole cells, wrapping around an area when asked
//! (Tiled's `MapDocument::offsetMap` and `OffsetLayer`, `reference/tiled/src/tiled/mapdocument.cpp`
//! and `tiled/offsetlayer.cpp`; `TileLayer::offsetTiles` and `ObjectGroup::offsetObjects`,
//! `libtiled/tilelayer.cpp` and `libtiled/objectgroup.cpp`; the dialog's area,
//! `tiled/offsetmapdialog.cpp`).

use std::sync::Arc;

use bt_core::{Cell, LayerId, LayerKind, Map, MapGeometry, MapObject, TileLayer};
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2};

use crate::error::EditError;
use crate::layer_ops::tree::{check_layers, distinct, holds_or_is, is_group};
use crate::region::TileRegion;
use crate::report::LayersReport;
use crate::tile_layers::{cells_rect, map_size};

/// Offset Map: moves the content of `layers` by `offset` cells within `area`, the bounds of the
/// selection, or with `None` the whole map. Tiles shift within the area; with `wrap_x` and
/// `wrap_y` those pushed out come back on the other side, else empty cells come in and the
/// tiles pushed out are lost. Objects whose rectangle's centre (unturned, placed by their
/// alignment) lies in the area move by the offset in pixels, their centre wrapping around the
/// area; with the whole map every object moves. Image and group layers change their offset,
/// without wrapping, whatever the area. On an infinite map the whole map has no edges: every
/// tile moves and nothing wraps. A layer inside a group that is offset too moves only with the
/// group. Reports the layers that changed; nothing for no offset or an empty area.
pub fn offset_map(
    map: &mut Map,
    layers: &[LayerId],
    area: Option<IRect>,
    offset: IVec2,
    wrap_x: bool,
    wrap_y: bool,
) -> Result<LayersReport, EditError> {
    check_layers(map, layers)?;
    let mut report = LayersReport::default();
    if offset == IVec2::ZERO || area.is_some_and(|rect| rect.is_empty()) {
        return Ok(report);
    }
    let bounds = match area {
        Some(rect) => Some(rect),
        None if map.infinite => None,
        None => Some(cells_rect(map_size(map))),
    };
    let wrap = Wrap {
        x: wrap_x,
        y: wrap_y,
    };
    let geometry = map.geometry();
    let pixel_offset =
        geometry.tile_to_pixel(offset.as_dvec2()) - geometry.tile_to_pixel(DVec2::ZERO);
    let objects_area = ObjectsArea {
        bounds: bounds.map(|rect| pixel_rect(&geometry, rect)),
        whole_map: area.is_none(),
    };
    let ids = distinct(layers);
    // A group's offset carries what is in it.
    let ids: Vec<LayerId> = ids
        .iter()
        .copied()
        .filter(|id| {
            !ids.iter()
                .any(|group| group != id && is_group(map, *group) && holds_or_is(map, *group, *id))
        })
        .collect();
    for id in ids {
        let positions = map
            .layer(id)
            .and_then(|layer| layer.as_object_layer())
            .map(|objects| {
                moved_positions(
                    map,
                    &geometry,
                    &objects.objects,
                    pixel_offset,
                    objects_area,
                    wrap,
                )
            });
        let Some(layer) = map.layer_mut(id) else {
            continue;
        };
        let changed = match &mut layer.kind {
            LayerKind::Tile(tiles) => match offset_tiles(tiles, offset, bounds, wrap)? {
                Some(moved) => {
                    *tiles = moved;
                    true
                }
                None => false,
            },
            LayerKind::Object(objects) => {
                let positions = positions.unwrap_or_default();
                let moves = objects
                    .objects
                    .iter()
                    .zip(&positions)
                    .any(|(object, pos)| object.pos != *pos);
                if moves {
                    for (object, pos) in Arc::make_mut(&mut objects.objects)
                        .iter_mut()
                        .zip(positions)
                    {
                        object.pos = pos;
                    }
                }
                moves
            }
            LayerKind::Image(_) | LayerKind::Group(_) => {
                layer.offset += pixel_offset;
                pixel_offset != DVec2::ZERO
            }
        };
        if changed {
            report.changed.push(id);
        }
    }
    Ok(report)
}

/// Which axes wrap.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Wrap {
    x: bool,
    y: bool,
}

/// Where objects move: those centred in `bounds` (pixels), or every one for the whole map.
#[derive(Copy, Clone, Debug, PartialEq)]
struct ObjectsArea {
    bounds: Option<DRect>,
    whole_map: bool,
}

/// The pixel rectangle of a rectangle of cells, between its corners' pixel positions (Tiled's
/// `tileToPixelCoords(QRectF)`).
fn pixel_rect(geometry: &MapGeometry, rect: IRect) -> DRect {
    let a = geometry.tile_to_pixel(rect.min().as_dvec2());
    let b = geometry.tile_to_pixel(rect.max().as_dvec2());
    DRect {
        min: a.min(b),
        max: a.max(b),
    }
}

/// `tiles` with its cells shifted, or `None` when nothing changes. Within `bounds` each cell
/// takes the cell `offset` before it, wrapped into `bounds` on the axes that wrap, or the empty
/// cell from outside (Tiled's `offsetTiles`); cells outside `bounds` stay. Without bounds every
/// cell moves.
fn offset_tiles(
    tiles: &TileLayer,
    offset: IVec2,
    bounds: Option<IRect>,
    wrap: Wrap,
) -> Result<Option<TileLayer>, EditError> {
    let Some(bounds) = bounds else {
        let mut moved = TileLayer::infinite(tiles.size());
        for placed in tiles.cells() {
            moved.set_cell(placed.at.saturating_add(offset), placed.cell)?;
        }
        return Ok((tiles.cells().next().is_some()).then_some(moved));
    };
    let shift = Shift {
        offset,
        bounds,
        wrap,
    };
    // Tiled visits every cell of the area; only those holding a tile or receiving one change.
    let mut visits = Vec::new();
    for placed in tiles.cells().filter(|placed| bounds.contains(placed.at)) {
        visits.push(placed.at);
        visits.push(shift.destination(placed.at));
    }
    let mut moved = tiles.clone();
    let mut changed = false;
    for to in TileRegion::from_cells(visits).iter() {
        if !bounds.contains(to) || !moved.contains(to) {
            continue;
        }
        let from = shift.source(to);
        let cell = if bounds.contains(from) {
            tiles.cell(from)
        } else {
            Cell::EMPTY
        };
        changed |= moved.set_cell(to, cell)?;
    }
    moved.prune_empty_chunks();
    Ok(changed.then_some(moved))
}

/// How cells shift within an area.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Shift {
    offset: IVec2,
    bounds: IRect,
    wrap: Wrap,
}

impl Shift {
    /// The cell whose tile lands at `to`: `offset` before it, wrapped into the area on the axes
    /// that wrap.
    fn source(self, to: IVec2) -> IVec2 {
        self.wrapped(to.saturating_sub(self.offset))
    }

    /// Where the tile at `from` lands: `offset` after it, wrapped into the area on the axes that
    /// wrap.
    fn destination(self, from: IVec2) -> IVec2 {
        self.wrapped(from.saturating_add(self.offset))
    }

    fn wrapped(self, mut p: IVec2) -> IVec2 {
        if self.wrap.x {
            p.x = wrapped(p.x, self.bounds.x, self.bounds.w);
        }
        if self.wrap.y {
            p.y = wrapped(p.y, self.bounds.y, self.bounds.h);
        }
        p
    }
}

/// `value` wrapped into `start..start + length` (Tiled's `clampWrap`).
fn wrapped(value: i32, start: i32, length: i32) -> i32 {
    let length = i64::from(length.max(1));
    let inside = (i64::from(value) - i64::from(start)).rem_euclid(length) + i64::from(start);
    // Inside the area, which lies in `i32`.
    i32::try_from(inside).unwrap_or(value)
}

/// Where each object goes: moved by `pixel_offset` when its centre is in the area, its centre
/// wrapping around the area's pixels on the axes that wrap (Tiled's `offsetObjects`).
fn moved_positions(
    map: &Map,
    geometry: &MapGeometry,
    objects: &[MapObject],
    pixel_offset: DVec2,
    area: ObjectsArea,
    wrap: Wrap,
) -> Vec<DVec2> {
    objects
        .iter()
        .map(|object| {
            let alignment =
                geometry.object_alignment(object.tile.and_then(|cell| map.tileset_of(cell)));
            let centre =
                object.pos + (DVec2::splat(0.5) - alignment.origin_fraction()) * object.size;
            let inside = area
                .bounds
                .is_some_and(|bounds| contains_closed(bounds, centre));
            if !area.whole_map && !inside {
                return object.pos;
            }
            let mut moved = centre + pixel_offset;
            if let Some(bounds) = area.bounds {
                let size = bounds.size();
                if wrap.x && size.x > 0.0 {
                    moved.x = bounds.min.x + (moved.x - bounds.min.x).rem_euclid(size.x);
                }
                if wrap.y && size.y > 0.0 {
                    moved.y = bounds.min.y + (moved.y - bounds.min.y).rem_euclid(size.y);
                }
            }
            object.pos + (moved - centre)
        })
        .collect()
}

/// Whether `p` is in `rect`, edges included; never in a rectangle without area (Qt's
/// `QRectF::contains`).
fn contains_closed(rect: DRect, p: DVec2) -> bool {
    !rect.is_empty()
        && p.x >= rect.min.x
        && p.x <= rect.max.x
        && p.y >= rect.min.y
        && p.y <= rect.max.y
}

#[cfg(test)]
mod tests {
    use bt_core::{ImageLayer, Shape, TileId, TilesetSlot};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;
    use crate::testutil::{grid_of, group, rect_object, tile_layer, trimmed};

    /// 4×2 cells of 16 pixels: `1:ground 2:things 3:picture 4:group[5:inner]`.
    fn scene() -> Map {
        let atlas = Atlas::builder().build_embedded().unwrap();
        MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(atlas.tileset)
            .tile_layer("ground", "1 . . 2\n. 3 . .")
            .object_layer(
                "things",
                vec![
                    rect_object(48.0, 0.0, 16.0, 16.0),
                    rect_object(0.0, 16.0, 8.0, 8.0),
                ],
            )
            .image_layer("picture", ImageLayer::default())
            .layer(group(
                "group",
                vec![tile_layer("inner", "4 . . .\n. . . .")],
            ))
            .build()
            .unwrap()
    }

    fn ground(map: &Map) -> String {
        let tiles = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        grid_of(tiles, cells_rect(map_size(map)))
    }

    fn positions(map: &Map, layer: u32) -> Vec<DVec2> {
        let objects = &map
            .layer(LayerId(layer))
            .unwrap()
            .as_object_layer()
            .unwrap()
            .objects;
        objects.iter().map(|o| o.pos).collect()
    }

    /// Tiled's `clampWrap`, with C++'s truncating remainder.
    fn clamp_wrap(value: i32, min: i32, max: i32) -> i32 {
        let v = value - min;
        let d = max - min;
        (if v < 0 { (v + 1) % d + d - 1 } else { v % d }) + min
    }

    /// Tiled's `offsetTiles` loop: every cell of the area pulls the cell `offset` before it.
    fn tileds_offset(tiles: &TileLayer, offset: IVec2, bounds: IRect, wrap: Wrap) -> TileLayer {
        let mut moved = tiles.clone();
        for to in bounds.points() {
            if !moved.contains(to) {
                continue;
            }
            let mut from = to - offset;
            if wrap.x {
                from.x = clamp_wrap(from.x, bounds.x, bounds.x + bounds.w);
            }
            if wrap.y {
                from.y = clamp_wrap(from.y, bounds.y, bounds.y + bounds.h);
            }
            let cell = if bounds.contains(from) {
                tiles.cell(from)
            } else {
                Cell::EMPTY
            };
            moved.set_cell(to, cell).unwrap();
        }
        moved.prune_empty_chunks();
        moved
    }

    #[test]
    fn shifting_only_the_tiles_that_move_equals_tileds_loop() {
        let fuzz = bt_test_data::Fuzz::from_env(0x0FF5_E7ED, 400);
        for case in 0..fuzz.cases {
            let mut rng = bt_utils::rng::Rng::new(fuzz.case_seed(case));
            let mut between = |low: i32, high: i32| {
                low + i32::try_from(rng.below((high - low) as u64).unwrap_or(0)).unwrap_or(0)
            };
            let size = UVec2::new(8, 6);
            let mut tiles = if between(0, 3) == 0 {
                TileLayer::infinite(size)
            } else {
                TileLayer::finite(size)
            };
            for _ in 0..between(0, 30) {
                let at = IVec2::new(between(-4, 12), between(-4, 10));
                if tiles.contains(at) {
                    let cell = bt_core::Cell::new(
                        TilesetSlot::new(0).unwrap(),
                        TileId(1 + between(0, 5) as u32),
                    );
                    tiles.set_cell(at, cell).unwrap();
                }
            }
            let bounds = IRect {
                x: between(-2, 6),
                y: between(-2, 5),
                w: between(1, 9),
                h: between(1, 7),
            };
            let offset = IVec2::new(between(-9, 10), between(-9, 10));
            let wrap = Wrap {
                x: between(0, 2) == 0,
                y: between(0, 2) == 0,
            };
            let ours = offset_tiles(&tiles, offset, Some(bounds), wrap)
                .unwrap()
                .unwrap_or_else(|| tiles.clone());
            assert_eq!(
                ours,
                tileds_offset(&tiles, offset, bounds, wrap),
                "case {case} ({}): {bounds:?} by {offset}, {wrap:?}",
                fuzz.reproduce()
            );
        }
    }

    #[test]
    fn the_whole_map_wraps_tiles_and_objects_and_moves_pictures() {
        let mut map = scene();
        let report = offset_map(
            &mut map,
            &[LayerId(1), LayerId(2), LayerId(3)],
            None,
            IVec2::new(2, 0),
            true,
            false,
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(2), LayerId(3)]);
        assert_eq!(ground(&map), trimmed(". 2 1 .\n. . . 3"));
        // The first object's centre (56, 8) goes to 88 and wraps to 24 in the 64 pixels.
        assert_eq!(
            positions(&map, 2),
            [DVec2::new(16.0, 0.0), DVec2::new(32.0, 16.0)]
        );
        assert_eq!(map.layer(LayerId(3)).unwrap().offset, DVec2::new(32.0, 0.0));
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn without_wrapping_what_leaves_is_lost() {
        let mut map = scene();
        offset_map(
            &mut map,
            &[LayerId(1), LayerId(2)],
            None,
            IVec2::new(0, 1),
            false,
            false,
        )
        .unwrap();
        assert_eq!(ground(&map), trimmed(". . . .\n1 . . 2"));
        assert_eq!(
            positions(&map, 2),
            [DVec2::new(48.0, 16.0), DVec2::new(0.0, 32.0)],
            "objects move out of the map"
        );
    }

    #[test]
    fn negative_offsets_wrap_as_tiled_does() {
        let mut map = scene();
        offset_map(&mut map, &[LayerId(1)], None, IVec2::new(-1, 0), true, true).unwrap();
        assert_eq!(ground(&map), trimmed(". . 2 1\n3 . . ."));
        // Six left and three up in all: two right and one down, around a 4×2 map.
        offset_map(
            &mut map,
            &[LayerId(1)],
            None,
            IVec2::new(-5, -3),
            true,
            true,
        )
        .unwrap();
        assert_eq!(ground(&map), trimmed(". . . 3\n. 2 1 ."));
    }

    #[test]
    fn within_an_area_only_what_is_centred_in_it_moves() {
        let mut map = scene();
        let area = IRect {
            x: 1,
            y: 0,
            w: 3,
            h: 1,
        };
        let report = offset_map(
            &mut map,
            &[LayerId(1), LayerId(2), LayerId(3)],
            Some(area),
            IVec2::new(1, 0),
            true,
            true,
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(2), LayerId(3)]);
        assert_eq!(ground(&map), trimmed("1 2 . .\n. 3 . ."));
        // The first object's centre wraps around the area's 48 pixels; the second is outside.
        assert_eq!(
            positions(&map, 2),
            [DVec2::new(16.0, 0.0), DVec2::new(0.0, 16.0)]
        );
        assert_eq!(
            map.layer(LayerId(3)).unwrap().offset,
            DVec2::new(16.0, 0.0),
            "pictures move whatever the area"
        );
        let empty = IRect {
            x: 1,
            y: 0,
            w: 0,
            h: 1,
        };
        assert!(
            offset_map(&mut map, &[LayerId(1)], Some(empty), IVec2::ONE, true, true)
                .unwrap()
                .is_empty()
        );
        assert!(
            offset_map(&mut map, &[LayerId(1)], None, IVec2::ZERO, true, true)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            offset_map(&mut map, &[LayerId(9)], None, IVec2::ONE, true, true),
            Err(EditError::NoSuchLayer(LayerId(9)))
        );
    }

    #[test]
    fn tile_objects_are_centred_by_their_alignment() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let tile = bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(0));
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 2))
            .tileset(atlas.tileset)
            .object_layer(
                "things",
                vec![
                    // Standing on (0, 32): its picture covers the second row.
                    MapObject {
                        tile: Some(tile),
                        ..rect_object(0.0, 32.0, 16.0, 16.0)
                    },
                    // Hanging from (0, 32): below the map.
                    rect_object(0.0, 32.0, 16.0, 16.0),
                ],
            )
            .build()
            .unwrap();
        let row = IRect {
            x: 0,
            y: 1,
            w: 1,
            h: 1,
        };
        offset_map(
            &mut map,
            &[LayerId(1)],
            Some(row),
            IVec2::new(1, 0),
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            positions(&map, 1),
            [DVec2::new(16.0, 32.0), DVec2::new(0.0, 32.0)]
        );
    }

    #[test]
    fn infinite_maps_move_everything_without_wrapping() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(4, 4))
            .infinite()
            .tileset(atlas.tileset)
            .tile_layer_at("ground", IVec2::new(-40, 3), "1")
            .object_layer(
                "things",
                vec![MapObject {
                    shape: Shape::Point,
                    ..rect_object(-1000.0, 0.0, 0.0, 0.0)
                }],
            )
            .build()
            .unwrap();
        let report = offset_map(
            &mut map,
            &[LayerId(1), LayerId(2)],
            None,
            IVec2::new(-3, 2),
            true,
            true,
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(1), LayerId(2)]);
        let tiles = map.layer(LayerId(1)).unwrap().as_tile_layer().unwrap();
        assert_eq!(tiles.cell(IVec2::new(-43, 5)).tile(), TileId(1));
        assert_eq!(tiles.cells().count(), 1);
        assert!(tiles.is_infinite());
        assert_eq!(positions(&map, 2), [DVec2::new(-1048.0, 32.0)]);
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn a_group_carries_its_layers() {
        let mut map = scene();
        let report = offset_map(
            &mut map,
            &[LayerId(5), LayerId(4)],
            None,
            IVec2::new(1, 1),
            true,
            true,
        )
        .unwrap();
        assert_eq!(report.changed, [LayerId(4)]);
        assert_eq!(
            map.layer(LayerId(4)).unwrap().offset,
            DVec2::new(16.0, 16.0)
        );
        let inner = map.layer(LayerId(5)).unwrap();
        assert_eq!(inner.offset, DVec2::ZERO);
        assert_eq!(
            inner.as_tile_layer().unwrap().cell(IVec2::ZERO).tile(),
            TileId(4)
        );
    }
}
