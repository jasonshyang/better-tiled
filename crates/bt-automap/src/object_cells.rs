//! The cells an object covers, as Tiled's automapper works them out (`objectTileRect` and
//! `objectsInRegion` in `automappingutils.cpp`): the object's box, aligned as Tiled aligns it,
//! turned into tile coordinates and rounded outwards. The result is a rectangle as Qt's `QRect`
//! holds it, which can be zero or negative in size, and Qt treats such rectangles in ways the
//! rules depend on: a point at a cell's corner covers nothing, and one on a cell's edge touches
//! a region only where the region spans that edge.

use bt_core::{Alignment, Map, MapObject, Orientation};
use bt_utils::IRect;
use glam::DVec2;

use crate::cell_set::CellSet;

/// A rectangle as `QRect` keeps it: a top-left and a size that may be zero or negative.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct QtRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

impl QtRect {
    /// The cells `QRegion(rect)` holds: none unless both sides are positive.
    pub(crate) fn cells(self) -> Option<IRect> {
        let rect = IRect {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        };
        (!rect.is_empty()).then_some(rect)
    }

    /// Whether `QRegion::intersects(rect)` holds for a region whose band rectangles are `bands`
    /// ([`CellSet::rects`]): a rectangle of no width and no height intersects nothing;
    /// otherwise the rectangle is normalized as Qt normalizes it and compared, by inclusive
    /// edges, with each band rectangle. A rectangle one cell wide but of no height so
    /// "intersects" a band rectangle that spans its top edge.
    pub(crate) fn intersects(self, bands: &[IRect]) -> bool {
        if bands.is_empty() || (self.w == 0 && self.h == 0) {
            return false;
        }
        let (left, right) = normalized(self.x, self.w);
        let (top, bottom) = normalized(self.y, self.h);
        bands
            .iter()
            .any(|r| r.x + r.w > left && r.x <= right && r.y + r.h > top && r.y <= bottom)
    }
}

/// The first and last cell of a side, as `QRect::normalized` makes them: a negative side is
/// turned around; a side of 0 keeps its last cell one before its first.
fn normalized(start: i32, size: i32) -> (i32, i32) {
    let first = start;
    let last = start.saturating_add(size).saturating_sub(1);
    if last < first {
        (last.saturating_add(1), first.saturating_sub(1))
    } else {
        (first, last)
    }
}

/// The alignment Tiled gives `object` on `map` (`MapObject::alignment`): its tileset's for a
/// tile object that sets one, else bottom-centre on isometric maps and bottom-left elsewhere for
/// tile objects, and top-left for every other object.
fn alignment(map: &Map, object: &MapObject) -> Alignment {
    let Some(cell) = object.tile else {
        return Alignment::TopLeft;
    };
    match map.tileset_of(cell).map(|t| t.object_alignment) {
        Some(set) if set != Alignment::Unspecified => set,
        _ if map.orientation == Orientation::Isometric => Alignment::Bottom,
        _ => Alignment::BottomLeft,
    }
}

/// The cells `object` covers on `map`, as Tiled's `objectTileRect`: the object's box (its
/// position and size, aligned), its corners turned into tile coordinates, rounded outwards.
/// Rotation and a tile's offset are ignored, as in Tiled. `None` when a coordinate is not a
/// number or does not fit.
pub(crate) fn object_tile_rect(map: &Map, object: &MapObject) -> Option<QtRect> {
    let geometry = map.geometry();
    let min = object.pos - alignment(map, object).origin_fraction() * object.size;
    let max = min + object.size;
    let top_left = geometry.pixel_to_tile(min);
    let bottom_right = geometry.pixel_to_tile(max);
    let aligned = aligned(top_left, bottom_right)?;
    Some(aligned)
}

/// `QRectF(top_left, bottom_right).toAlignedRect()`: the left and top rounded down, the right
/// and bottom rounded up, the size what is between, negative when the corners are crossed.
fn aligned(top_left: DVec2, bottom_right: DVec2) -> Option<QtRect> {
    let min = top_left.floor();
    let max = bottom_right.ceil();
    let fits = |v: f64| v.is_finite() && v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX);
    if ![min.x, min.y, max.x, max.y].into_iter().all(fits) || !fits(max.x - min.x) {
        return None;
    }
    if !fits(max.y - min.y) {
        return None;
    }
    // The values were checked to fit, so the casts are exact.
    let size = (max - min).as_ivec2();
    let min = min.as_ivec2();
    Some(QtRect {
        x: min.x,
        y: min.y,
        w: size.x,
        h: size.y,
    })
}

