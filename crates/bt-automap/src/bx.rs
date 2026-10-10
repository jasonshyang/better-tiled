//! Inclusive boxes of cells, the geometry of rules: a rule's input and output extents, the
//! offsets it reads and writes, and their Minkowski sums, which say where a rule can be tried
//! for a region and how far its effects reach.

use bt_utils::IRect;
use glam::IVec2;

/// A box of cells with both corners included, Qt's `QRect` convention (`left..=right`). It is
/// never empty: an empty set of cells has no box (`Option<Bx>`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Bx {
    /// The top-left cell.
    pub(crate) min: IVec2,
    /// The bottom-right cell, included.
    pub(crate) max: IVec2,
}

impl Bx {
    /// The box of one cell.
    pub(crate) fn point(p: IVec2) -> Bx {
        Bx { min: p, max: p }
    }

    /// The box spanning two corners, both included, given in any order.
    pub(crate) fn spanning(a: IVec2, b: IVec2) -> Bx {
        Bx {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// The smallest box holding every cell, or `None` when there are none.
    pub(crate) fn bounding(cells: impl IntoIterator<Item = IVec2>) -> Option<Bx> {
        cells.into_iter().map(Bx::point).reduce(|a, b| a.union(b))
    }

    /// The box of a rectangle's cells, or `None` for an empty rectangle.
    pub(crate) fn from_rect(rect: IRect) -> Option<Bx> {
        (!rect.is_empty()).then(|| Bx {
            min: rect.min(),
            max: rect.max() - IVec2::ONE,
        })
    }

    /// The same cells as a half-open rectangle.
    pub(crate) fn rect(self) -> IRect {
        IRect::from_min_size(self.min, self.size())
    }

    /// The width and height in cells.
    pub(crate) fn size(self) -> IVec2 {
        self.max - self.min + IVec2::ONE
    }

    /// Whether `p` is inside.
    pub(crate) fn contains(self, p: IVec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    /// The smallest box holding both.
    pub(crate) fn union(self, other: Bx) -> Bx {
        Bx {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The cells both hold, or `None` when they do not overlap.
    pub(crate) fn intersection(self, other: Bx) -> Option<Bx> {
        let min = self.min.max(other.min);
        let max = self.max.min(other.max);
        min.cmple(max).all().then_some(Bx { min, max })
    }

    /// The Minkowski sum: every `a + b` with `a` in `self` and `b` in `other`.
    pub(crate) fn minkowski_sum(self, other: Bx) -> Bx {
        Bx {
            min: self.min + other.min,
            max: self.max + other.max,
        }
    }

    /// The box mirrored through the origin: every `-a` with `a` in `self`.
    pub(crate) fn negated(self) -> Bx {
        Bx {
            min: -self.max,
            max: -self.min,
        }
    }

    /// The box moved by `delta`.
    pub(crate) fn translated(self, delta: IVec2) -> Bx {
        Bx {
            min: self.min + delta,
            max: self.max + delta,
        }
    }

    /// The box grown by `margin` cells on every side.
    pub(crate) fn grown(self, margin: IVec2) -> Bx {
        Bx {
            min: self.min - margin,
            max: self.max + margin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(x0: i32, y0: i32, x1: i32, y1: i32) -> Bx {
        Bx::spanning(IVec2::new(x0, y0), IVec2::new(x1, y1))
    }

    #[test]
    fn boxes_include_both_corners() {
        let b = bx(2, 3, 4, 3);
        assert_eq!(b.size(), IVec2::new(3, 1));
        assert!(b.contains(IVec2::new(4, 3)) && !b.contains(IVec2::new(5, 3)));
        assert_eq!(
            b.rect(),
            IRect::from_min_size(IVec2::new(2, 3), IVec2::new(3, 1))
        );
        assert_eq!(Bx::from_rect(b.rect()), Some(b));
        assert_eq!(Bx::from_rect(IRect::default()), None);
    }

    #[test]
    fn minkowski_sums_add_the_corners() {
        // Where a 3×2 rule (offsets 0..=2, 0..=1) can be tried so that its box overlaps cell
        // (10, 10): the cell minus every offset.
        let rule = bx(0, 0, 2, 1);
        let at = Bx::point(IVec2::new(10, 10));
        assert_eq!(at.minkowski_sum(rule.negated()), bx(8, 9, 10, 10));
        assert_eq!(rule.negated(), bx(-2, -1, 0, 0));
    }

    #[test]
    fn boxes_meet_and_join() {
        let a = bx(0, 0, 4, 4);
        let b = bx(3, 2, 8, 9);
        assert_eq!(a.intersection(b), Some(bx(3, 2, 4, 4)));
        assert_eq!(a.intersection(bx(5, 0, 6, 1)), None);
        assert_eq!(a.union(b), bx(0, 0, 8, 9));
        assert_eq!(
            Bx::bounding([IVec2::new(3, -1), IVec2::new(-2, 4)]),
            Some(bx(-2, -1, 3, 4))
        );
        assert_eq!(Bx::bounding([]), None);
        assert_eq!(a.grown(IVec2::ONE), bx(-1, -1, 5, 5));
        assert_eq!(a.translated(IVec2::new(1, 2)), bx(1, 2, 5, 6));
    }
}
