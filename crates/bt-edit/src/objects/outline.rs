//! What an object covers as it is drawn, placed as Tiled's renderers and its `MapObjectItem`
//! place it (`reference/tiled/src/libtiled/orthogonalrenderer.cpp`, `isometricrenderer.cpp`,
//! `obliquerenderer.cpp`, `maprenderer.cpp`; `tiled/mapobjectitem.cpp`): shapes are projected
//! from pixel space onto the screen, tile and text objects stand upright at their projected
//! position, and the whole turns clockwise on screen around that position. Ellipses and capsules
//! are drawn with as many points as `bt-render` draws them with, so what is picked is what is
//! drawn.
//!
//! Outlines are in screen space, where the renderer draws, or in pixel space, where objects
//! are stored; the operations convert at their edges.

use bt_core::{Cell, MapGeometry, MapObject, Shape};
use bt_utils::DRect;
use glam::DVec2;

use super::polygon::{
    Segment, TOUCH, closed_segments, open_segments, polygon_holds, polygon_holds_or_bounds, rotated,
};
use super::space::ObjectSpace;

/// Half the side of Tiled's marker for rectangles, ellipses and capsules without a size: a
/// square, or a circle for ellipses and capsules, in screen pixels.
const MARKER_RADIUS: f64 = 10.0;

/// The points an ellipse is drawn with.
const ELLIPSE_POINTS: u32 = 64;

/// The steps of each rounded end of a capsule.
const CAPSULE_STEPS: u32 = 16;

/// An object's outline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Outline {
    /// A filled shape: the closed polygon through the points.
    Area(Vec<DVec2>),
    /// An open line through the points (at least two).
    Path(Vec<DVec2>),
    /// A point.
    Point(DVec2),
}

impl Outline {
    /// The outline of a polygon object's points: an area, or a line or point when there are too
    /// few points to enclose one.
    fn polygon(points: &[DVec2]) -> Outline {
        match points {
            [] => Outline::Point(DVec2::ZERO),
            [only] => Outline::Point(*only),
            [_, _] => Outline::Path(points.to_vec()),
            _ => Outline::Area(points.to_vec()),
        }
    }

    /// The outline of a polyline object's points.
    fn polyline(points: &[DVec2]) -> Outline {
        match points {
            [] => Outline::Point(DVec2::ZERO),
            [only] => Outline::Point(*only),
            _ => Outline::Path(points.to_vec()),
        }
    }

    /// The points.
    pub(crate) fn points(&self) -> &[DVec2] {
        match self {
            Outline::Area(points) | Outline::Path(points) => points,
            Outline::Point(point) => std::slice::from_ref(point),
        }
    }

    /// The same outline with every point moved by `place`.
    fn mapped(self, place: impl Fn(DVec2) -> DVec2) -> Outline {
        match self {
            Outline::Area(points) => Outline::Area(points.into_iter().map(place).collect()),
            Outline::Path(points) => Outline::Path(points.into_iter().map(place).collect()),
            Outline::Point(point) => Outline::Point(place(point)),
        }
    }

    /// The smallest rectangle holding the outline; a point's has no size.
    pub(crate) fn bounds(&self) -> DRect {
        // Every outline has a point, so the fallback is never taken.
        DRect::from_points(self.points().iter().copied()).unwrap_or_default()
    }

    /// The outline's segments: an area's closed, a line's open, none for a point.
    fn segments(&self) -> Vec<Segment> {
        match self {
            Outline::Area(points) => closed_segments(points).collect(),
            Outline::Path(points) => open_segments(points).collect(),
            Outline::Point(_) => Vec::new(),
        }
    }

    /// Whether `p` is inside an area, or within `tolerance` of the outline.
    pub(crate) fn hits(&self, p: DVec2, tolerance: f64) -> bool {
        match self {
            Outline::Area(points) => {
                polygon_holds(points, p)
                    || closed_segments(points).any(|edge| edge.distance_to(p) <= tolerance)
            }
            Outline::Path(points) => {
                open_segments(points).any(|edge| edge.distance_to(p) <= tolerance)
            }
            Outline::Point(point) => point.distance(p) <= tolerance,
        }
    }

