//! Straight pieces of outlines: where on one a point is nearest, how far it is, whether two
//! touch or cross, and the pieces of open lines and closed polygons.

use glam::DVec2;

/// How close two points must be to touch: far below anything visible, above the rounding of
/// the coordinates a map holds.
pub(crate) const TOUCH: f64 = 1e-9;

/// A straight piece of an outline, from one point to another.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Segment {
    /// Where it starts.
    pub from: DVec2,
    /// Where it ends.
    pub to: DVec2,
}

impl Segment {
    /// The point of the segment nearest to `p`; its start when it has no length.
    pub fn nearest_point(self, p: DVec2) -> DVec2 {
        let along = self.to - self.from;
        let length_squared = along.length_squared();
        let t = if length_squared > 0.0 {
            ((p - self.from).dot(along) / length_squared).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.from + along * t
    }

    /// The distance from `p` to the nearest point of the segment.
    pub fn distance_to(self, p: DVec2) -> f64 {
        p.distance(self.nearest_point(p))
    }

    /// Whether the two segments share a point.
    pub fn touches(self, other: Segment) -> bool {
        self.crosses(other)
            || other.distance_to(self.from) <= TOUCH
            || other.distance_to(self.to) <= TOUCH
            || self.distance_to(other.from) <= TOUCH
            || self.distance_to(other.to) <= TOUCH
    }

    /// Whether the segments cross at a point inside both: not at an end of either, and not
    /// along a line they share.
    pub fn crosses(self, other: Segment) -> bool {
        let side = |s: Segment, p: DVec2| (s.to - s.from).perp_dot(p - s.from);
        opposite(side(other, self.from), side(other, self.to))
            && opposite(side(self, other.from), side(self, other.to))
    }
}

/// Whether two signed areas lie strictly on opposite sides of zero.
fn opposite(a: f64, b: f64) -> bool {
    (a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0)
}

/// The segments of an open line through `points`, in order: none for fewer than two points.
pub fn open_segments(points: &[DVec2]) -> impl Iterator<Item = Segment> + '_ {
    points.windows(2).filter_map(|pair| match pair {
        [from, to] => Some(Segment {
            from: *from,
            to: *to,
        }),
        _ => None,
    })
}

/// The segments of the closed polygon through `points`, in order, the closing one from the last
/// point back to the first included (a polygon of one point is one segment of no length).
pub fn closed_segments(points: &[DVec2]) -> impl Iterator<Item = Segment> + '_ {
    let closing = match (points.last(), points.first()) {
        (Some(&from), Some(&to)) => Some(Segment { from, to }),
        _ => None,
    };
    open_segments(points).chain(closing)
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
        assert_eq!(s.nearest_point(v(5.0, 3.0)), v(5.0, 0.0));
        assert_eq!(s.distance_to(v(13.0, 4.0)), 5.0, "past the end");
        assert_eq!(
            s.nearest_point(v(-3.0, 4.0)),
            v(0.0, 0.0),
            "before the start"
        );
        let point = Segment {
            from: v(1.0, 1.0),
            to: v(1.0, 1.0),
        };
        assert_eq!(point.distance_to(v(4.0, 5.0)), 5.0);
        assert_eq!(point.nearest_point(v(4.0, 5.0)), v(1.0, 1.0));
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
    fn lines_and_polygons_have_their_segments() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert_eq!(closed_segments(&square).count(), 4);
        assert_eq!(
            closed_segments(&square).last(),
            Some(Segment {
                from: v(0.0, 10.0),
                to: v(0.0, 0.0)
            }),
            "the closing segment comes last"
        );
        assert_eq!(open_segments(&square).count(), 3);
        assert_eq!(closed_segments(&[v(1.0, 1.0)]).count(), 1);
        assert_eq!(open_segments(&[v(1.0, 1.0)]).count(), 0);
        assert_eq!(closed_segments(&[]).count(), 0);
    }
}
