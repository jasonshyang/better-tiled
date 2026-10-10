//! Points, segments and polygons in the plane: turning, distances, containment and crossings,
//! what object outlines, hit tests and rubber bands are made of. Polygons are filled by the
//! even-odd rule, as Qt fills the paths Tiled picks objects with.

use glam::DVec2;

/// How close two points must be to touch: far below anything visible, above the rounding of
/// the coordinates a map holds.
pub(crate) const TOUCH: f64 = 1e-9;

/// A straight piece of an outline.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Segment {
    /// Where it starts.
    pub(crate) from: DVec2,
    /// Where it ends.
    pub(crate) to: DVec2,
}

impl Segment {
    /// The distance from `p` to the nearest point of the segment.
    pub(crate) fn distance_to(self, p: DVec2) -> f64 {
        let along = self.to - self.from;
        let length_squared = along.length_squared();
        let t = if length_squared > 0.0 {
            ((p - self.from).dot(along) / length_squared).clamp(0.0, 1.0)
        } else {
            0.0
        };
        p.distance(self.from + along * t)
    }

    /// Whether the two segments share a point.
    pub(crate) fn touches(self, other: Segment) -> bool {
        self.crosses(other)
            || other.distance_to(self.from) <= TOUCH
            || other.distance_to(self.to) <= TOUCH
            || self.distance_to(other.from) <= TOUCH
            || self.distance_to(other.to) <= TOUCH
    }

    /// Whether the segments cross at a point inside both: not at an end of either, and not
    /// along a line they share.
    pub(crate) fn crosses(self, other: Segment) -> bool {
        let side = |s: Segment, p: DVec2| (s.to - s.from).perp_dot(p - s.from);
        let from_side = side(other, self.from);
        let to_side = side(other, self.to);
        let other_from_side = side(self, other.from);
        let other_to_side = side(self, other.to);
        opposite(from_side, to_side) && opposite(other_from_side, other_to_side)
    }
}

/// Whether two signed areas lie strictly on opposite sides of zero.
fn opposite(a: f64, b: f64) -> bool {
    (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0)
}

/// `v` turned clockwise on screen (y down) by `degrees`, as Qt's `QTransform::rotate` turns it.
/// Whole quarter turns are exact.
pub(crate) fn rotated(v: DVec2, degrees: f64) -> DVec2 {
    let quarters = degrees / 90.0;
    if quarters == quarters.round() {
        // A whole number of quarter turns; `rem_euclid` brings it into 0..4.
        let turn = quarters.rem_euclid(4.0);
        return if turn == 0.0 {
            v
        } else if turn == 1.0 {
            DVec2::new(-v.y, v.x)
        } else if turn == 2.0 {
            -v
        } else {
            DVec2::new(v.y, -v.x)
        };
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    DVec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

/// The segments of an open line through `points`.
pub(crate) fn open_segments(points: &[DVec2]) -> impl Iterator<Item = Segment> + '_ {
    points.windows(2).filter_map(|pair| match pair {
        [from, to] => Some(Segment {
            from: *from,
            to: *to,
        }),
        _ => None,
    })
}

/// The segments of the closed polygon through `points`, the closing one included (a polygon of
/// one point is a segment of no length).
pub(crate) fn closed_segments(points: &[DVec2]) -> impl Iterator<Item = Segment> + '_ {
    let closing = match (points.last(), points.first()) {
        (Some(&from), Some(&to)) => Some(Segment { from, to }),
        _ => None,
    };
    open_segments(points).chain(closing)
}

/// Whether `p` is inside the closed polygon through `points`, by the even-odd rule. A point on
/// the boundary may count either way; [`polygon_holds_or_bounds`] counts it in.
pub(crate) fn polygon_holds(points: &[DVec2], p: DVec2) -> bool {
    let mut inside = false;
    for edge in closed_segments(points) {
        let a = edge.from;
        let b = edge.to;
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// Whether `p` is inside the closed polygon through `points` or on its boundary.
pub(crate) fn polygon_holds_or_bounds(points: &[DVec2], p: DVec2) -> bool {
    polygon_holds(points, p) || closed_segments(points).any(|edge| edge.distance_to(p) <= TOUCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn segments_measure_and_cross() {
        let s = Segment {
            from: v(0.0, 0.0),
            to: v(10.0, 0.0),
        };
        assert_eq!(s.distance_to(v(5.0, 3.0)), 3.0);
        assert_eq!(s.distance_to(v(13.0, 4.0)), 5.0, "past the end");
        let point = Segment {
            from: v(1.0, 1.0),
            to: v(1.0, 1.0),
        };
        assert_eq!(point.distance_to(v(4.0, 5.0)), 5.0);
        let across = Segment {
            from: v(5.0, -5.0),
            to: v(5.0, 5.0),
        };
        assert!(s.crosses(across) && s.touches(across));
        let meeting = Segment {
            from: v(10.0, 0.0),
            to: v(10.0, 5.0),
        };
        assert!(!s.crosses(meeting) && s.touches(meeting), "ends meet");
        let along = Segment {
            from: v(4.0, 0.0),
            to: v(20.0, 0.0),
        };
        assert!(!s.crosses(along) && s.touches(along), "a shared line");
        let apart = Segment {
            from: v(11.0, 0.0),
            to: v(20.0, 0.0),
        };
        assert!(!s.touches(apart));
    }

    #[test]
    fn quarter_turns_are_exact_and_clockwise() {
        let x = v(1.0, 0.0);
        assert_eq!(rotated(x, 90.0), v(0.0, 1.0), "down the screen");
        assert_eq!(rotated(x, 180.0), v(-1.0, 0.0));
        assert_eq!(rotated(x, -90.0), v(0.0, -1.0));
        assert_eq!(rotated(x, 630.0), v(0.0, -1.0));
        assert_eq!(rotated(x, 0.0), x);
        let turned = rotated(v(2.0, 0.0), 30.0);
        assert!((turned - v(3f64.sqrt(), 1.0)).length() < 1e-12);
    }

    #[test]
    fn polygons_hold_by_the_even_odd_rule() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(polygon_holds(&square, v(5.0, 5.0)));
        assert!(!polygon_holds(&square, v(15.0, 5.0)));
        assert!(polygon_holds_or_bounds(&square, v(10.0, 5.0)));
        // A star drawn in one stroke: its middle crosses twice and is out.
        let star = [
            v(0.0, 3.0),
            v(10.0, 3.0),
            v(2.0, 9.0),
            v(5.0, 0.0),
            v(8.0, 9.0),
        ];
        assert!(!polygon_holds(&star, v(5.0, 5.0)));
        assert!(polygon_holds(&star, v(5.0, 1.5)));
        assert_eq!(closed_segments(&square).count(), 4);
        assert_eq!(open_segments(&square).count(), 3);
        assert_eq!(closed_segments(&[v(1.0, 1.0)]).count(), 1);
    }
}
