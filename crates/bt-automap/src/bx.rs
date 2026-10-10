//! Inclusive boxes of cells, the geometry of rules: a rule's input box and output extent, and
//! the box of offsets whose Minkowski sum with a region gives where a rule can be tried for it.

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

    /// The smallest box holding both.
    pub(crate) fn union(self, other: Bx) -> Bx {
        Bx {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// The box mirrored through the origin: every `-a` with `a` in `self`. The Minkowski sum of
    /// a region with a rule's input box mirrored is where the rule's box overlaps the region.
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
        Bx {
            min: IVec2::new(x0, y0),
            max: IVec2::new(x1, y1),
        }
    }

    #[test]
    fn boxes_include_both_corners() {
        let b = bx(2, 3, 4, 3);
        assert_eq!(b.size(), IVec2::new(3, 1));
        assert_eq!(
            b.rect(),
            IRect::from_min_size(IVec2::new(2, 3), IVec2::new(3, 1))
        );
        assert_eq!(Bx::from_rect(b.rect()), Some(b));
        assert_eq!(Bx::from_rect(IRect::default()), None);
    }

    #[test]
    fn a_mirrored_rule_box_gives_the_offsets_that_reach_a_cell() {
        // A 3×2 rule (offsets 0..=2, 0..=1) overlaps cell (10, 10) from positions (8..=10,
        // 9..=10): the cell plus every offset of the mirrored box.
        let rule = bx(0, 0, 2, 1);
        assert_eq!(rule.negated(), bx(-2, -1, 0, 0));
        assert_eq!(
            rule.negated().translated(IVec2::new(10, 10)),
            bx(8, 9, 10, 10)
        );
    }

    #[test]
    fn boxes_join_and_grow() {
        let a = bx(0, 0, 4, 4);
        let b = bx(3, 2, 8, 9);
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
