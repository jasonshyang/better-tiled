//! The two directions of a map: the way a flip mirrors (Tiled's `FlipDirection`,
//! `reference/tiled/src/libtiled/tiled.h`), for objects, brushes, clips and carried tiles alike,
//! and the way objects are spaced out.

use glam::DVec2;

/// A direction on the map: across or down. Flipping along an axis mirrors that coordinate;
/// distributing along one spaces things out in that direction.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Across: a horizontal flip mirrors left and right.
    Horizontal,
    /// Down: a vertical flip mirrors top and bottom.
    Vertical,
}

impl Axis {
    /// The coordinate of `v` along the axis.
    pub(crate) fn along(self, v: DVec2) -> f64 {
        match self {
            Axis::Horizontal => v.x,
            Axis::Vertical => v.y,
        }
    }

    /// A vector of `length` along the axis.
    pub(crate) fn vector(self, length: f64) -> DVec2 {
        match self {
            Axis::Horizontal => DVec2::new(length, 0.0),
            Axis::Vertical => DVec2::new(0.0, length),
        }
    }

    /// `p` mirrored across the line through `origin` square to the axis.
    pub(crate) fn mirrored(self, p: DVec2, origin: DVec2) -> DVec2 {
        match self {
            Axis::Horizontal => DVec2::new(2.0 * origin.x - p.x, p.y),
            Axis::Vertical => DVec2::new(p.x, 2.0 * origin.y - p.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_pick_and_mirror_their_coordinate() {
        let p = DVec2::new(3.0, 5.0);
        assert_eq!(
            (Axis::Horizontal.along(p), Axis::Vertical.along(p)),
            (3.0, 5.0)
        );
        assert_eq!(Axis::Vertical.vector(2.0), DVec2::new(0.0, 2.0));
        let origin = DVec2::new(10.0, 10.0);
        assert_eq!(Axis::Horizontal.mirrored(p, origin), DVec2::new(17.0, 5.0));
        assert_eq!(Axis::Vertical.mirrored(p, origin), DVec2::new(3.0, 15.0));
    }
}
