//! Sets of cells: what a tile selection, a fill area or a brush footprint covers.

use std::collections::BTreeMap;

use bt_utils::IRect;
use bt_utils::bands::banded_runs;
use glam::IVec2;

/// A set of cells, kept as sorted horizontal runs per row. Two regions are equal when they hold
/// the same cells.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TileRegion {
    /// The rows that hold cells. A row's spans are sorted and at least one cell apart, so each
    /// set of cells has one form.
    rows: BTreeMap<i32, Vec<Span>>,
}

/// Columns `start..end` of a row.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Span {
    start: i32,
    end: i32,
}

impl TileRegion {
    /// The empty region.
    pub fn new() -> TileRegion {
        TileRegion::default()
    }

    /// The cells of a rectangle.
    pub fn from_rect(rect: IRect) -> TileRegion {
        let mut region = TileRegion::new();
        region.insert_rect(rect);
        region
    }

    /// The cells of several rectangles.
    pub fn from_rects(rects: impl IntoIterator<Item = IRect>) -> TileRegion {
        let mut region = TileRegion::new();
        for rect in rects {
            region.insert_rect(rect);
        }
        region
    }

    /// The cells given, each once.
    pub fn from_cells(cells: impl IntoIterator<Item = IVec2>) -> TileRegion {
        let mut cells: Vec<IVec2> = cells.into_iter().collect();
        cells.sort_unstable_by_key(|p| (p.y, p.x));
        cells.dedup();
        let mut rows: BTreeMap<i32, Vec<Span>> = BTreeMap::new();
        for p in cells {
            let spans = rows.entry(p.y).or_default();
            match spans.last_mut() {
                Some(span) if span.end == p.x => span.end += 1,
                _ => spans.push(Span {
                    start: p.x,
                    end: p.x + 1,
                }),
            }
        }
        TileRegion { rows }
    }

    /// Whether the region holds no cell.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many cells it holds.
    pub fn len(&self) -> usize {
        self.rows
            .values()
            .flatten()
            .map(|span| usize::try_from(span.end - span.start).unwrap_or(0))
            .sum()
    }

    /// Whether it holds `p`.
    pub fn contains(&self, p: IVec2) -> bool {
        self.rows.get(&p.y).is_some_and(|spans| {
            let index = spans.partition_point(|span| span.end <= p.x);
            spans.get(index).is_some_and(|span| span.start <= p.x)
        })
    }

    /// Adds a cell; whether it was not there before.
    pub fn insert(&mut self, p: IVec2) -> bool {
        if self.contains(p) {
            return false;
        }
        self.insert_rect(IRect::from_min_size(p, IVec2::ONE));
        true
    }

    /// Takes a cell out; whether it was there.
    pub fn remove(&mut self, p: IVec2) -> bool {
        if !self.contains(p) {
            return false;
        }
        *self = self.subtract(&TileRegion::from_rect(IRect::from_min_size(p, IVec2::ONE)));
        true
    }

    /// Adds the cells of a rectangle.
    pub fn insert_rect(&mut self, rect: IRect) {
        if rect.is_empty() {
            return;
        }
        let span = [Span {
            start: rect.x,
            end: rect.x + rect.w,
        }];
        for y in rect.y..rect.y + rect.h {
            let spans = self.rows.entry(y).or_default();
            *spans = union_spans(spans, &span);
        }
    }

    /// The cells in either region.
    pub fn union(&self, other: &TileRegion) -> TileRegion {
        let mut rows = self.rows.clone();
        for (y, spans) in &other.rows {
            let row = rows.entry(*y).or_default();
            *row = union_spans(row, spans);
        }
        TileRegion { rows }
    }

    /// The cells of this region that are not in `other`.
    pub fn subtract(&self, other: &TileRegion) -> TileRegion {
        let rows = self
            .rows
            .iter()
            .filter_map(|(y, spans)| {
                let left = match other.rows.get(y) {
                    Some(cut) => subtract_spans(spans, cut),
                    None => spans.clone(),
                };
                (!left.is_empty()).then_some((*y, left))
            })
            .collect();
        TileRegion { rows }
    }

