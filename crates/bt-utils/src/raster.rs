//! Cells on a line and on an ellipse, for brushes, shapes and strokes.
//!
//! The line is Bresenham's algorithm, in the order Tiled's tools visit its cells. The ellipses
//! follow Alois Zingl's "Bresenham Curve Rasterizing Algorithms"
//! (<https://github.com/zingl/Bresenham>, MIT license; see `THIRD_PARTY_NOTICES.md`): the
//! outline is his `plotEllipse`, and the filled ellipse fills the rows of his `plotEllipseRect`,
//! which is how Tiled rasterises both.

use glam::IVec2;

use crate::rect::IRect;

/// Which neighbours a line may step to.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Connectivity {
    /// Any of the eight neighbours: a diagonal step is one step.
    Eight,
    /// Only the four that share a side: a diagonal step becomes two straight ones, so the line
    /// never hangs together by a corner alone.
    Four,
}

/// The cells on the segment from `a` to `b`, from `a`, each once and without gaps.
pub fn points_on_line(a: IVec2, b: IVec2, connectivity: Connectivity) -> Vec<IVec2> {
    let steep = (b.y - a.y).abs() > (b.x - a.x).abs();
    // Walk along the longer axis, called x below.
    let (mut start, mut end) = if steep {
        (IVec2::new(a.y, a.x), IVec2::new(b.y, b.x))
    } else {
        (a, b)
    };
    let reverse = start.x > end.x;
    if reverse {
        std::mem::swap(&mut start, &mut end);
    }
    let delta_x = end.x - start.x;
    let delta_y = (end.y - start.y).abs();
    let y_step = if start.y < end.y { 1 } else { -1 };
    let point = |x: i32, y: i32| {
        if steep {
            IVec2::new(y, x)
        } else {
            IVec2::new(x, y)
        }
    };

    let mut error = delta_x / 2;
    let mut y = start.y;
    let mut points = Vec::new();
    for x in start.x..=end.x {
        points.push(point(x, y));
        error -= delta_y;
        if error < 0 {
            y += y_step;
            error += delta_x;
            if connectivity == Connectivity::Four && x < end.x {
                points.push(point(x, y));
            }
        }
    }
    if reverse {
        points.reverse();
    }
    points
}

/// The outline of the ellipse centred on `center` with `radii`, quadrant by quadrant as Zingl's
/// `plotEllipse` visits it (cells on an axis appear more than once).
pub fn points_on_ellipse(center: IVec2, radii: IVec2) -> Vec<IVec2> {
    let (xm, ym) = (i64::from(center.x), i64::from(center.y));
    let (a, b) = (i64::from(radii.x), i64::from(radii.y));
    let mut points = Vec::new();
    let mut push = |x: i64, y: i64| {
        // Every coordinate is between the centre and the centre plus or minus a radius.
        if let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) {
            points.push(IVec2::new(x, y));
        }
    };

    // The second quadrant, from the bottom left to the top right.
    let (mut x, mut y) = (-a, 0i64);
    let mut dx = (1 + 2 * x) * b * b;
    let mut dy = x * x;
    let mut err = dx + dy;
    loop {
        push(xm - x, ym + y);
        push(xm + x, ym + y);
        push(xm + x, ym - y);
        push(xm - x, ym - y);
        let e2 = 2 * err;
        if e2 >= dx {
            x += 1;
            dx += 2 * b * b;
            err += dx;
        }
        if e2 <= dy {
            y += 1;
            dy += 2 * a * a;
            err += dy;
        }
        if x > 0 {
            break;
        }
    }
    // Flat ellipses (a radius of 1) stop too early: finish the tips.
    while y < b {
        y += 1;
        push(xm, ym + y);
        push(xm, ym - y);
    }
    points
}

