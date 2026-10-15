//! What an object covers as it is drawn, placed as libtiled's renderers place it
//! (`reference/tiled/src/libtiled/orthogonalrenderer.cpp`, `isometricrenderer.cpp`,
//! `maprenderer.cpp`, `mapobject.cpp`): rectangles, ellipses, capsules, points, polygons and
//! polylines in their [`ObjectFrame`], projected onto the screen, tile and text objects upright
//! at their projected position, all turned around it. Ellipses and capsules are polygons of as
//! many points as the renderer draws them with, so what the editor picks is what is drawn.
//!
//! An object's outline is the same wherever it is used: the renderer draws it, the editor's hit
//! tests, rubber bands and bounds measure it, and exports read it.

use bt_utils::DRect;
use glam::DVec2;

use crate::geometry::MapGeometry;
use crate::object::{MapObject, Shape};
use crate::object_frame::ObjectFrame;
use crate::outline::Outline;
use crate::tileset::{Alignment, Tileset};

/// Half the side of Tiled's marker for rectangles, ellipses and capsules without a size: a
/// square, or a circle for ellipses and capsules, in screen pixels.
const MARKER_RADIUS: f64 = 10.0;

/// The points an ellipse is drawn with.
const ELLIPSE_POINTS: u32 = 64;

/// The steps of each rounded end of a capsule.
const CAPSULE_STEPS: u32 = 16;

/// The outline of `object` as it is drawn, in screen space. `tileset` is the tileset of a tile
/// object's tile, when there is one to draw it with. Rectangles, ellipses and capsules without a
/// size are their position here; [`object_marker`] gives what Tiled draws in their place. A
/// rectangle with no width or no height is a line, and so is a flat ellipse or capsule, as the
/// renderer draws them. A polygon of fewer than three points is a line or a point, a polyline of
/// one point a point.
pub fn object_outline(
    geometry: &MapGeometry,
    object: &MapObject,
    tileset: Option<&Tileset>,
) -> Outline {
    let frame = ObjectFrame::of(geometry, object);
    object_local_outline(geometry, object, tileset).mapped(|p| frame.to_screen(p))
}

/// The outline of `object` as drawn (see [`object_outline`]), in pixel space: the pixel
/// positions drawn where it is drawn. It is exact for unturned shapes; turned shapes and upright
/// objects on isometric maps come back from the screen.
pub fn object_pixel_outline(
    geometry: &MapGeometry,
    object: &MapObject,
    tileset: Option<&Tileset>,
) -> Outline {
    let frame = ObjectFrame::of(geometry, object);
    object_local_outline(geometry, object, tileset).mapped(|p| frame.to_pixels(p))
}

/// Tiled's marker for a rectangle, ellipse or capsule without a size, in screen space: a square
/// of 20 pixels around its projected position, or a circle for ellipses and capsules, turned
/// with it (libtiled's `OrthogonalRenderer::drawMapObject` and `shape`; libtiled leaves such
/// shapes unmarked on isometric maps, better-tiled marks them on every grid). `None` for every
/// other object.
pub fn object_marker(geometry: &MapGeometry, object: &MapObject) -> Option<Outline> {
    if object.tile.is_some() || object.size != DVec2::ZERO {
        return None;
    }
    let min = DVec2::splat(-MARKER_RADIUS);
    let size = DVec2::splat(2.0 * MARKER_RADIUS);
    let points = match object.shape {
        Shape::Rectangle => rect_corners(min, size),
        Shape::Ellipse | Shape::Capsule => ellipse_points(min, size),
        Shape::Point | Shape::Polygon(_) | Shape::Polyline(_) | Shape::Text(_) => return None,
    };
    let frame = ObjectFrame::upright_of(geometry, object);
    Some(Outline::Area(points).mapped(|p| frame.to_screen(p)))
}

