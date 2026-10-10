//! Cells as runs: a set of cells travels as the stretches of each row it covers, so a selection
//! of ten thousand cells is a short list.

use std::cmp::Ordering;

use glam::IVec2;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The cells of row `y` from column `x0`, included, to `x1`, excluded. Written as the array
/// `[y, x0, x1]`; one with no cells (`x1` not after `x0`) is refused when read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Run {
    /// The row.
    pub y: i32,
    /// The first column.
    pub x0: i32,
    /// The column after the last.
    pub x1: i32,
}

impl Run {
    /// The cells of the run, left to right.
    pub fn cells(&self) -> impl Iterator<Item = IVec2> + use<> {
        let y = self.y;
        (self.x0..self.x1).map(move |x| IVec2::new(x, y))
    }

    /// How many cells the run covers.
    pub fn width(&self) -> u32 {
        if self.x1 > self.x0 {
            self.x1.abs_diff(self.x0)
        } else {
            0
        }
    }
}

/// The runs covering `cells`, given in any order: row by row from the top, left to right in a
/// row, cells next to each other in a row in one run, a cell given twice counted once. A cell in
/// the last column an `i32` can hold has no run (its run would end past it) and is left out.
pub fn runs_from_cells(cells: impl IntoIterator<Item = IVec2>) -> Vec<Run> {
    let mut sorted: Vec<IVec2> = cells.into_iter().collect();
    sorted.sort_unstable_by(row_major);
    sorted.dedup();
    let mut runs: Vec<Run> = Vec::new();
    for cell in sorted {
        let Some(after) = cell.x.checked_add(1) else {
            continue;
        };
        match runs.last_mut() {
            Some(run) if run.y == cell.y && run.x1 == cell.x => run.x1 = after,
            _ => runs.push(Run {
                y: cell.y,
                x0: cell.x,
                x1: after,
            }),
        }
    }
    runs
}

/// The cells of `runs`, given in any order and overlapping or not: row by row from the top,
/// left to right in a row, each once.
pub fn cells_from_runs(runs: &[Run]) -> Vec<IVec2> {
    let mut cells: Vec<IVec2> = runs.iter().flat_map(Run::cells).collect();
    cells.sort_unstable_by(row_major);
    cells.dedup();
    cells
}

/// Row by row from the top, then left to right.
fn row_major(a: &IVec2, b: &IVec2) -> Ordering {
    a.y.cmp(&b.y).then(a.x.cmp(&b.x))
}

impl Serialize for Run {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        [self.y, self.x0, self.x1].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Run {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Run, D::Error> {
        let [y, x0, x1] = <[i32; 3]>::deserialize(deserializer)?;
        if x1 <= x0 {
            return Err(serde::de::Error::custom(format!(
                "the run [{y}, {x0}, {x1}] has no cells: its end must be after its start"
            )));
        }
        Ok(Run { y, x0, x1 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: i32, y: i32) -> IVec2 {
        IVec2::new(x, y)
    }

    #[test]
    fn runs_and_cells_convert_both_ways() {
        let cells = [
            cell(3, 1),
            cell(1, 0),
            cell(2, 1),
            cell(0, 0),
            cell(5, 1),
            cell(1, 0),
            cell(-2, -4),
        ];
        let runs = runs_from_cells(cells);
        let run = |y, x0, x1| Run { y, x0, x1 };
        assert_eq!(
            runs,
            [run(-4, -2, -1), run(0, 0, 2), run(1, 2, 4), run(1, 5, 6)],
            "sorted, merged and counted once"
        );
        assert_eq!(
            cells_from_runs(&runs),
            [
                cell(-2, -4),
                cell(0, 0),
                cell(1, 0),
                cell(2, 1),
                cell(3, 1),
                cell(5, 1)
            ]
        );
        assert_eq!(runs_from_cells(cells_from_runs(&runs)), runs, "both ways");

        let overlapping = [run(2, 0, 3), run(0, 5, 6), run(2, 2, 5)];
        assert_eq!(
            runs_from_cells(cells_from_runs(&overlapping)),
            [run(0, 5, 6), run(2, 0, 5)],
            "runs in any order, overlapping, give each cell once"
        );
        assert!(runs_from_cells([]).is_empty());
        assert!(cells_from_runs(&[]).is_empty());
    }

    #[test]
    fn a_large_area_is_a_short_list() {
        let cells = (0..100).flat_map(|y| (0..100).map(move |x| cell(x, y)));
        let runs = runs_from_cells(cells);
        assert_eq!(runs.len(), 100);
        assert!(runs.iter().all(|run| run.width() == 100));
        assert_eq!(cells_from_runs(&runs).len(), 10_000);
    }

    #[test]
    fn the_last_column_has_no_run() {
        let runs = runs_from_cells([cell(i32::MAX, 0), cell(i32::MAX - 1, 0)]);
        assert_eq!(
            runs,
            [Run {
                y: 0,
                x0: i32::MAX - 1,
                x1: i32::MAX
            }]
        );
        let wide = Run {
            y: 0,
            x0: i32::MIN,
            x1: i32::MAX,
        };
        assert_eq!(wide.width(), u32::MAX);
        assert_eq!(Run { y: 0, x0: 3, x1: 3 }.width(), 0);
    }

    #[test]
    fn runs_are_written_as_arrays_and_empty_ones_are_refused() {
        let run = Run {
            y: 7,
            x0: -1,
            x1: 4,
        };
        assert_eq!(serde_json::to_string(&run).unwrap(), "[7,-1,4]");
        assert_eq!(serde_json::from_str::<Run>("[7,-1,4]").unwrap(), run);
        for empty in ["[7,4,4]", "[7,5,4]"] {
            let error = serde_json::from_str::<Run>(empty).unwrap_err().to_string();
            assert!(error.contains("has no cells"), "{empty}: {error}");
        }
        assert!(serde_json::from_str::<Run>("[7,4]").is_err());
        assert!(serde_json::from_str::<Run>(r#"{"y":7,"x0":1,"x1":4}"#).is_err());
    }
}
