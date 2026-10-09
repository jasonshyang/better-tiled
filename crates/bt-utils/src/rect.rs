//! Rectangles: [`IRect`] for cells and pixels on a grid, [`DRect`] for real-valued areas such as
//! the part of a map a view shows. One convention for both: a rectangle includes its minimum,
//! excludes its maximum, and is empty when a side is not positive.

use glam::{DVec2, IVec2};

/// A rectangle of integer positions: `x`, `y` is its top-left and `w`, `h` its size, so it covers
/// columns `x..x + w` and rows `y..y + h`. It is empty when `w` or `h` is not positive.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct IRect {
    /// The left column.
    pub x: i32,
    /// The top row.
    pub y: i32,
    /// The width.
    pub w: i32,
    /// The height.
    pub h: i32,
}

impl IRect {
    /// The rectangle whose top-left is `min`, of `size`.
    pub const fn from_min_size(min: IVec2, size: IVec2) -> IRect {
        IRect {
            x: min.x,
            y: min.y,
            w: size.x,
            h: size.y,
        }
    }

    /// The rectangle spanning two corners, both included, given in any order.
    pub fn from_corners(a: IVec2, b: IVec2) -> IRect {
        let min = a.min(b);
        let max = a.max(b);
        IRect::from_min_size(min, max - min + IVec2::ONE)
    }

    /// The smallest rectangle holding every position, or `None` when there are none.
    pub fn bounding(positions: impl IntoIterator<Item = IVec2>) -> Option<IRect> {
        let mut positions = positions.into_iter();
        let first = positions.next()?;
        let (min, max) = positions.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
        Some(IRect::from_corners(min, max))
    }

    /// The top-left corner, included.
    pub const fn min(&self) -> IVec2 {
        IVec2::new(self.x, self.y)
    }

    /// The bottom-right corner, excluded.
    pub const fn max(&self) -> IVec2 {
        IVec2::new(self.x + self.w, self.y + self.h)
    }

    /// The width and height.
    pub const fn size(&self) -> IVec2 {
        IVec2::new(self.w, self.h)
    }

    /// Whether the rectangle covers nothing.
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Whether `p` is inside.
    pub const fn contains(&self, p: IVec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.w && p.y < self.y + self.h
    }

    /// The part both rectangles cover; empty when they do not overlap.
    pub fn intersection(&self, other: IRect) -> IRect {
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        IRect::from_min_size(min, (max - min).max(IVec2::ZERO))
    }

    /// The smallest rectangle holding both; an empty rectangle adds nothing.
    pub fn union(&self, other: IRect) -> IRect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        let min = self.min().min(other.min());
        let max = self.max().max(other.max());
        IRect::from_min_size(min, max - min)
    }

    /// The same rectangle moved by `delta`.
    pub fn translated(&self, delta: IVec2) -> IRect {
        IRect::from_min_size(self.min() + delta, self.size())
    }

    /// Every position inside, row by row from the top-left.
    pub fn points(&self) -> impl Iterator<Item = IVec2> + use<> {
        let IRect { x, y, w, h } = *self;
        (0..h.max(0)).flat_map(move |dy| (0..w.max(0)).map(move |dx| IVec2::new(x + dx, y + dy)))
    }
}

/// A rectangle of real coordinates from `min`, included, to `max`, excluded. It is empty when
/// `max` is not greater than `min` on either axis.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct DRect {
    /// The top-left corner.
    pub min: DVec2,
    /// The bottom-right corner.
    pub max: DVec2,
}

impl DRect {
    /// The rectangle whose top-left is `min`, of `size`.
    pub fn from_min_size(min: DVec2, size: DVec2) -> DRect {
        DRect {
            min,
            max: min + size,
        }
    }

    /// The smallest rectangle holding every point, or `None` when there are none.
    pub fn from_points(points: impl IntoIterator<Item = DVec2>) -> Option<DRect> {
        let mut points = points.into_iter();
        let first = points.next()?;
        Some(points.fold(
            DRect {
                min: first,
                max: first,
            },
            |r, p| DRect {
                min: r.min.min(p),
                max: r.max.max(p),
            },
        ))
    }

    /// The width and height.
    pub fn size(&self) -> DVec2 {
        self.max - self.min
    }

    /// Whether the rectangle covers no area (a rectangle with a non-finite side covers none).
    pub fn is_empty(&self) -> bool {
        !(self.max.x > self.min.x && self.max.y > self.min.y)
    }

    /// Whether `p` is inside.
    pub fn contains(&self, p: DVec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x < self.max.x && p.y < self.max.y
    }

    /// Whether the two rectangles share an area.
    pub fn intersects(&self, other: DRect) -> bool {
        !self.intersection(other).is_empty()
    }

