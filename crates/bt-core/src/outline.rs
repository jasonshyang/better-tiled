//! The outline of something drawn: a filled area, an open line or a point, and what hit tests
//! and rubber bands ask of it. Areas are filled by the even-odd rule (see
//! [`crate::polygon_holds`]).

use bt_utils::DRect;
use glam::DVec2;

use crate::polygon::{polygon_holds, polygon_holds_or_bounds};
use crate::segment::{Segment, TOUCH, closed_segments, open_segments};

/// An outline, in whatever space its points are in.
#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    /// A filled shape: the closed polygon through the points.
    Area(Vec<DVec2>),
    /// An open line through the points (at least two).
    Path(Vec<DVec2>),
    /// A point.
    Point(DVec2),
}

impl Outline {
    /// The outline of a polygon's points: an area, or a line or a point when there are too few
    /// points to enclose one (no points at all are the origin).
    pub(crate) fn polygon(points: &[DVec2]) -> Outline {
        match points {
            [] => Outline::Point(DVec2::ZERO),
            [only] => Outline::Point(*only),
            [_, _] => Outline::Path(points.to_vec()),
            _ => Outline::Area(points.to_vec()),
        }
    }

    /// The outline of a polyline's points: a line, or a point when there is only one (no points
    /// at all are the origin).
    pub(crate) fn polyline(points: &[DVec2]) -> Outline {
        match points {
            [] => Outline::Point(DVec2::ZERO),
            [only] => Outline::Point(*only),
            _ => Outline::Path(points.to_vec()),
        }
    }

    /// The points.
    pub fn points(&self) -> &[DVec2] {
        match self {
            Outline::Area(points) | Outline::Path(points) => points,
            Outline::Point(point) => std::slice::from_ref(point),
        }
    }

    /// The points, taken out of the outline.
    pub fn into_points(self) -> Vec<DVec2> {
        match self {
            Outline::Area(points) | Outline::Path(points) => points,
            Outline::Point(point) => vec![point],
        }
    }

    /// The same outline with every point moved by `place`.
    pub(crate) fn mapped(self, place: impl Fn(DVec2) -> DVec2) -> Outline {
        match self {
            Outline::Area(points) => Outline::Area(points.into_iter().map(place).collect()),
            Outline::Path(points) => Outline::Path(points.into_iter().map(place).collect()),
            Outline::Point(point) => Outline::Point(place(point)),
        }
    }

    /// The smallest rectangle holding the outline; a point's has no size. An outline without
    /// points, which no object has, gives the empty rectangle at the origin.
    pub fn bounds(&self) -> DRect {
        DRect::from_points(self.points().iter().copied()).unwrap_or_default()
    }

    /// The outline's segments: an area's closed, a line's open, none for a point.
    pub fn segments(&self) -> Vec<Segment> {
        match self {
            Outline::Area(points) => closed_segments(points).collect(),
            Outline::Path(points) => open_segments(points).collect(),
            Outline::Point(_) => Vec::new(),
        }
    }

    /// Whether `p` is inside an area, or within `tolerance` of the outline.
    pub fn hits(&self, p: DVec2, tolerance: f64) -> bool {
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

    /// Whether the two outlines share a point: they cross or touch, or one lies inside the
    /// other's area.
    pub fn touches(&self, other: &Outline) -> bool {
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

    /// Whether the whole outline lies inside the closed polygon through `area`, its boundary
    /// included.
    pub fn lies_within(&self, area: &[DVec2]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// The corners of a rectangle, clockwise from `min`.
    fn corners(min: DVec2, size: DVec2) -> Vec<DVec2> {
        vec![
            min,
            min + DVec2::new(size.x, 0.0),
            min + size,
            min + DVec2::new(0.0, size.y),
        ]
    }

    #[test]
    fn outlines_touch_and_lie_within() {
        let square = Outline::Area(corners(v(0.0, 0.0), v(10.0, 10.0)));
        let inner = Outline::Area(corners(v(2.0, 2.0), v(2.0, 2.0)));
        let line = Outline::Path(vec![v(-5.0, 5.0), v(15.0, 5.0)]);
        let far = Outline::Point(v(20.0, 20.0));
        assert!(square.touches(&inner) && inner.touches(&square), "nested");
        assert!(square.touches(&line) && line.touches(&square), "crossing");
        assert!(!square.touches(&far) && !far.touches(&line));
        let band = corners(v(-1.0, -1.0), v(12.0, 12.0));
        assert!(square.lies_within(&band) && inner.lies_within(&band));
        assert!(!line.lies_within(&band));
        assert!(
            square.lies_within(&corners(v(0.0, 0.0), v(10.0, 10.0))),
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
    fn outlines_are_hit_inside_or_near() {
        let square = Outline::Area(corners(v(0.0, 0.0), v(10.0, 10.0)));
        assert!(square.hits(v(5.0, 5.0), 0.0));
        assert!(!square.hits(v(12.0, 5.0), 1.0) && square.hits(v(12.0, 5.0), 2.0));
        let line = Outline::Path(vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)]);
        assert!(!line.hits(v(5.0, 5.0), 0.0), "a line holds nothing");
        assert!(line.hits(v(11.0, 5.0), 1.0));
        let point = Outline::Point(v(3.0, 4.0));
        assert!(point.hits(v(0.0, 0.0), 5.0) && !point.hits(v(0.0, 0.0), 4.9));
        assert_eq!(
            point.bounds(),
            DRect::from_min_size(v(3.0, 4.0), DVec2::ZERO)
        );
        assert_eq!(
            line.bounds(),
            DRect::from_min_size(v(0.0, 0.0), v(10.0, 10.0))
        );
        assert_eq!(line.segments().len(), 2);
        assert_eq!(square.segments().len(), 4);
        assert_eq!(point.clone().into_points(), [v(3.0, 4.0)]);
    }

    #[test]
    fn polygons_with_too_few_points_are_lines_or_points() {
        assert_eq!(Outline::polygon(&[]), Outline::Point(DVec2::ZERO));
        assert_eq!(
            Outline::polygon(&[v(1.0, 2.0)]),
            Outline::Point(v(1.0, 2.0))
        );
        assert_eq!(
            Outline::polygon(&[v(1.0, 2.0), v(3.0, 4.0)]),
            Outline::Path(vec![v(1.0, 2.0), v(3.0, 4.0)])
        );
        assert_eq!(
            Outline::polyline(&[v(1.0, 2.0)]),
            Outline::Point(v(1.0, 2.0))
        );
        assert!(matches!(
            Outline::polyline(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)]),
            Outline::Path(_)
        ));
    }
}