/// A tile object's image box in its [`ObjectFrame`] (screen pixels from its projected
/// position, before it turns), as libtiled's `boundingRect` of tile objects places it: its
/// size, or its tile's when it has none, as the renderer draws it; placed by its alignment (the
/// tileset's, else the grid's default); moved by the tileset's tile offset, scaled with the
/// image. A tile whose tileset is missing is drawn as a marker the size of the object, or of a
/// cell. `None` for an object without a tile.
pub fn tile_object_box(
    geometry: &MapGeometry,
    object: &MapObject,
    tileset: Option<&Tileset>,
) -> Option<DRect> {
    let cell = object.tile?;
    let image = tileset.map(|t| t.tile_pixel_size(cell.tile()).as_dvec2().max(DVec2::ONE));
    let sized = object.size.x > 0.0 && object.size.y > 0.0;
    let grid = DVec2::new(
        f64::from(geometry.tile_width),
        f64::from(geometry.tile_height),
    );
    let size = if sized {
        object.size
    } else {
        image.unwrap_or(grid)
    };
    let mut min = -tile_alignment(geometry, tileset).origin_fraction() * size;
    if let (Some(tileset), Some(image)) = (tileset, image) {
        min += tileset.tile_offset.as_dvec2() * (size / image);
    }
    Some(DRect::from_min_size(min, size))
}

/// The alignment of a tile object: its tileset's, else the grid's default for tile objects,
/// which is also how one whose tileset is missing is drawn.
fn tile_alignment(geometry: &MapGeometry, tileset: Option<&Tileset>) -> Alignment {
    match tileset {
        Some(tileset) => geometry.object_alignment(Some(tileset)),
        None => geometry.object_alignment(Some(&Tileset::default())),
    }
}

/// `object`'s outline in its own frame.
pub fn object_local_outline(
    geometry: &MapGeometry,
    object: &MapObject,
    tileset: Option<&Tileset>,
) -> Outline {
    if let Some(image) = tile_object_box(geometry, object, tileset) {
        return Outline::Area(rect_corners(image.min, image.size()));
    }
    let size = object.size;
    let flat = size.x == 0.0 || size.y == 0.0;
    match &object.shape {
        Shape::Text(_) => Outline::Area(rect_corners(DVec2::ZERO, size)),
        Shape::Point => Outline::Point(DVec2::ZERO),
        Shape::Rectangle | Shape::Ellipse | Shape::Capsule if size == DVec2::ZERO => {
            Outline::Point(DVec2::ZERO)
        }
        Shape::Rectangle => Outline::Area(rect_corners(DVec2::ZERO, size)),
        Shape::Ellipse | Shape::Capsule if flat => Outline::Area(rect_corners(DVec2::ZERO, size)),
        Shape::Ellipse => Outline::Area(ellipse_points(DVec2::ZERO, size)),
        Shape::Capsule => Outline::Area(capsule_points(DVec2::ZERO, size)),
        Shape::Polygon(points) => Outline::polygon(points),
        Shape::Polyline(points) => Outline::polyline(points),
    }
}

/// The corners of a rectangle, clockwise from `min`.
fn rect_corners(min: DVec2, size: DVec2) -> Vec<DVec2> {
    vec![
        min,
        min + DVec2::new(size.x, 0.0),
        min + size,
        min + DVec2::new(0.0, size.y),
    ]
}

