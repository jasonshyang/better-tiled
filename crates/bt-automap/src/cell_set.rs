//! Sets of cells kept as runs per row, and visited the way Tiled visits a `QRegion`: as `QRegion`'s
//! y-x bands, rectangle by rectangle, each row by row. The order matters where Tiled's results
//! depend on it: rules matched in order, `NoOverlappingOutput`, and later writes winning.

use std::collections::BTreeMap;

use bt_utils::IRect;
use bt_utils::bands::banded_runs;
use glam::IVec2;

use crate::bx::Bx;

/// A run of cells in one row, both ends included.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Span {
    start: i32,
    end: i32,
}

/// A set of cells: per row, sorted runs that neither overlap nor touch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CellSet {
    rows: BTreeMap<i32, Vec<Span>>,
}

impl CellSet {
    /// No cells.
    pub(crate) fn new() -> CellSet {
        CellSet::default()
    }

    /// The cells of a rectangle.
    pub(crate) fn from_rect(rect: IRect) -> CellSet {
        let mut set = CellSet::new();
        set.insert_rect(rect);
        set
    }

    /// The given cells.
    pub(crate) fn from_cells(cells: impl IntoIterator<Item = IVec2>) -> CellSet {
        let mut set = CellSet::new();
        for p in cells {
            set.insert_span(
                p.y,
                Span {
                    start: p.x,
                    end: p.x,
                },
            );
        }
        set
    }

    /// Whether there are no cells.
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Adds the cells of a rectangle.
    pub(crate) fn insert_rect(&mut self, rect: IRect) {
        let Some(b) = Bx::from_rect(rect) else {
            return;
        };
        for y in b.min.y..=b.max.y {
            self.insert_span(
                y,
                Span {
                    start: b.min.x,
                    end: b.max.x,
                },
            );
        }
    }

    /// Adds the cells of another set.
    pub(crate) fn insert_set(&mut self, other: &CellSet) {
        for (&y, spans) in &other.rows {
            for &span in spans {
                self.insert_span(y, span);
            }
        }
    }

    fn insert_span(&mut self, y: i32, span: Span) {
        let row = self.rows.entry(y).or_default();
        // The first run that ends at or after the cell before the new one could touch it.
        let first = row.partition_point(|s| s.end < span.start.saturating_sub(1));
        let mut merged = span;
        let mut last = first;
        while let Some(s) = row.get(last) {
            if s.start > merged.end.saturating_add(1) {
                break;
            }
            merged.start = merged.start.min(s.start);
            merged.end = merged.end.max(s.end);
            last += 1;
        }
        row.splice(first..last, [merged]);
    }

    /// Whether `p` is in the set.
    pub(crate) fn contains(&self, p: IVec2) -> bool {
        self.rows.get(&p.y).is_some_and(|row| {
            let i = row.partition_point(|s| s.end < p.x);
            row.get(i).is_some_and(|s| s.start <= p.x)
        })
    }

    /// The cells outside `rect`.
    pub(crate) fn difference_with_rect(&self, rect: IRect) -> CellSet {
        let Some(b) = Bx::from_rect(rect) else {
            return self.clone();
        };
        let mut result = CellSet::new();
        for (&y, row) in &self.rows {
            let mut spans = Vec::new();
            for &s in row {
                if y < b.min.y || y > b.max.y || s.end < b.min.x || s.start > b.max.x {
                    spans.push(s);
                    continue;
                }
                if s.start < b.min.x {
                    spans.push(Span {
                        start: s.start,
                        end: b.min.x - 1,
                    });
                }
                if s.end > b.max.x {
                    spans.push(Span {
                        start: b.max.x + 1,
                        end: s.end,
                    });
                }
            }
            if !spans.is_empty() {
                result.rows.insert(y, spans);
            }
        }
        result
    }

    /// The cells also inside `rect`.
    pub(crate) fn intersection_with_rect(&self, rect: IRect) -> CellSet {
        let Some(b) = Bx::from_rect(rect) else {
            return CellSet::new();
        };
        let mut result = CellSet::new();
        for (&y, row) in self.rows.range(b.min.y..=b.max.y) {
            let spans: Vec<Span> = row
                .iter()
                .filter_map(|s| {
                    let start = s.start.max(b.min.x);
                    let end = s.end.min(b.max.x);
                    (start <= end).then_some(Span { start, end })
                })
                .collect();
            if !spans.is_empty() {
                result.rows.insert(y, spans);
            }
        }
        result
    }

    /// Whether any cell of `other` is in the set.
    pub(crate) fn intersects(&self, other: &CellSet) -> bool {
        other.rows.iter().any(|(y, spans)| {
            self.rows.get(y).is_some_and(|row| {
                spans.iter().any(|s| {
                    let i = row.partition_point(|r| r.end < s.start);
                    row.get(i).is_some_and(|r| r.start <= s.end)
                })
            })
        })
    }

    /// The smallest box holding every cell, or `None` when there is none.
    pub(crate) fn bounding(&self) -> Option<Bx> {
        let (&top, _) = self.rows.first_key_value()?;
        let (&bottom, _) = self.rows.last_key_value()?;
        let left = self
            .rows
            .values()
            .filter_map(|r| r.first())
            .map(|s| s.start)
            .min()?;
        let right = self
            .rows
            .values()
            .filter_map(|r| r.last())
            .map(|s| s.end)
            .max()?;
        Some(Bx {
            min: IVec2::new(left, top),
            max: IVec2::new(right, bottom),
        })
    }

