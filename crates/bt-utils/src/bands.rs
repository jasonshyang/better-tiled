//! A set of cells as banded rectangles: the form Qt's `QRegion` stores a region in, and so the
//! order in which Tiled's terrain filler and automapper visit one.

use glam::IVec2;

use crate::rect::IRect;

/// The cells as `QRegion`'s y-x bands: each row is split into maximal runs, consecutive rows with
/// identical runs are merged into one band, and the rectangles come band by band from the top,
/// left to right within a band. Duplicate cells count once.
pub fn banded_rects(cells: impl IntoIterator<Item = IVec2>) -> Vec<IRect> {
    let mut cells: Vec<IVec2> = cells.into_iter().collect();
    cells.sort_unstable_by_key(|p| (p.y, p.x));
    cells.dedup();

    let mut rows: Vec<Row> = Vec::new();
    for p in cells {
        match rows.last_mut() {
            Some(row) if row.y == p.y => match row.runs.last_mut() {
                Some(run) if run.end + 1 == p.x => run.end = p.x,
                _ => row.runs.push(Run {
                    start: p.x,
                    end: p.x,
                }),
            },
            _ => rows.push(Row {
                y: p.y,
                runs: vec![Run {
                    start: p.x,
                    end: p.x,
                }],
            }),
        }
    }

    let mut bands: Vec<Band> = Vec::new();
    for row in rows {
        match bands.last_mut() {
            Some(band) if band.bottom + 1 == row.y && band.runs == row.runs => band.bottom = row.y,
            _ => bands.push(Band {
                top: row.y,
                bottom: row.y,
                runs: row.runs,
            }),
        }
    }

    bands
        .into_iter()
        .flat_map(|band| {
            band.runs.into_iter().map(move |run| IRect {
                x: run.start,
                y: band.top,
                w: run.end - run.start + 1,
                h: band.bottom - band.top + 1,
            })
        })
        .collect()
}

/// Columns `start..=end` of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Run {
    start: i32,
    end: i32,
}

/// The runs of one row.
struct Row {
    y: i32,
    runs: Vec<Run>,
}

/// Rows `top..=bottom`, each with the same runs.
struct Band {
    top: i32,
    bottom: i32,
    runs: Vec<Run>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(points: &[(i32, i32)]) -> Vec<IVec2> {
        points.iter().map(|&(x, y)| IVec2::new(x, y)).collect()
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn a_square_is_one_band() {
        assert_eq!(
            banded_rects(cells(&[(1, 1), (0, 0), (1, 0), (0, 1), (0, 0)])),
            [rect(0, 0, 2, 2)]
        );
        assert!(banded_rects([]).is_empty());
    }

    #[test]
    fn bands_split_where_the_runs_change() {
        // ##.##
        // ##.##
        // #####
        let shape = [
            (0, 0),
            (1, 0),
            (3, 0),
            (4, 0),
            (0, 1),
            (1, 1),
            (3, 1),
            (4, 1),
        ];
        let bottom = [(0, 2), (1, 2), (2, 2), (3, 2), (4, 2)];
        let all: Vec<IVec2> = cells(&shape).into_iter().chain(cells(&bottom)).collect();
        assert_eq!(
            banded_rects(all),
            [rect(0, 0, 2, 2), rect(3, 0, 2, 2), rect(0, 2, 5, 1)]
        );
    }

    #[test]
    fn rows_with_a_gap_between_them_are_separate_bands() {
        assert_eq!(
            banded_rects(cells(&[(0, 0), (0, 2), (5, 5)])),
            [rect(0, 0, 1, 1), rect(0, 2, 1, 1), rect(5, 5, 1, 1)]
        );
    }
}