    /// The cells in both regions.
    pub fn intersect(&self, other: &TileRegion) -> TileRegion {
        let rows = self
            .rows
            .iter()
            .filter_map(|(y, spans)| {
                let both = intersect_spans(spans, other.rows.get(y)?);
                (!both.is_empty()).then_some((*y, both))
            })
            .collect();
        TileRegion { rows }
    }

    /// The cells in exactly one of the regions.
    pub fn xor(&self, other: &TileRegion) -> TileRegion {
        self.subtract(other).union(&other.subtract(self))
    }

    /// The cells inside `rect`.
    pub fn clipped(&self, rect: IRect) -> TileRegion {
        self.intersect(&TileRegion::from_rect(rect))
    }

    /// The region moved by `delta`.
    pub fn translated(&self, delta: IVec2) -> TileRegion {
        let rows = self
            .rows
            .iter()
            .map(|(y, spans)| {
                let moved = spans
                    .iter()
                    .map(|span| Span {
                        start: span.start + delta.x,
                        end: span.end + delta.x,
                    })
                    .collect();
                (y + delta.y, moved)
            })
            .collect();
        TileRegion { rows }
    }

    /// The smallest rectangle holding every cell, or `None` when there is none.
    pub fn bounds(&self) -> Option<IRect> {
        let (&top, _) = self.rows.first_key_value()?;
        let (&bottom, _) = self.rows.last_key_value()?;
        let left = self
            .rows
            .values()
            .filter_map(|s| s.first())
            .map(|s| s.start)
            .min()?;
        let right = self
            .rows
            .values()
            .filter_map(|s| s.last())
            .map(|s| s.end)
            .max()?;
        Some(IRect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top + 1,
        })
    }

    /// The cells, row by row from the top, left to right.
    pub fn iter(&self) -> impl Iterator<Item = IVec2> + '_ {
        self.rows.iter().flat_map(|(&y, spans)| {
            spans
                .iter()
                .flat_map(move |span| (span.start..span.end).map(move |x| IVec2::new(x, y)))
        })
    }

    /// The maximal runs of each row as one-row rectangles, row by row from the top.
    pub fn runs(&self) -> impl Iterator<Item = IRect> + '_ {
        self.rows.iter().flat_map(|(&y, spans)| {
            spans.iter().map(move |span| IRect {
                x: span.start,
                y,
                w: span.end - span.start,
                h: 1,
            })
        })
    }

    /// The region as Qt's `QRegion` stores it: rows with the same runs merged into bands, band
    /// by band from the top. The order Tiled visits a region's rectangles in.
    pub fn rects(&self) -> Vec<IRect> {
        banded_runs(self.runs())
    }

    /// Whether the regions share a cell.
    pub fn intersects(&self, other: &TileRegion) -> bool {
        self.rows.iter().any(|(y, spans)| {
            other
                .rows
                .get(y)
                .is_some_and(|theirs| !intersect_spans(spans, theirs).is_empty())
        })
    }

    /// Whether every cell of this region is in `other`.
    pub fn is_subset(&self, other: &TileRegion) -> bool {
        self.subtract(other).is_empty()
    }
}

impl FromIterator<IVec2> for TileRegion {
    fn from_iter<T: IntoIterator<Item = IVec2>>(cells: T) -> TileRegion {
        TileRegion::from_cells(cells)
    }
}

/// How a new selection combines with the one there is. The window decides which keys mean
/// which mode; operations only know modes.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SelectionMode {
    /// The new selection replaces it.
    #[default]
    Replace,
    /// The new cells are added.
    Add,
    /// The new cells are taken out.
    Subtract,
    /// Only the cells in both are kept.
    Intersect,
}

impl SelectionMode {
    /// The selection after combining `new` with `current`.
    pub fn apply(self, current: &TileRegion, new: &TileRegion) -> TileRegion {
        match self {
            SelectionMode::Replace => new.clone(),
            SelectionMode::Add => current.union(new),
            SelectionMode::Subtract => current.subtract(new),
            SelectionMode::Intersect => current.intersect(new),
        }
    }
}