    /// The set moved by `delta`.
    pub(crate) fn translated(&self, delta: IVec2) -> CellSet {
        CellSet {
            rows: self
                .rows
                .iter()
                .map(|(&y, row)| {
                    let row = row
                        .iter()
                        .map(|s| Span {
                            start: s.start + delta.x,
                            end: s.end + delta.x,
                        })
                        .collect();
                    (y + delta.y, row)
                })
                .collect(),
        }
    }

    /// The Minkowski sum with a box: every `p + d` with `p` in the set and `d` in `offsets`.
    pub(crate) fn dilated(&self, offsets: Bx) -> CellSet {
        let mut result = CellSet::new();
        for (&y, row) in &self.rows {
            for dy in offsets.min.y..=offsets.max.y {
                for s in row {
                    result.insert_span(
                        y + dy,
                        Span {
                            start: s.start + offsets.min.x,
                            end: s.end + offsets.max.x,
                        },
                    );
                }
            }
        }
        result
    }

    /// The cells, row by row from the top, each row from the left.
    pub(crate) fn cells(&self) -> impl Iterator<Item = IVec2> + '_ {
        self.rows.iter().flat_map(|(&y, row)| {
            row.iter()
                .flat_map(move |s| (s.start..=s.end).map(move |x| IVec2::new(x, y)))
        })
    }

    /// How many cells there are.
    pub(crate) fn len(&self) -> usize {
        self.rows
            .values()
            .flatten()
            .map(|s| usize::try_from(i64::from(s.end) - i64::from(s.start) + 1).unwrap_or(0))
            .sum()
    }

    /// The set as `QRegion`'s y-x bands: the order Tiled visits a region in.
    pub(crate) fn rects(&self) -> Vec<IRect> {
        banded_runs(self.rows.iter().flat_map(|(&y, row)| {
            row.iter().map(move |s| IRect {
                x: s.start,
                y,
                w: s.end - s.start + 1,
                h: 1,
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn runs_merge_when_they_touch() {
        let mut set = CellSet::from_cells([IVec2::new(0, 0), IVec2::new(2, 0)]);
        assert_eq!(set.len(), 2);
        set.insert_rect(rect(1, 0, 1, 1));
        assert_eq!(set.rects(), [rect(0, 0, 3, 1)]);
        set.insert_rect(rect(-5, 0, 2, 1));
        assert_eq!(set.rects(), [rect(-5, 0, 2, 1), rect(0, 0, 3, 1)]);
        set.insert_rect(rect(-3, 0, 3, 1));
        assert_eq!(set.rects(), [rect(-5, 0, 8, 1)]);
        assert!(set.contains(IVec2::new(2, 0)) && !set.contains(IVec2::new(3, 0)));
    }

    #[test]
    fn rects_come_in_band_order() {
        // Two columns of different heights: QRegion splits them into bands where the rows
        // differ, and visits each band's rectangles left to right.
        let mut set = CellSet::from_rect(rect(0, 0, 2, 3));
        set.insert_rect(rect(5, 1, 2, 2));
        assert_eq!(
            set.rects(),
            [rect(0, 0, 2, 1), rect(0, 1, 2, 2), rect(5, 1, 2, 2)]
        );
        assert_eq!(set.len(), 10);
        assert_eq!(
            set.bounding(),
            Some(Bx {
                min: IVec2::new(0, 0),
                max: IVec2::new(6, 2)
            })
        );
    }

    #[test]
    fn dilation_is_the_minkowski_sum() {
        let set = CellSet::from_cells([IVec2::new(5, 5)]);
        let grown = set.dilated(Bx {
            min: IVec2::new(-2, -1),
            max: IVec2::ZERO,
        });
        assert_eq!(grown.rects(), [rect(3, 4, 3, 2)]);
        let moved = grown.translated(IVec2::new(1, 1));
        assert_eq!(moved.rects(), [rect(4, 5, 3, 2)]);
        assert!(grown.intersects(&moved));
        let mut both = grown.clone();
        both.insert_set(&moved);
        assert_eq!(both.len(), 10);
        assert!(!grown.intersects(&CellSet::from_rect(rect(0, 0, 3, 9))));
    }

    #[test]
    fn intersections_clip_to_the_rectangle() {
        let mut set = CellSet::from_rect(rect(0, 0, 10, 2));
        set.insert_rect(rect(20, 0, 2, 2));
        let clipped = set.intersection_with_rect(rect(5, 1, 20, 5));
        assert_eq!(clipped.rects(), [rect(5, 1, 5, 1), rect(20, 1, 2, 1)]);
        let outside = set.difference_with_rect(rect(2, 1, 3, 9));
        assert_eq!(
            outside.rects(),
            [
                rect(0, 0, 10, 1),
                rect(20, 0, 2, 1),
                rect(0, 1, 2, 1),
                rect(5, 1, 5, 1),
                rect(20, 1, 2, 1)
            ]
        );
        assert!(set.intersection_with_rect(rect(0, 5, 3, 3)).is_empty());
        let cells: Vec<IVec2> = clipped.cells().take(2).collect();
        assert_eq!(cells, [IVec2::new(5, 1), IVec2::new(6, 1)]);
    }
}