/// The filled ellipse inscribed in `rect`, as one span per row from the top: each span is a
/// rectangle one row high. Empty for an empty `rect`.
pub fn ellipse_region(rect: IRect) -> Vec<IRect> {
    if rect.is_empty() {
        return Vec::new();
    }
    let (mut x0, mut y0) = (i64::from(rect.x), i64::from(rect.y));
    let (mut x1, mut y1) = (x0 + i64::from(rect.w) - 1, y0 + i64::from(rect.h) - 1);
    let mut rows = Rows::default();

    let mut a = (x1 - x0).abs();
    let b = (y1 - y0).abs();
    let mut b1 = b & 1;
    let mut dx = 4.0 * (1.0 - a as f64) * (b * b) as f64;
    let mut dy = (4 * (b1 + 1) * a * a) as f64;
    let mut err = dx + dy + (b1 * a * a) as f64;
    if x0 > x1 {
        x0 = x1;
        x1 += a;
    }
    if y0 > y1 {
        y0 = y1;
    }
    y0 += (b + 1) / 2;
    y1 = y0 - b1;
    a = 8 * a * a;
    b1 = 8 * b * b;

    loop {
        rows.add_span(y0, x0, x1);
        rows.add_span(y1, x0, x1);
        let e2 = 2.0 * err;
        if e2 <= dy {
            y0 += 1;
            y1 -= 1;
            dy += a as f64;
            err += dy;
        }
        if e2 >= dx || 2.0 * err > dy {
            x0 += 1;
            x1 -= 1;
            dx += b1 as f64;
            err += dx;
        }
        if x0 > x1 {
            break;
        }
    }
    // Flat ellipses (a height of 2 or less) stop too early: finish the tips.
    while y0 - y1 <= b {
        rows.add_span(y0, x0 - 1, x1 + 1);
        rows.add_span(y1, x0 - 1, x1 + 1);
        y0 += 1;
        y1 -= 1;
    }
    rows.into_rects()
}

/// The widest span seen for each row.
#[derive(Default)]
struct Rows {
    spans: std::collections::BTreeMap<i64, Span>,
}

/// Columns `start..=end` of a row.
#[derive(Clone, Copy)]
struct Span {
    start: i64,
    end: i64,
}

impl Rows {
    fn add_span(&mut self, row: i64, start: i64, end: i64) {
        if start > end {
            return;
        }
        self.spans
            .entry(row)
            .and_modify(|span| {
                span.start = span.start.min(start);
                span.end = span.end.max(end);
            })
            .or_insert(Span { start, end });
    }

