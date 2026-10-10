//! Whether a point lies inside a polygon. Polygons are filled by the even-odd rule, as Qt fills
//! the paths Tiled draws and picks objects with, so a figure drawn in one stroke that crosses
//! itself leaves its doubly covered parts out.

use glam::DVec2;

use crate::segment::{TOUCH, closed_segments};

/// Whether `p` is inside the closed polygon through `points`, by the even-odd rule. A point on
/// the boundary may count either way; [`polygon_holds_or_bounds`] counts it in.
pub fn polygon_holds(points: &[DVec2], p: DVec2) -> bool {
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
pub fn polygon_holds_or_bounds(points: &[DVec2], p: DVec2) -> bool {
    polygon_holds(points, p) || closed_segments(points).any(|edge| edge.distance_to(p) <= TOUCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn polygons_hold_by_the_even_odd_rule() {
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        assert!(polygon_holds(&square, v(5.0, 5.0)));
        assert!(!polygon_holds(&square, v(15.0, 5.0)));
        assert!(polygon_holds_or_bounds(&square, v(10.0, 5.0)));
        assert!(!polygon_holds_or_bounds(&square, v(10.1, 5.0)));
        // A star drawn in one stroke: its middle is covered twice and is out.
        let star = [
            v(0.0, 3.0),
            v(10.0, 3.0),
            v(2.0, 9.0),
            v(5.0, 0.0),
            v(8.0, 9.0),
        ];
        assert!(!polygon_holds(&star, v(5.0, 5.0)));
        assert!(polygon_holds(&star, v(5.0, 1.5)));
        assert!(!polygon_holds(&[], v(0.0, 0.0)), "no points hold nothing");
    }
}