/// The spans covering either list's cells, merged where they overlap or touch.
fn union_spans(a: &[Span], b: &[Span]) -> Vec<Span> {
    let mut merged: Vec<Span> = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    loop {
        let next = match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x.start <= y.start => {
                i += 1;
                *x
            }
            (_, Some(y)) => {
                j += 1;
                *y
            }
            (Some(x), None) => {
                i += 1;
                *x
            }
            (None, None) => break,
        };
        match merged.last_mut() {
            Some(last) if next.start <= last.end => last.end = last.end.max(next.end),
            _ => merged.push(next),
        }
    }
    merged
}

/// The parts of `a`'s spans that no span of `cut` covers.
fn subtract_spans(a: &[Span], cut: &[Span]) -> Vec<Span> {
    let mut left = Vec::new();
    // Cuts that end before a span starts end before every later span starts too.
    let mut first_cut = 0;
    for span in a {
        while cut.get(first_cut).is_some_and(|c| c.end <= span.start) {
            first_cut += 1;
        }
        let mut start = span.start;
        for c in cut.get(first_cut..).unwrap_or_default() {
            if c.start >= span.end {
                break;
            }
            if c.start > start {
                left.push(Span {
                    start,
                    end: c.start,
                });
            }
            start = start.max(c.end);
        }
        if start < span.end {
            left.push(Span {
                start,
                end: span.end,
            });
        }
    }
    left
}