    /// Whether the two outlines share a point.
    pub(crate) fn touches(&self, other: &Outline) -> bool {
        match (self, other) {
            (Outline::Point(p), _) => other.hits(*p, TOUCH),
            (_, Outline::Point(p)) => self.hits(*p, TOUCH),
            _ => {
                let mine = self.segments();
                let theirs = other.segments();
                mine.iter().any(|a| theirs.iter().any(|b| a.touches(*b)))
                    || self.encloses_start_of(other)
                    || other.encloses_start_of(self)
            }
        }
    }

    /// Whether this is an area holding the first point of `other` (which then lies wholly
    /// inside when no segments cross).
    fn encloses_start_of(&self, other: &Outline) -> bool {
        match (self, other.points().first()) {
            (Outline::Area(points), Some(&start)) => polygon_holds(points, start),
            _ => false,
        }
    }

    /// Whether the whole outline lies inside the closed polygon through `area` (its boundary
    /// included).
    pub(crate) fn lies_within(&self, area: &[DVec2]) -> bool {
        let edges: Vec<Segment> = closed_segments(area).collect();
        self.points()
            .iter()
            .all(|&p| polygon_holds_or_bounds(area, p))
            && self
                .segments()
                .iter()
                .all(|s| edges.iter().all(|edge| !s.crosses(*edge)))
    }
}

/// Where an object's own coordinates are drawn: a shape's are pixels from its position,
/// projected; an upright object's are screen pixels from its projected position. Both turn
/// clockwise on screen around the projected position.
#[derive(Copy, Clone, Debug)]
struct Placement {
    geometry: MapGeometry,
    pos: DVec2,
    screen: DVec2,
    rotation: f64,
    upright: bool,
}

impl Placement {
    /// Where `object`'s coordinates are drawn, as a shape's or, when `upright`, as a tile or text
    /// object's.
    fn of(geometry: MapGeometry, object: &MapObject, upright: bool) -> Placement {
        Placement {
            geometry,
            pos: object.pos,
            screen: geometry.pixel_to_screen(object.pos),
            rotation: object.rotation,
            upright,
        }
    }

    /// Where `local` is drawn.
    fn screen_of(&self, local: DVec2) -> DVec2 {
        if self.upright {
            self.screen + rotated(local, self.rotation)
        } else if self.rotation == 0.0 {
            self.geometry.pixel_to_screen(self.pos + local)
        } else {
            let projected = self.geometry.pixel_to_screen(self.pos + local) - self.screen;
            self.screen + rotated(projected, self.rotation)
        }
    }

    /// The pixel position drawn where `local` is: exact for unturned shapes and for the
    /// position itself.
    fn pixel_of(&self, local: DVec2) -> DVec2 {
        if local == DVec2::ZERO {
            self.pos
        } else if !self.upright && self.rotation == 0.0 {
            self.pos + local
        } else {
            self.geometry.screen_to_pixel(self.screen_of(local))
        }
    }
}

/// An outline in an object's own coordinates, with where they are drawn.
struct Local {
    outline: Outline,
    placement: Placement,
}

impl Local {
    /// The outline where it is drawn.
    fn in_screen(self) -> Outline {
        let placement = self.placement;
        self.outline.mapped(|p| placement.screen_of(p))
    }

    /// The outline in pixel space: the pixels drawn where it is drawn.
    fn in_pixels(self) -> Outline {
        let placement = self.placement;
        self.outline.mapped(|p| placement.pixel_of(p))
    }
}

/// The outline of `object` as drawn, in screen space. Rectangles, ellipses and capsules
/// without a size are their position.
pub(crate) fn drawn_outline(space: &ObjectSpace, object: &MapObject) -> Outline {
    local(space, object).in_screen()
}