    /// The part both rectangles cover (empty when they do not overlap).
    pub fn intersection(&self, other: DRect) -> DRect {
        DRect {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }

    /// The smallest rectangle holding both; an empty rectangle adds nothing.
    pub fn union(&self, other: DRect) -> DRect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return *self;
        }
        DRect {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The rectangle grown by `margin` on every side.
    pub fn expanded(&self, margin: DVec2) -> DRect {
        DRect {
            min: self.min - margin,
            max: self.max + margin,
        }
    }

    /// The same rectangle moved by `delta`.
    pub fn translated(&self, delta: DVec2) -> DRect {
        DRect {
            min: self.min + delta,
            max: self.max + delta,
        }
    }

    /// The smallest integer rectangle holding this one (Qt's `QRectF::toAlignedRect`), or `None`
    /// when a coordinate is not finite or does not fit an `i32`.
    pub fn to_aligned(&self) -> Option<IRect> {
        let min = DVec2::new(self.min.x.floor(), self.min.y.floor());
        let max = DVec2::new(self.max.x.ceil(), self.max.y.ceil());
        let fits = |v: f64| v.is_finite() && v >= f64::from(i32::MIN) && v <= f64::from(i32::MAX);
        if ![min.x, min.y, max.x, max.y].into_iter().all(fits) {
            return None;
        }
        let size = max - min;
        if !fits(size.x) || !fits(size.y) {
            return None;
        }
        // The values were checked to fit, so the casts are exact.
        Some(IRect::from_min_size(min.as_ivec2(), size.as_ivec2()))
    }
}

impl From<IRect> for DRect {
    fn from(r: IRect) -> DRect {
        DRect {
            min: r.min().as_dvec2(),
            max: r.max().as_dvec2(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn integer_rectangles_include_their_minimum_and_exclude_their_maximum() {
        let r = rect(2, 3, 4, 2);
        assert_eq!(
            (r.min(), r.max(), r.size()),
            (IVec2::new(2, 3), IVec2::new(6, 5), IVec2::new(4, 2))
        );
        assert!(r.contains(IVec2::new(2, 3)) && r.contains(IVec2::new(5, 4)));
        assert!(!r.contains(IVec2::new(6, 4)) && !r.contains(IVec2::new(5, 5)));
        assert!(rect(0, 0, 0, 5).is_empty() && rect(0, 0, 5, -1).is_empty() && !r.is_empty());
        let points: Vec<IVec2> = rect(-1, 0, 2, 2).points().collect();
        assert_eq!(
            points,
            [
                IVec2::new(-1, 0),
                IVec2::new(0, 0),
                IVec2::new(-1, 1),
                IVec2::new(0, 1)
            ]
        );
        assert_eq!(rect(0, 0, -3, 2).points().count(), 0);
    }

    #[test]
    fn corners_bounds_intersections_and_unions() {
        assert_eq!(
            IRect::from_corners(IVec2::new(4, 1), IVec2::new(1, 3)),
            rect(1, 1, 4, 3)
        );
        let points = [IVec2::new(3, -2), IVec2::new(-1, 4), IVec2::new(0, 0)];
        assert_eq!(IRect::bounding(points), Some(rect(-1, -2, 5, 7)));
        assert_eq!(IRect::bounding([]), None);
        assert_eq!(
            rect(0, 0, 4, 4).intersection(rect(2, 1, 5, 2)),
            rect(2, 1, 2, 2)
        );
        assert!(rect(0, 0, 2, 2).intersection(rect(5, 5, 1, 1)).is_empty());
        assert_eq!(rect(0, 0, 2, 2).union(rect(5, 5, 1, 1)), rect(0, 0, 6, 6));
        assert_eq!(rect(0, 0, 0, 0).union(rect(5, 5, 1, 1)), rect(5, 5, 1, 1));
        assert_eq!(
            rect(1, 1, 2, 2).translated(IVec2::new(-3, 4)),
            rect(-2, 5, 2, 2)
        );
    }

    #[test]
    fn real_rectangles() {
        let r = DRect::from_min_size(DVec2::new(0.5, -1.0), DVec2::new(2.0, 1.5));
        assert_eq!(r.max, DVec2::new(2.5, 0.5));
        assert!(r.contains(DVec2::new(0.5, -1.0)) && !r.contains(DVec2::new(2.5, 0.0)));
        assert_eq!(r.to_aligned(), Some(rect(0, -1, 3, 2)));
        let points = [DVec2::new(3.0, 1.0), DVec2::new(-1.0, 2.0)];
        assert_eq!(
            DRect::from_points(points),
            Some(DRect {
                min: DVec2::new(-1.0, 1.0),
                max: DVec2::new(3.0, 2.0)
            })
        );
        assert_eq!(DRect::from_points([]), None);
        let a = DRect::from_min_size(DVec2::ZERO, DVec2::splat(2.0));
        assert!(a.intersects(DRect::from_min_size(DVec2::splat(1.0), DVec2::splat(2.0))));
        assert!(
            !a.intersects(DRect::from_min_size(
                DVec2::new(2.0, 0.0),
                DVec2::splat(2.0)
            )),
            "touching shares no area"
        );
        assert_eq!(a.expanded(DVec2::ONE).min, DVec2::splat(-1.0));
        assert_eq!(a.translated(DVec2::new(1.0, 0.0)).max, DVec2::new(3.0, 2.0));
        assert_eq!(DRect::from(rect(1, 2, 3, 4)).max, DVec2::new(4.0, 6.0));
        assert!(
            DRect {
                min: DVec2::ZERO,
                max: DVec2::new(f64::NAN, 1.0)
            }
            .is_empty()
        );
        assert_eq!(
            DRect {
                min: DVec2::ZERO,
                max: DVec2::new(1e20, 1.0)
            }
            .to_aligned(),
            None
        );
    }
}