/// The parts covered by a span of each list.
fn intersect_spans(a: &[Span], b: &[Span]) -> Vec<Span> {
    let mut both = Vec::new();
    let (mut i, mut j) = (0, 0);
    while let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
        let start = x.start.max(y.start);
        let end = x.end.min(y.end);
        if start < end {
            both.push(Span { start, end });
        }
        if x.end <= y.end {
            i += 1;
        } else {
            j += 1;
        }
    }
    both
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use bt_test_data::Fuzz;
    use bt_utils::bands::banded_rects;
    use bt_utils::rng::Rng;

    use super::*;
    use crate::testutil::drawn;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn rectangles_and_cells_make_the_same_region() {
        let from_rect = TileRegion::from_rect(rect(2, -1, 3, 2));
        let mut cells: Vec<IVec2> = rect(2, -1, 3, 2).points().collect();
        cells.reverse();
        let from_cells: TileRegion = cells.into_iter().collect();
        assert_eq!(from_rect, from_cells);
        assert_eq!(from_rect.len(), 6);
        assert_eq!(from_rect.bounds(), Some(rect(2, -1, 3, 2)));
        assert!(TileRegion::from_rect(rect(0, 0, 0, 5)).is_empty());
        assert_eq!(TileRegion::new().bounds(), None);
    }

    #[test]
    fn touching_runs_merge_so_equal_sets_are_equal() {
        let mut region = TileRegion::new();
        assert!(region.insert(IVec2::new(0, 0)));
        assert!(region.insert(IVec2::new(2, 0)));
        assert!(region.insert(IVec2::new(1, 0)));
        assert!(!region.insert(IVec2::new(1, 0)));
        assert_eq!(region, TileRegion::from_rect(rect(0, 0, 3, 1)));
        assert_eq!(region.runs().collect::<Vec<_>>(), [rect(0, 0, 3, 1)]);
        assert!(region.remove(IVec2::new(1, 0)));
        assert!(!region.remove(IVec2::new(1, 0)));
        assert_eq!(
            region.runs().collect::<Vec<_>>(),
            [rect(0, 0, 1, 1), rect(2, 0, 1, 1)]
        );
        region.remove(IVec2::new(0, 0));
        region.remove(IVec2::new(2, 0));
        assert_eq!(region, TileRegion::new(), "an emptied row is gone");
    }

    #[test]
    fn set_operations_on_drawn_shapes() {
        let a = drawn(
            "
            ###.
            ###.
            ",
        );
        let b = drawn(
            "
            .###
            ....
            .###
            ",
        );
        assert_eq!(
            a.union(&b),
            drawn(
                "
                ####
                ###.
                .###
                "
            )
        );
        assert_eq!(
            a.subtract(&b),
            drawn(
                "
                #...
                ###.
                "
            )
        );
        assert_eq!(a.intersect(&b), drawn(".##."));
        assert_eq!(
            a.xor(&b),
            drawn(
                "
                #..#
                ###.
                .###
                "
            )
        );
        assert!(a.intersects(&b) && !a.is_subset(&b));
        assert!(drawn(".##.").is_subset(&a));
        assert_eq!(a.clipped(rect(1, 1, 5, 5)), drawn("....\n.##."));
        assert_eq!(
            a.translated(IVec2::new(-1, 3)).bounds(),
            Some(rect(-1, 3, 3, 2))
        );
    }

    #[test]
    fn rects_are_qregions_bands() {
        let region = drawn(
            "
            ##.##
            ##.##
            #####
            ",
        );
        assert_eq!(
            region.rects(),
            [rect(0, 0, 2, 2), rect(3, 0, 2, 2), rect(0, 2, 5, 1)]
        );
    }

    #[test]
    fn selection_modes_combine_as_named() {
        let current = drawn("##..");
        let new = drawn(".##.");
        assert_eq!(SelectionMode::Replace.apply(&current, &new), new);
        assert_eq!(SelectionMode::Add.apply(&current, &new), drawn("###."));
        assert_eq!(SelectionMode::Subtract.apply(&current, &new), drawn("#..."));
        assert_eq!(
            SelectionMode::Intersect.apply(&current, &new),
            drawn(".#..")
        );
    }

    fn random_cells(rng: &mut Rng) -> BTreeSet<(i32, i32)> {
        let count = rng.below(40).unwrap_or(0);
        let mut cells = BTreeSet::new();
        for _ in 0..count {
            // Small coordinates, negative ones included, so runs touch and overlap often.
            let x = rng.below(12).unwrap_or(0) as i32 - 4;
            let y = rng.below(5).unwrap_or(0) as i32 - 2;
            if rng.below(3) == Some(0) {
                let w = rng.below(5).unwrap_or(0) as i32;
                for dx in 0..w {
                    cells.insert((x + dx, y));
                }
            } else {
                cells.insert((x, y));
            }
        }
        cells
    }

    fn region_of(cells: &BTreeSet<(i32, i32)>) -> TileRegion {
        cells.iter().map(|&(x, y)| IVec2::new(x, y)).collect()
    }

    fn cells_of(region: &TileRegion) -> BTreeSet<(i32, i32)> {
        region.iter().map(|p| (p.x, p.y)).collect()
    }

    #[test]
    fn region_algebra_agrees_with_a_set_of_cells() {
        let fuzz = Fuzz::from_env(0x5E1E_C7ED, 500);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let (a, b) = (random_cells(&mut rng), random_cells(&mut rng));
            let (ra, rb) = (region_of(&a), region_of(&b));
            let context = || format!("case {case} ({}): a = {a:?}, b = {b:?}", fuzz.reproduce());
            let union: BTreeSet<_> = a.union(&b).copied().collect();
            let difference: BTreeSet<_> = a.difference(&b).copied().collect();
            let intersection: BTreeSet<_> = a.intersection(&b).copied().collect();
            let symmetric: BTreeSet<_> = a.symmetric_difference(&b).copied().collect();
            assert_eq!(cells_of(&ra.union(&rb)), union, "{}", context());
            assert_eq!(cells_of(&ra.subtract(&rb)), difference, "{}", context());
            assert_eq!(cells_of(&ra.intersect(&rb)), intersection, "{}", context());
            assert_eq!(cells_of(&ra.xor(&rb)), symmetric, "{}", context());
            assert_eq!(ra.union(&rb), region_of(&union), "one form: {}", context());
            assert_eq!(ra.len(), a.len(), "{}", context());
            assert_eq!(
                ra.intersects(&rb),
                !intersection.is_empty(),
                "{}",
                context()
            );
            assert_eq!(ra.is_subset(&rb), a.is_subset(&b), "{}", context());
            for &(x, y) in a.iter().chain(&b) {
                assert_eq!(
                    ra.contains(IVec2::new(x, y)),
                    a.contains(&(x, y)),
                    "{}",
                    context()
                );
            }
            let cells: Vec<IVec2> = ra.iter().collect();
            assert_eq!(ra.rects(), banded_rects(cells), "{}", context());
        }
    }
}
