//! Rules as Tiled finds them (`AutoMapper::setupRules`, `coherentRegions`): the 8-connected
//! components of the input and output cells together, ordered by the top-left of their boxes,
//! each split back into its input and output part.

use std::collections::VecDeque;

use bt_utils::hash::CellHashSet;
use glam::IVec2;

use crate::bx::Bx;

/// One 8-connected component of rule cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Component {
    /// Its cells, row by row from the top, each row from the left.
    pub(crate) cells: Vec<IVec2>,
    /// The box of its cells.
    pub(crate) bounds: Bx,
}

/// The 8-connected components of `cells`, ordered as Tiled orders rules: by the top of their
/// box, then its left; components with the same top-left (one wrapped around the other's
/// corner) by their first cell.
pub(crate) fn components(cells: &CellHashSet<IVec2>) -> Vec<Component> {
    let mut seen: CellHashSet<IVec2> = CellHashSet::default();
    let mut ordered: Vec<IVec2> = cells.iter().copied().collect();
    ordered.sort_unstable_by_key(|p| (p.y, p.x));
    let mut result = Vec::new();
    for &start in &ordered {
        if !seen.insert(start) {
            continue;
        }
        let mut component = vec![start];
        let mut queue = VecDeque::from([start]);
        while let Some(p) = queue.pop_front() {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let q = p + IVec2::new(dx, dy);
                    if cells.contains(&q) && seen.insert(q) {
                        component.push(q);
                        queue.push_back(q);
                    }
                }
            }
        }
        component.sort_unstable_by_key(|p| (p.y, p.x));
        if let Some(bounds) = Bx::bounding(component.iter().copied()) {
            result.push(Component {
                cells: component,
                bounds,
            });
        }
    }
    result.sort_by_key(|c| {
        let first = c.cells.first().copied().unwrap_or(c.bounds.min);
        (c.bounds.min.y, c.bounds.min.x, first.y, first.x)
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(cells: &[(i32, i32)]) -> CellHashSet<IVec2> {
        cells.iter().map(|&(x, y)| IVec2::new(x, y)).collect()
    }

    #[test]
    fn diagonal_neighbours_join_and_gaps_split() {
        let found = components(&set(&[(0, 0), (1, 1), (3, 1), (4, 0)]));
        let cells: Vec<Vec<IVec2>> = found.into_iter().map(|c| c.cells).collect();
        assert_eq!(
            cells,
            [
                vec![IVec2::new(0, 0), IVec2::new(1, 1)],
                vec![IVec2::new(4, 0), IVec2::new(3, 1)],
            ]
        );
    }

    #[test]
    fn rules_are_ordered_by_the_top_left_of_their_boxes() {
        // The tall rule starts higher, so it comes first though its first cell is further right.
        let found = components(&set(&[(5, 0), (5, 1), (0, 1), (0, 2), (9, 1)]));
        let tops: Vec<IVec2> = found.iter().map(|c| c.bounds.min).collect();
        assert_eq!(tops, [IVec2::new(5, 0), IVec2::new(0, 1), IVec2::new(9, 1)]);
    }

    #[test]
    fn an_empty_set_has_no_rules() {
        assert!(components(&CellHashSet::default()).is_empty());
    }
}