/// The outline of `object` as drawn, in pixel space.
pub(crate) fn pixel_outline(space: &ObjectSpace, object: &MapObject) -> Outline {
    local(space, object).in_pixels()
}

/// The outline hit tests use, in screen space: the drawn one, with Tiled's marker for
/// rectangles, ellipses and capsules without a size (a 20-pixel square, or circle for
/// ellipses and capsules), turned with the object.
pub(crate) fn hit_outline(space: &ObjectSpace, object: &MapObject) -> Outline {
    let sizeless = object.tile.is_none() && object.size == DVec2::ZERO;
    let marker = match &object.shape {
        Shape::Rectangle if sizeless => Some(rect_corners(
            DVec2::splat(-MARKER_RADIUS),
            DVec2::splat(2.0 * MARKER_RADIUS),
        )),
        Shape::Ellipse | Shape::Capsule if sizeless => Some(ellipse_points(
            DVec2::splat(-MARKER_RADIUS),
            DVec2::splat(2.0 * MARKER_RADIUS),
        )),
        _ => None,
    };
    match marker {
        Some(points) => Local {
            outline: Outline::Area(points),
            placement: Placement::of(*space.geometry(), object, true),
        }
        .in_screen(),
        None => drawn_outline(space, object),
    }
}

/// An object's outline in its own coordinates.
fn local(space: &ObjectSpace, object: &MapObject) -> Local {
    let geometry = *space.geometry();
    let upright = |outline| Local {
        outline,
        placement: Placement::of(geometry, object, true),
    };
    let projected = |outline| Local {
        outline,
        placement: Placement::of(geometry, object, false),
    };
    if let Some(cell) = object.tile {
        let image = tile_box(space, object, cell);
        return upright(Outline::Area(rect_corners(image.min, image.size())));
    }
    let size = object.size;
    let flat = size.x == 0.0 || size.y == 0.0;
    match &object.shape {
        Shape::Text(_) => upright(Outline::Area(rect_corners(DVec2::ZERO, size))),
        Shape::Point => projected(Outline::Point(DVec2::ZERO)),
        Shape::Rectangle | Shape::Ellipse | Shape::Capsule if size == DVec2::ZERO => {
            projected(Outline::Point(DVec2::ZERO))
        }
        // A rectangle of no width or height is a line; so are flat ellipses and capsules, as
        // the renderer draws them.
        Shape::Rectangle => projected(Outline::Area(rect_corners(DVec2::ZERO, size))),
        Shape::Ellipse | Shape::Capsule if flat => {
            projected(Outline::Area(rect_corners(DVec2::ZERO, size)))
        }
        Shape::Ellipse => projected(Outline::Area(ellipse_points(DVec2::ZERO, size))),
        Shape::Capsule => projected(Outline::Area(capsule_points(DVec2::ZERO, size))),
        Shape::Polygon(points) => projected(Outline::polygon(points)),
        Shape::Polyline(points) => projected(Outline::polyline(points)),
    }
}