/// The points of the ellipse inscribed in a rectangle, clockwise on screen from the middle of
/// its right side.
fn ellipse_points(min: DVec2, size: DVec2) -> Vec<DVec2> {
    let center = min + size * 0.5;
    let radius = size * 0.5;
    (0..ELLIPSE_POINTS)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from(i) / f64::from(ELLIPSE_POINTS);
            center + DVec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// The points of a capsule: a rectangle with its shorter sides rounded into half circles
/// (libtiled's rounded rectangle with a radius of half the shorter side).
fn capsule_points(min: DVec2, size: DVec2) -> Vec<DVec2> {
    let low = min.min(min + size);
    let extent = size.abs();
    let radius = extent.x.min(extent.y) * 0.5;
    let arc = |center: DVec2, from: f64| {
        (0..=CAPSULE_STEPS).map(move |i| {
            let angle = from + std::f64::consts::PI * f64::from(i) / f64::from(CAPSULE_STEPS);
            center + DVec2::new(angle.cos(), angle.sin()) * radius
        })
    };
    let half = std::f64::consts::FRAC_PI_2;
    let near = low + DVec2::splat(radius);
    if extent.x >= extent.y {
        let far = low + DVec2::new(extent.x - radius, radius);
        arc(far, -half).chain(arc(near, half)).collect()
    } else {
        let far = low + DVec2::new(radius, extent.y - radius);
        arc(far, 0.0)
            .chain(arc(near, std::f64::consts::PI))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec2;

    use super::*;
    use crate::cell::Cell;
    use crate::ids::{TileId, TilesetSlot};
    use crate::map::Orientation;
    use crate::text::TextData;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        (a - b).length() < 1e-9
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> DRect {
        DRect::from_min_size(v(x, y), v(w, h))
    }

    fn assert_rect_near(actual: DRect, expected: DRect) {
        assert!(
            close(actual.min, expected.min) && close(actual.max, expected.max),
            "{actual:?} is not {expected:?}"
        );
    }

    fn square_grid() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(8))
    }

    fn shape(shape: Shape, x: f64, y: f64, w: f64, h: f64) -> MapObject {
        MapObject {
            shape,
            pos: v(x, y),
            size: v(w, h),
            ..MapObject::default()
        }
    }

    fn tile_object(x: f64, y: f64, w: f64, h: f64) -> MapObject {
        MapObject {
            tile: Some(Cell::new(TilesetSlot::new(0).unwrap(), TileId(1))),
            ..shape(Shape::Rectangle, x, y, w, h)
        }
    }

    /// A tileset of `w` × `h` tiles moved by `offset`.
    fn tileset(w: u32, h: u32, offset: IVec2) -> Tileset {
        Tileset {
            tile_width: w,
            tile_height: h,
            tile_offset: offset,
            ..Tileset::default()
        }
    }

    #[test]
    fn shapes_cover_their_box_turned_around_their_position() {
        let grid = square_grid();
        let bounds = |o: &MapObject| object_pixel_outline(&grid, o, None).bounds();
        let box_ = shape(Shape::Rectangle, 10.0, 20.0, 30.0, 10.0);
        assert_eq!(bounds(&box_), rect(10.0, 20.0, 30.0, 10.0));
        let turned = MapObject {
            rotation: 90.0,
            ..box_.clone()
        };
        assert_eq!(bounds(&turned), rect(0.0, 20.0, 10.0, 30.0));
        let ellipse = MapObject {
            shape: Shape::Ellipse,
            rotation: 45.0,
            size: v(20.0, 20.0),
            ..box_.clone()
        };
        assert_rect_near(
            bounds(&ellipse),
            DRect::from_min_size(
                // The centre, (10, 10) from the position, turns to straight below it.
                v(10.0, 20.0) + v(0.0, 10.0 * 2f64.sqrt()) - v(10.0, 10.0),
                v(20.0, 20.0),
            ),
        );
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(10.0, -5.0), v(4.0, 8.0)]),
            ..box_.clone()
        };
        assert_eq!(bounds(&polygon), rect(10.0, 15.0, 10.0, 13.0));
        let point = shape(Shape::Point, 10.0, 20.0, 0.0, 0.0);
        assert_eq!(
            object_outline(&grid, &point, None),
            Outline::Point(v(10.0, 20.0))
        );
        let marker = shape(Shape::Rectangle, 10.0, 20.0, 0.0, 0.0);
        assert_eq!(
            bounds(&marker),
            rect(10.0, 20.0, 0.0, 0.0),
            "no size, no marker"
        );
    }

    #[test]
    fn shapes_are_projected_and_upright_objects_are_not() {
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::splat(4));
        // One cell's square of pixels is drawn as the cell's diamond.
        let cell = shape(Shape::Rectangle, 32.0, 0.0, 32.0, 32.0);
        let drawn = object_outline(&iso, &cell, None);
        let diamond =
            [v(1.0, 0.0), v(2.0, 0.0), v(2.0, 1.0), v(1.0, 1.0)].map(|t| iso.tile_to_screen(t));
        for (point, corner) in drawn.points().iter().zip(diamond) {
            assert!(close(*point, corner), "{point} {corner}");
        }
        assert_eq!(
            object_pixel_outline(&iso, &cell, None).bounds(),
            rect(32.0, 0.0, 32.0, 32.0),
            "unturned shapes are exact in pixel space"
        );
        // Text stands upright at its projected position.
        let text = MapObject {
            shape: Shape::Text(TextData::default()),
            ..cell
        };
        let screen = iso.pixel_to_screen(text.pos);
        assert_eq!(
            object_outline(&iso, &text, None).bounds(),
            DRect::from_min_size(screen, v(32.0, 32.0))
        );
    }

    #[test]
    fn tile_objects_cover_their_image_by_alignment_and_offset() {
        let grid = square_grid();
        let tiles = tileset(16, 24, IVec2::new(4, -2));
        let bounds = |o: &MapObject, t: &Tileset| object_pixel_outline(&grid, o, Some(t)).bounds();
        // Bottom-left: the image hangs up from the position, moved by the offset; at twice the
        // tile's size the offset doubles.
        let tile = tile_object(100.0, 100.0, 32.0, 48.0);
        assert_eq!(bounds(&tile, &tiles), rect(108.0, 48.0, 32.0, 48.0));
        assert_eq!(
            tile_object_box(&grid, &tile, Some(&tiles)),
            Some(rect(8.0, -52.0, 32.0, 48.0))
        );
        let sizeless = tile_object(100.0, 100.0, 0.0, 0.0);
        assert_eq!(
            bounds(&sizeless, &tiles),
            rect(104.0, 74.0, 16.0, 24.0),
            "the tile's own size when the object has none"
        );
        let turned = MapObject {
            rotation: -90.0,
            ..tile.clone()
        };
        // (8, -52)..(40, -4) around the position, turned a quarter to the left.
        assert_eq!(bounds(&turned, &tiles), rect(48.0, 60.0, 48.0, 32.0));
        let centered = Tileset {
            object_alignment: Alignment::Center,
            ..tiles.clone()
        };
        assert_eq!(bounds(&tile, &centered), rect(92.0, 72.0, 32.0, 48.0));
        // Without its tileset it is drawn as a marker the size of the object, or of a cell.
        assert_eq!(
            tile_object_box(&grid, &sizeless, None),
            Some(rect(0.0, -16.0, 16.0, 16.0))
        );
        assert_eq!(
            tile_object_box(&grid, &shape(Shape::Rectangle, 0.0, 0.0, 4.0, 4.0), None),
            None
        );
    }

    #[test]
    fn isometric_tile_objects_stand_upright_at_their_projected_position() {
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(32, 16), IVec2::splat(8));
        let tiles = tileset(32, 32, IVec2::ZERO);
        let tile = tile_object(32.0, 32.0, 32.0, 32.0);
        let foot = iso.pixel_to_screen(tile.pos);
        // Bottom-centre on isometric maps.
        assert_eq!(
            object_outline(&iso, &tile, Some(&tiles)).bounds(),
            DRect::from_min_size(foot - v(16.0, 32.0), v(32.0, 32.0))
        );
        // In pixel space the upright image is a diamond around the position.
        let bounds = object_pixel_outline(&iso, &tile, Some(&tiles)).bounds();
        let corners = [v(-16.0, -32.0), v(16.0, -32.0), v(16.0, 0.0), v(-16.0, 0.0)]
            .map(|c| iso.screen_to_pixel(foot + c));
        assert_rect_near(bounds, DRect::from_points(corners).unwrap());
    }

    #[test]
    fn sizeless_shapes_are_drawn_as_tileds_marker() {
        let grid = square_grid();
        let marker = shape(Shape::Rectangle, 50.0, 50.0, 0.0, 0.0);
        assert_eq!(
            object_outline(&grid, &marker, None),
            Outline::Point(v(50.0, 50.0))
        );
        let square = object_marker(&grid, &marker).unwrap();
        assert_eq!(square.bounds(), rect(40.0, 40.0, 20.0, 20.0));
        assert!(square.hits(v(59.0, 41.0), 0.0) && !square.hits(v(61.0, 50.0), 0.0));
        for round in [Shape::Ellipse, Shape::Capsule] {
            let circle = object_marker(
                &grid,
                &MapObject {
                    shape: round,
                    ..marker.clone()
                },
            )
            .unwrap();
            assert!(
                !circle.hits(v(59.0, 41.0), 0.0),
                "a circle leaves the corners"
            );
            assert!(circle.hits(v(59.0, 50.0), 0.0));
        }
        let turned = object_marker(
            &grid,
            &MapObject {
                rotation: 45.0,
                ..marker.clone()
            },
        )
        .unwrap();
        assert!(
            !turned.hits(v(59.0, 59.0), 0.0),
            "the turned square's corner is elsewhere"
        );
        assert!(turned.hits(v(63.0, 50.0), 0.0));
        // Shapes with a size, points and tile objects have none.
        assert_eq!(
            object_marker(&grid, &shape(Shape::Rectangle, 0.0, 0.0, 1.0, 0.0)),
            None
        );
        assert_eq!(
            object_marker(&grid, &shape(Shape::Point, 0.0, 0.0, 0.0, 0.0)),
            None
        );
        assert_eq!(object_marker(&grid, &tile_object(0.0, 0.0, 0.0, 0.0)), None);
        // On isometric maps the marker stands upright at the projected position.
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::splat(4));
        let foot = iso.pixel_to_screen(marker.pos);
        assert_eq!(
            object_marker(&iso, &marker).unwrap().bounds(),
            DRect::from_min_size(foot - v(10.0, 10.0), v(20.0, 20.0))
        );
    }

    #[test]
    fn flat_shapes_are_lines_and_polygons_keep_their_points() {
        let grid = square_grid();
        let flat = shape(Shape::Ellipse, 0.0, 0.0, 10.0, 0.0);
        assert_eq!(
            object_outline(&grid, &flat, None).points().len(),
            4,
            "a flat box"
        );
        let ellipse = shape(Shape::Ellipse, 0.0, 0.0, 10.0, 6.0);
        assert_eq!(object_outline(&grid, &ellipse, None).points().len(), 64);
        let capsule = shape(Shape::Capsule, 0.0, 0.0, 10.0, 6.0);
        assert_eq!(object_outline(&grid, &capsule, None).points().len(), 34);
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(5.0, 0.0)]),
            ..shape(Shape::Rectangle, 2.0, 3.0, 0.0, 0.0)
        };
        assert_eq!(
            object_outline(&grid, &polygon, None),
            Outline::Path(vec![v(2.0, 3.0), v(7.0, 3.0)])
        );
        let polyline = MapObject {
            shape: Shape::Polyline(vec![v(1.0, 1.0)]),
            ..polygon
        };
        assert_eq!(
            object_outline(&grid, &polyline, None),
            Outline::Point(v(3.0, 4.0))
        );
    }

    #[test]
    fn capsules_round_their_short_sides() {
        let wide = capsule_points(DVec2::ZERO, v(40.0, 10.0));
        let bounds = DRect::from_points(wide.iter().copied()).unwrap();
        assert!(close(bounds.min, v(0.0, 0.0)) && close(bounds.max, v(40.0, 10.0)));
        let tall = capsule_points(DVec2::ZERO, v(10.0, 40.0));
        let bounds = DRect::from_points(tall.iter().copied()).unwrap();
        assert!(close(bounds.max, v(10.0, 40.0)));
        let area = Outline::Area(wide);
        assert!(area.hits(v(20.0, 0.5), 0.0), "the long side is straight");
        assert!(!area.hits(v(0.5, 0.5), 0.0), "the corner is rounded off");
        let backwards = capsule_points(DVec2::ZERO, v(-40.0, 10.0));
        let bounds = DRect::from_points(backwards.iter().copied()).unwrap();
        assert!(
            close(bounds.min, v(-40.0, 0.0)),
            "a negative size extends the other way"
        );
    }
}