    fn into_rects(self) -> Vec<IRect> {
        self.spans
            .into_iter()
            .filter_map(|(row, span)| {
                let x = i32::try_from(span.start).ok()?;
                let y = i32::try_from(row).ok()?;
                let w = i32::try_from(span.end - span.start + 1).ok()?;
                Some(IRect { x, y, w, h: 1 })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `"(0,0) (1,0)"` as cells.
    fn cells(text: &str) -> Vec<IVec2> {
        text.split_whitespace()
            .map(|p| {
                let (x, y) = p.trim_matches(['(', ')']).split_once(',').unwrap();
                IVec2::new(x.parse().unwrap(), y.parse().unwrap())
            })
            .collect()
    }

    fn sorted(mut cells: Vec<IVec2>) -> Vec<IVec2> {
        cells.sort_by_key(|p| (p.x, p.y));
        cells.dedup();
        cells
    }

    #[test]
    fn lines_visit_cells_as_tileds_tools_do() {
        use Connectivity::{Eight, Four};
        let cases = [
            ((0, 0, 5, 2, Eight), "(0,0) (1,0) (2,1) (3,1) (4,2) (5,2)"),
            (
                (0, 0, 5, 2, Four),
                "(0,0) (1,0) (1,1) (2,1) (3,1) (3,2) (4,2) (5,2)",
            ),
            ((5, 2, 0, 0, Eight), "(5,2) (4,2) (3,1) (2,1) (1,0) (0,0)"),
            (
                (0, 0, 2, 7, Eight),
                "(0,0) (0,1) (1,2) (1,3) (1,4) (1,5) (2,6) (2,7)",
            ),
            (
                (3, 3, -4, 1, Eight),
                "(3,3) (2,3) (1,2) (0,2) (-1,2) (-2,2) (-3,1) (-4,1)",
            ),
            ((0, 0, 4, 4, Eight), "(0,0) (1,1) (2,2) (3,3) (4,4)"),
            (
                (0, 0, 4, 4, Four),
                "(0,0) (0,1) (1,1) (1,2) (2,2) (2,3) (3,3) (3,4) (4,4)",
            ),
            ((1, 1, 1, 1, Eight), "(1,1)"),
            ((0, 0, 4, 1, Eight), "(0,0) (1,0) (2,0) (3,1) (4,1)"),
            ((0, 0, 4, 3, Eight), "(0,0) (1,1) (2,1) (3,2) (4,3)"),
        ];
        for ((x0, y0, x1, y1, connectivity), expected) in cases {
            let line = points_on_line(IVec2::new(x0, y0), IVec2::new(x1, y1), connectivity);
            assert_eq!(line, cells(expected), "{x0},{y0} to {x1},{y1}");
        }
    }

    #[test]
    fn ellipse_outlines_visit_quadrant_by_quadrant() {
        let cases = [
            ((0, 0, 0, 0), "(0,0) (0,0) (0,0) (0,0)"),
            (
                (0, 0, 1, 1),
                "(1,0) (-1,0) (-1,0) (1,0) (0,1) (0,1) (0,-1) (0,-1)",
            ),
            (
                (0, 0, 3, 2),
                "(3,0) (-3,0) (-3,0) (3,0) (3,1) (-3,1) (-3,-1) (3,-1) (2,1) (-2,1) (-2,-1) (2,-1) (1,2) (-1,2) \
                 (-1,-2) (1,-2) (0,2) (0,2) (0,-2) (0,-2)",
            ),
            (
                (5, 5, 2, 4),
                "(7,5) (3,5) (3,5) (7,5) (7,6) (3,6) (3,4) (7,4) (7,7) (3,7) (3,3) (7,3) (6,8) (4,8) (4,2) (6,2) \
                 (5,9) (5,9) (5,1) (5,1)",
            ),
            (
                (0, 0, 0, 3),
                "(0,0) (0,0) (0,0) (0,0) (0,1) (0,-1) (0,2) (0,-2) (0,3) (0,-3)",
            ),
        ];
        for ((xm, ym, a, b), expected) in cases {
            let outline = points_on_ellipse(IVec2::new(xm, ym), IVec2::new(a, b));
            assert_eq!(outline, cells(expected), "centre {xm},{ym} radii {a},{b}");
        }
    }

    #[test]
    fn filled_ellipses_cover_the_cells_tiled_fills() {
        let cases = [
            ((0, 0, 0, 0), "(0,0)"),
            ((0, 0, 1, 0), "(0,0) (1,0)"),
            (
                (0, 0, 4, 2),
                "(0,1) (1,0) (1,1) (1,2) (2,0) (2,1) (2,2) (3,0) (3,1) (3,2) (4,1)",
            ),
            (
                (0, 0, 6, 6),
                "(0,2) (0,3) (0,4) (1,1) (1,2) (1,3) (1,4) (1,5) (2,0) (2,1) (2,2) (2,3) (2,4) (2,5) (2,6) (3,0) \
                 (3,1) (3,2) (3,3) (3,4) (3,5) (3,6) (4,0) (4,1) (4,2) (4,3) (4,4) (4,5) (4,6) (5,1) (5,2) (5,3) \
                 (5,4) (5,5) (6,2) (6,3) (6,4)",
            ),
            (
                (2, 3, 9, 5),
                "(2,4) (3,4) (4,3) (4,4) (4,5) (5,3) (5,4) (5,5) (6,3) (6,4) (6,5) (7,3) (7,4) (7,5) (8,4) (9,4)",
            ),
            ((0, 0, 0, 5), "(0,0) (0,1) (0,2) (0,3) (0,4) (0,5)"),
            (
                (-3, -2, 3, 2),
                "(-3,0) (-2,-1) (-2,0) (-2,1) (-1,-2) (-1,-1) (-1,0) (-1,1) (-1,2) (0,-2) (0,-1) (0,0) (0,1) (0,2) \
                 (1,-2) (1,-1) (1,0) (1,1) (1,2) (2,-1) (2,0) (2,1) (3,0)",
            ),
            (
                (0, 0, 7, 3),
                "(0,1) (0,2) (1,1) (1,2) (2,0) (2,1) (2,2) (2,3) (3,0) (3,1) (3,2) (3,3) (4,0) (4,1) (4,2) (4,3) \
                 (5,0) (5,1) (5,2) (5,3) (6,1) (6,2) (7,1) (7,2)",
            ),
            (
                (1, 1, 5, 4),
                "(1,2) (1,3) (2,1) (2,2) (2,3) (2,4) (3,1) (3,2) (3,3) (3,4) (4,1) (4,2) (4,3) (4,4) (5,2) (5,3)",
            ),
        ];
        for ((x0, y0, x1, y1), expected) in cases {
            let rows = ellipse_region(IRect::from_corners(IVec2::new(x0, y0), IVec2::new(x1, y1)));
            assert!(
                rows.windows(2).all(|pair| pair[0].y < pair[1].y),
                "one span per row, from the top"
            );
            let filled: Vec<IVec2> = rows.iter().flat_map(IRect::points).collect();
            assert_eq!(
                sorted(filled),
                sorted(cells(expected)),
                "{x0},{y0} to {x1},{y1}"
            );
        }
        assert!(
            ellipse_region(IRect {
                x: 3,
                y: 3,
                w: 0,
                h: 4
            })
            .is_empty()
        );
    }
}