/// A tile object's image box in screen pixels around its position, before it turns (Tiled's
/// `boundingRect` of tile objects): its size, or its tile's when it has none, as the renderer
/// draws it; placed by its alignment and moved by the tileset's tile offset, scaled with it. A
/// tile whose tileset the space lacks is drawn as a marker the size of the object, or of a cell.
pub(crate) fn tile_box(space: &ObjectSpace, object: &MapObject, cell: Cell) -> DRect {
    let geometry = space.geometry();
    let tileset = space.tileset_of(cell);
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
    let mut min = -space.alignment_of(object).origin_fraction() * size;
    if let (Some(tileset), Some(image)) = (tileset, image) {
        min += tileset.tile_offset.as_dvec2() * (size / image);
    }
    DRect::from_min_size(min, size)
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

/// The points of the ellipse inscribed in a rectangle.
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

/// The points of a capsule: a rectangle with its shorter sides rounded into half circles.
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
    use bt_core::{Map, Orientation};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn shapes_are_projected_and_upright_objects_are_not() {
        let map = Map {
            orientation: Orientation::Isometric,
            width: 4,
            height: 4,
            tile_width: 64,
            tile_height: 32,
            ..Map::default()
        };
        let space = ObjectSpace::of_map(&map);
        let geometry = map.geometry();
        // One cell's square of pixels is drawn as the cell's diamond.
        let cell = MapObject {
            pos: v(32.0, 0.0),
            size: v(32.0, 32.0),
            ..MapObject::default()
        };
        let drawn = drawn_outline(&space, &cell);
        let diamond = [v(1.0, 0.0), v(2.0, 0.0), v(2.0, 1.0), v(1.0, 1.0)]
            .map(|t| geometry.tile_to_screen(t));
        for (point, corner) in drawn.points().iter().zip(diamond) {
            assert!(close(*point, corner), "{point} {corner}");
        }
        assert_eq!(
            pixel_outline(&space, &cell).bounds(),
            DRect::from_min_size(v(32.0, 0.0), v(32.0, 32.0)),
            "unturned shapes are exact in pixel space"
        );
        // Text stands upright at its projected position.
        let text = MapObject {
            shape: Shape::Text(bt_core::TextData::default()),
            ..cell
        };
        let screen = geometry.pixel_to_screen(text.pos);
        assert_eq!(
            drawn_outline(&space, &text).bounds(),
            DRect::from_min_size(screen, v(32.0, 32.0))
        );
    }

    #[test]
    fn sizeless_shapes_are_points_drawn_and_markers_hit() {
        let map = Map {
            tile_width: 16,
            tile_height: 16,
            ..Map::default()
        };
        let space = ObjectSpace::of_map(&map);
        let marker = MapObject {
            pos: v(50.0, 50.0),
            ..MapObject::default()
        };
        assert_eq!(
            drawn_outline(&space, &marker),
            Outline::Point(v(50.0, 50.0))
        );
        let hit = hit_outline(&space, &marker);
        assert_eq!(
            hit.bounds(),
            DRect::from_min_size(v(40.0, 40.0), v(20.0, 20.0))
        );
        assert!(hit.hits(v(59.0, 41.0), 0.0) && !hit.hits(v(61.0, 50.0), 0.0));
        let circle = hit_outline(
            &space,
            &MapObject {
                shape: Shape::Ellipse,
                ..marker
            },
        );
        assert!(
            !circle.hits(v(59.0, 41.0), 0.0),
            "a circle leaves the corners"
        );
        assert!(circle.hits(v(59.0, 50.0), 0.0));
    }

    #[test]
    fn outlines_touch_and_lie_within() {
        let square = Outline::Area(rect_corners(v(0.0, 0.0), v(10.0, 10.0)));
        let inner = Outline::Area(rect_corners(v(2.0, 2.0), v(2.0, 2.0)));
        let line = Outline::Path(vec![v(-5.0, 5.0), v(15.0, 5.0)]);
        let far = Outline::Point(v(20.0, 20.0));
        assert!(square.touches(&inner) && inner.touches(&square), "nested");
        assert!(square.touches(&line) && line.touches(&square), "crossing");
        assert!(!square.touches(&far) && !far.touches(&line));
        let band = rect_corners(v(-1.0, -1.0), v(12.0, 12.0));
        assert!(square.lies_within(&band) && inner.lies_within(&band));
        assert!(!line.lies_within(&band));
        assert!(
            square.lies_within(&rect_corners(v(0.0, 0.0), v(10.0, 10.0))),
            "the boundary counts"
        );
        // A concave band holding every corner of the square still cuts into its bottom side.
        let notched = [
            v(-1.0, -1.0),
            v(11.0, -1.0),
            v(11.0, 11.0),
            v(6.0, 11.0),
            v(5.0, 8.0),
            v(4.0, 11.0),
            v(-1.0, 11.0),
        ];
        assert!(
            square
                .points()
                .iter()
                .all(|&p| polygon_holds_or_bounds(&notched, p))
        );
        assert!(!square.lies_within(&notched));
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
    }
}