/// The objects of `objects` whose cells, as [`object_tile_rect`] works them out on `map`,
/// intersect `region` as Qt's `QRegion::intersects` decides (Tiled's `objectsInRegion`).
pub(crate) fn objects_in_region<'o>(
    map: &Map,
    objects: &'o [MapObject],
    region: &CellSet,
) -> Vec<&'o MapObject> {
    let bands = region.rects();
    objects
        .iter()
        .filter(|object| object_tile_rect(map, object).is_some_and(|r| r.intersects(&bands)))
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::{Shape, TileId, TilesetSlot};

    use super::*;

    fn map(orientation: Orientation) -> Map {
        Map {
            orientation,
            width: 10,
            height: 10,
            tile_width: 16,
            tile_height: 16,
            ..Map::default()
        }
    }

    fn rectangle(x: f64, y: f64, w: f64, h: f64) -> MapObject {
        MapObject {
            pos: DVec2::new(x, y),
            size: DVec2::new(w, h),
            ..MapObject::default()
        }
    }

    #[test]
    fn rectangles_cover_the_cells_they_touch() {
        let map = map(Orientation::Orthogonal);
        let rect = object_tile_rect(&map, &rectangle(44.0, 12.0, 24.0, 24.0)).unwrap();
        // 44..68 by 12..36 pixels is columns 2..=4 and rows 0..=2.
        assert_eq!(
            rect,
            QtRect {
                x: 2,
                y: 0,
                w: 3,
                h: 3
            }
        );
        assert_eq!(
            rect.cells(),
            Some(IRect {
                x: 2,
                y: 0,
                w: 3,
                h: 3
            })
        );
    }

    #[test]
    fn points_cover_a_cell_only_inside_it() {
        let map = map(Orientation::Orthogonal);
        let point = |x, y| MapObject {
            shape: Shape::Point,
            ..rectangle(x, y, 0.0, 0.0)
        };
        let inside = object_tile_rect(&map, &point(24.0, 24.0)).unwrap();
        assert_eq!(
            inside.cells(),
            Some(IRect {
                x: 1,
                y: 1,
                w: 1,
                h: 1
            })
        );
        let corner = object_tile_rect(&map, &point(32.0, 32.0)).unwrap();
        assert_eq!(corner.cells(), None);
        let region = CellSet::from_rect(IRect {
            x: 0,
            y: 0,
            w: 5,
            h: 5,
        });
        assert!(inside.intersects(&region.rects()));
        assert!(
            !corner.intersects(&region.rects()),
            "a corner covers nothing"
        );
        // On a cell's edge: no width, one row. It meets a region only where the region spans
        // the edge.
        let edge = object_tile_rect(&map, &point(32.0, 24.0)).unwrap();
        assert_eq!(
            edge,
            QtRect {
                x: 2,
                y: 1,
                w: 0,
                h: 1
            }
        );
        assert!(edge.intersects(&region.rects()));
        let right_of_the_edge = CellSet::from_rect(IRect {
            x: 2,
            y: 0,
            w: 3,
            h: 3,
        });
        assert!(!edge.intersects(&right_of_the_edge.rects()));
    }

    #[test]
    fn tile_objects_are_aligned_bottom_left_or_bottom_centre_on_isometric_maps() {
        let mut ortho = map(Orientation::Orthogonal);
        ortho
            .tilesets
            .push(bt_core::TilesetRef::embedded(std::sync::Arc::new(
                bt_core::Tileset::default(),
            )));
        let tile = MapObject {
            tile: Some(bt_core::Cell::new(TilesetSlot::new(0).unwrap(), TileId(0))),
            ..rectangle(16.0, 32.0, 16.0, 16.0)
        };
        // Bottom-left at (16, 32): the box is 16..32 by 16..32, cell (1, 1).
        assert_eq!(
            object_tile_rect(&ortho, &tile).and_then(QtRect::cells),
            Some(IRect {
                x: 1,
                y: 1,
                w: 1,
                h: 1
            })
        );
        let mut iso = ortho.clone();
        iso.orientation = Orientation::Isometric;
        // Bottom-centre at (16, 32) on an isometric map: x 8..24, y 16..32, in units of the
        // tile height.
        assert_eq!(
            object_tile_rect(&iso, &tile).and_then(QtRect::cells),
            Some(IRect {
                x: 0,
                y: 1,
                w: 2,
                h: 1
            })
        );
    }

    #[test]
    fn crossed_corners_make_an_empty_region_but_are_normalized_for_intersections() {
        let rect = QtRect {
            x: 5,
            y: 0,
            w: -2,
            h: 1,
        };
        assert_eq!(rect.cells(), None);
        let left = CellSet::from_rect(IRect {
            x: 3,
            y: 0,
            w: 1,
            h: 1,
        });
        assert!(rect.intersects(&left.rects()));
        assert_eq!(normalized(5, -2), (3, 4));
        assert_eq!(normalized(5, 0), (5, 4));
    }
}
