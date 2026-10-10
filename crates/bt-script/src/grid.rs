//! How cells sit next to each other on each kind of map (`10-agents.md`, §6.2). Cells that share
//! a side are connected: four sides on orthogonal, isometric and oblique maps, six on hexagonal
//! maps, the four diagonal neighbours on staggered maps, whose diamonds meet their row
//! neighbours only at a corner. A margin counts cells that touch, at a side or a corner: eight
//! on orthogonal, isometric, oblique and staggered maps, six on hexagonal ones. Which cells count
//! is the caller's: every function takes the test that says.

use std::collections::VecDeque;

use bt_core::{MapGeometry, Orientation, StaggerAxis};
use bt_utils::hash::{CellHashMap, CellHashSet};
use glam::IVec2;
use serde::{Deserialize, Serialize};

/// A cell and how many steps it is from somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nearest {
    /// The cell.
    pub cell: IVec2,
    /// The steps between cells that share a side.
    pub steps: usize,
}

/// The cells sharing a side with `cell`, clockwise.
pub fn sides(geometry: &MapGeometry, cell: IVec2) -> impl Iterator<Item = IVec2> + use<> {
    let staggered = geometry.orientation == Orientation::Staggered;
    let diagonals = [
        geometry.top_right(cell),
        geometry.bottom_right(cell),
        geometry.bottom_left(cell),
        geometry.top_left(cell),
    ];
    diagonals
        .into_iter()
        .filter(move |_| staggered)
        .chain(geometry.neighbors(cell).filter(move |_| !staggered))
}

/// The cells touching `cell` at a side or a corner: what margins count.
pub fn touching(geometry: &MapGeometry, cell: IVec2) -> Vec<IVec2> {
    match geometry.orientation {
        Orientation::Hexagonal => sides(geometry, cell).collect(),
        Orientation::Staggered => {
            // Diamonds meet the ones beside them in their row (or column) at a corner, and the
            // ones two rows (columns) away at another.
            let corners = match geometry.stagger_axis {
                StaggerAxis::Y => [IVec2::NEG_X, IVec2::X, IVec2::NEG_Y * 2, IVec2::Y * 2],
                StaggerAxis::X => [IVec2::NEG_Y, IVec2::Y, IVec2::NEG_X * 2, IVec2::X * 2],
            };
            sides(geometry, cell)
                .chain(corners.into_iter().map(|offset| cell + offset))
                .collect()
        }
        Orientation::Orthogonal | Orientation::Isometric | Orientation::Oblique => (-1..=1)
            .flat_map(|y| (-1..=1).map(move |x| IVec2::new(x, y)))
            .filter(|&offset| offset != IVec2::ZERO)
            .map(|offset| cell + offset)
            .collect(),
    }
}

/// The cells at most `steps` touching steps from one of `from`, those included.
fn spread(
    geometry: &MapGeometry,
    from: impl IntoIterator<Item = IVec2>,
    steps: u32,
) -> CellHashSet<IVec2> {
    let mut seen: CellHashSet<IVec2> = from.into_iter().collect();
    let mut frontier: Vec<IVec2> = seen.iter().copied().collect();
    for _ in 0..steps {
        let mut next = Vec::new();
        for cell in frontier {
            for near in touching(geometry, cell) {
                if seen.insert(near) {
                    next.push(near);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    seen
}

/// The cells of `cells` at most `margin` touching steps from a cell where `is_target` holds; a
/// margin of 0 keeps the cells that are targets themselves.
pub fn within(
    geometry: &MapGeometry,
    cells: &[IVec2],
    margin: u32,
    is_target: impl Fn(IVec2) -> bool,
) -> CellHashSet<IVec2> {
    let reach = spread(geometry, cells.iter().copied(), margin);
    let targets: Vec<IVec2> = reach.into_iter().filter(|&cell| is_target(cell)).collect();
    let close = spread(geometry, targets, margin);
    cells
        .iter()
        .copied()
        .filter(|cell| close.contains(cell))
        .collect()
}

/// The cells connected to `seeds` through cells that share a side and where `allowed` holds,
/// the seeds included whatever they are.
pub fn flood(
    geometry: &MapGeometry,
    seeds: &[IVec2],
    allowed: impl Fn(IVec2) -> bool,
) -> CellHashSet<IVec2> {
    let mut seen: CellHashSet<IVec2> = seeds.iter().copied().collect();
    let mut queue: VecDeque<IVec2> = seeds.iter().copied().collect();
    while let Some(cell) = queue.pop_front() {
        for next in sides(geometry, cell) {
            if !seen.contains(&next) && allowed(next) {
                seen.insert(next);
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The connected parts of `cells`, each row by row, the largest first and then by their first
/// cell.
pub fn areas(geometry: &MapGeometry, cells: &[IVec2]) -> Vec<Vec<IVec2>> {
    let all: CellHashSet<IVec2> = cells.iter().copied().collect();
    let mut done: CellHashSet<IVec2> = CellHashSet::default();
    let mut parts: Vec<Vec<IVec2>> = Vec::new();
    for cell in sorted_cells(cells.iter().copied()) {
        if done.contains(&cell) {
            continue;
        }
        let part = sorted_cells(flood(geometry, &[cell], |c| all.contains(&c)));
        done.extend(part.iter().copied());
        parts.push(part);
    }
    parts.sort_by(|a, b| {
        b.len()
            .cmp(&a.len())
            .then_with(|| reading_key(a.first()).cmp(&reading_key(b.first())))
    });
    parts
}

/// The shortest way from one of `from` to one of `to` between cells that share a side: every
/// cell between the ends is one where `allowed` holds, and the ends need not be. The cells along
/// it, ends included; `None` when there is none. `allowed` must hold for finitely many cells.
pub fn way(
    geometry: &MapGeometry,
    from: &[IVec2],
    to: &[IVec2],
    allowed: impl Fn(IVec2) -> bool,
) -> Option<Vec<IVec2>> {
    let ends: CellHashSet<IVec2> = to.iter().copied().collect();
    if let Some(&both) = from.iter().find(|cell| ends.contains(cell)) {
        return Some(vec![both]);
    }
    let mut came: CellHashMap<IVec2, IVec2> = CellHashMap::default();
    let mut queue: VecDeque<IVec2> = VecDeque::new();
    for &cell in from {
        if came.insert(cell, cell).is_none() {
            queue.push_back(cell);
        }
    }
    while let Some(cell) = queue.pop_front() {
        for next in sides(geometry, cell) {
            if came.contains_key(&next) {
                continue;
            }
            if ends.contains(&next) {
                let mut cells = vec![next, cell];
                let mut at = cell;
                while let Some(&before) = came.get(&at).filter(|&&before| before != at) {
                    cells.push(before);
                    at = before;
                }
                cells.reverse();
                return Some(cells);
            }
            if allowed(next) {
                came.insert(next, cell);
                queue.push_back(next);
            }
        }
    }
    None
}

/// The cells where a way changes direction, and its ends: what describes it in a few cells.
pub fn turns(geometry: &MapGeometry, way: &[IVec2]) -> Vec<IVec2> {
    let direction = |from: IVec2, to: IVec2| sides(geometry, from).position(|side| side == to);
    let mut out = Vec::new();
    let last = way.len().saturating_sub(1);
    for (index, &cell) in way.iter().enumerate() {
        let straight = index > 0
            && index < last
            && match (way.get(index - 1), way.get(index + 1)) {
                (Some(&before), Some(&after)) => {
                    direction(before, cell).is_some()
                        && direction(before, cell) == direction(cell, after)
                }
                _ => false,
            };
        if !straight {
            out.push(cell);
        }
    }
    out
}

/// The cells every way from `from` to `to` passes through, so that closing one closes them all,
/// not counting the ends, in the order of `shortest` (a way [`way`] found). Ways go as [`way`]
/// takes them.
pub fn on_every_way(
    geometry: &MapGeometry,
    from: &[IVec2],
    to: &[IVec2],
    allowed: impl Fn(IVec2) -> bool,
    shortest: &[IVec2],
) -> Vec<IVec2> {
    let cuts = Cuts::find(geometry, from, to, &allowed);
    let ends: CellHashSet<IVec2> = from.iter().chain(to).copied().collect();
    shortest
        .iter()
        .copied()
        .filter(|cell| !ends.contains(cell) && cuts.contains(cell))
        .collect()
}

/// The cell of `cells` nearest to one of `to`, counting the steps between cells that share a
/// side through any cell where `inside` holds (the first of the nearest, row by row). `inside`
/// must hold for finitely many cells.
pub fn nearest(
    geometry: &MapGeometry,
    to: &[IVec2],
    cells: &CellHashSet<IVec2>,
    inside: impl Fn(IVec2) -> bool,
) -> Option<Nearest> {
    let mut seen: CellHashSet<IVec2> = to.iter().copied().collect();
    let mut level: Vec<IVec2> = seen.iter().copied().collect();
    let mut steps = 0;
    while !level.is_empty() {
        let found = sorted_cells(level.iter().copied().filter(|cell| cells.contains(cell)));
        if let Some(&cell) = found.first() {
            return Some(Nearest { cell, steps });
        }
        let mut next = Vec::new();
        for cell in level {
            for side in sides(geometry, cell) {
                if inside(side) && seen.insert(side) {
                    next.push(side);
                }
            }
        }
        level = next;
        steps += 1;
    }
    None
}

/// Cells row by row, each once.
pub(crate) fn sorted_cells(cells: impl IntoIterator<Item = IVec2>) -> Vec<IVec2> {
    let mut cells: Vec<IVec2> = cells.into_iter().collect();
    cells.sort_unstable_by_key(|cell| reading_key(Some(cell)));
    cells.dedup();
    cells
}

/// The order cells are read in: row by row, each row from the left.
fn reading_key(cell: Option<&IVec2>) -> Option<[i32; 2]> {
    cell.map(|cell| [cell.y, cell.x])
}

/// A node of the graph [`on_every_way`] searches: one end standing for every cell of `from`,
/// the other for every cell of `to`, and the cells between.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Node {
    From,
    To,
    Cell(IVec2),
}

/// The cut cells between the two ends, found by Tarjan's depth-first search for articulation
/// points (iteratively, so a large map does not overflow the stack): a cell separates the ends
/// when the subtree below it that holds the far end reaches no higher than the cell.
struct Cuts {
    cells: CellHashSet<IVec2>,
}

/// A node on the search's stack, with the next of its neighbours to try.
struct Frame {
    node: usize,
    next: usize,
}

/// What the search knows of a node.
#[derive(Clone, Copy)]
struct Visit {
    discovered: usize,
    low: usize,
    parent: Option<usize>,
    holds_to: bool,
}

impl Cuts {
    fn find(
        geometry: &MapGeometry,
        from: &[IVec2],
        to: &[IVec2],
        allowed: &impl Fn(IVec2) -> bool,
    ) -> Cuts {
        let starts: CellHashSet<IVec2> = from.iter().copied().collect();
        let ends: CellHashSet<IVec2> = to.iter().copied().collect();
        let member = |cell: IVec2| allowed(cell) || starts.contains(&cell) || ends.contains(&cell);
        let neighbour = |node: Node, index: usize| -> Option<Node> {
            match node {
                Node::From => from.get(index).copied().map(Node::Cell),
                Node::To => to.get(index).copied().map(Node::Cell),
                Node::Cell(cell) => {
                    let mut around: Vec<Node> = sides(geometry, cell)
                        .filter(|&side| member(side))
                        .map(Node::Cell)
                        .collect();
                    if starts.contains(&cell) {
                        around.push(Node::From);
                    }
                    if ends.contains(&cell) {
                        around.push(Node::To);
                    }
                    around.get(index).copied()
                }
            }
        };
        let mut nodes: Vec<Node> = vec![Node::From];
        let mut index_of: CellHashMap<IVec2, usize> = CellHashMap::default();
        let mut to_index: Option<usize> = None;
        let mut visits: Vec<Visit> = vec![Visit {
            discovered: 0,
            low: 0,
            parent: None,
            holds_to: false,
        }];
        let mut stack = vec![Frame { node: 0, next: 0 }];
        let mut cuts = CellHashSet::default();
        while let Some(frame) = stack.last_mut() {
            let node = frame.node;
            let Some(&current) = nodes.get(node) else {
                break;
            };
            if let Some(next) = neighbour(current, frame.next) {
                frame.next += 1;
                let known = match next {
                    Node::From => Some(0),
                    Node::To => to_index,
                    Node::Cell(cell) => index_of.get(&cell).copied(),
                };
                match known {
                    Some(seen) => {
                        let parent = visits.get(node).and_then(|v| v.parent);
                        let discovered = visits.get(seen).map_or(usize::MAX, |v| v.discovered);
                        if parent != Some(seen)
                            && let Some(visit) = visits.get_mut(node)
                        {
                            visit.low = visit.low.min(discovered);
                        }
                    }
                    None => {
                        let index = nodes.len();
                        nodes.push(next);
                        match next {
                            Node::To => to_index = Some(index),
                            Node::Cell(cell) => {
                                index_of.insert(cell, index);
                            }
                            Node::From => {}
                        }
                        visits.push(Visit {
                            discovered: index,
                            low: index,
                            parent: Some(node),
                            holds_to: next == Node::To,
                        });
                        stack.push(Frame {
                            node: index,
                            next: 0,
                        });
                    }
                }
            } else {
                stack.pop();
                let Some(&done) = visits.get(node) else {
                    continue;
                };
                let Some(parent) = done.parent else {
                    continue;
                };
                let Some(above) = visits.get_mut(parent) else {
                    continue;
                };
                above.low = above.low.min(done.low);
                above.holds_to |= done.holds_to;
                if done.low >= above.discovered
                    && done.holds_to
                    && let Some(Node::Cell(cell)) = nodes.get(parent)
                {
                    cuts.insert(*cell);
                }
            }
        }
        Cuts { cells: cuts }
    }

    fn contains(&self, cell: &IVec2) -> bool {
        self.cells.contains(cell)
    }
}

#[cfg(test)]
mod tests {
    use bt_core::StaggerIndex;

    use super::*;

    fn v(x: i32, y: i32) -> IVec2 {
        IVec2::new(x, y)
    }

    fn square() -> MapGeometry {
        MapGeometry::new(Orientation::Orthogonal, v(16, 16), v(20, 20))
    }

    fn staggered(axis: StaggerAxis) -> MapGeometry {
        let mut geometry = MapGeometry::new(Orientation::Staggered, v(64, 32), v(20, 20));
        geometry.stagger_axis = axis;
        geometry.stagger_index = StaggerIndex::Odd;
        geometry
    }

    fn hexagonal() -> MapGeometry {
        let mut geometry = MapGeometry::new(Orientation::Hexagonal, v(14, 12), v(20, 20));
        geometry.hex_side_length = 6;
        geometry
    }

    #[test]
    fn square_grids_connect_four_ways_and_margins_count_eight() {
        for orientation in [
            Orientation::Orthogonal,
            Orientation::Isometric,
            Orientation::Oblique,
        ] {
            let geometry = MapGeometry::new(orientation, v(32, 16), v(10, 10));
            let around: Vec<IVec2> = sides(&geometry, v(5, 5)).collect();
            assert_eq!(around, [v(5, 4), v(6, 5), v(5, 6), v(4, 5)]);
            assert_eq!(touching(&geometry, v(5, 5)).len(), 8);
        }
        // Two cells meeting at a corner are two areas.
        let parts = areas(&square(), &[v(1, 1), v(2, 2), v(2, 3)]);
        assert_eq!(parts, vec![vec![v(2, 2), v(2, 3)], vec![v(1, 1)]]);
    }

    #[test]
    fn staggered_cells_connect_by_their_diagonal_sides() {
        for axis in [StaggerAxis::Y, StaggerAxis::X] {
            let geometry = staggered(axis);
            let cell = v(4, 4);
            let around: Vec<IVec2> = sides(&geometry, cell).collect();
            assert_eq!(around.len(), 4);
            for side in &around {
                assert!(
                    sides(&geometry, *side).any(|back| back == cell),
                    "{axis:?}: {side} shares a side with {cell} both ways"
                );
            }
            let touch = touching(&geometry, cell);
            assert_eq!(touch.len(), 8);
            for side in &around {
                assert!(touch.contains(side));
            }
        }
        let rows = staggered(StaggerAxis::Y);
        // Diamonds side by side in a row meet at a corner only: not connected, but touching.
        assert!(!sides(&rows, v(4, 4)).any(|c| c == v(5, 4)));
        assert!(touching(&rows, v(4, 4)).contains(&v(5, 4)));
        assert!(touching(&rows, v(4, 4)).contains(&v(4, 6)));
        assert_eq!(areas(&rows, &[v(4, 4), v(5, 4)]).len(), 2);
        // An even row's diamond and the odd row's below and to the left share a side.
        assert_eq!(rows.bottom_left(v(4, 4)), v(3, 5));
        assert_eq!(areas(&rows, &[v(4, 4), v(3, 5), v(3, 6)]).len(), 1);
        let columns = staggered(StaggerAxis::X);
        assert!(!sides(&columns, v(4, 4)).any(|c| c == v(4, 5)));
        assert!(touching(&columns, v(4, 4)).contains(&v(6, 4)));
    }

    #[test]
    fn hexagons_have_six_mutual_sides_that_are_also_what_they_touch() {
        let geometry = hexagonal();
        let around: Vec<IVec2> = sides(&geometry, v(5, 5)).collect();
        assert_eq!(around.len(), 6);
        assert_eq!(touching(&geometry, v(5, 5)), around);
        for side in &around {
            assert!(
                sides(&geometry, *side).any(|back| back == v(5, 5)),
                "{side}"
            );
        }
    }

    #[test]
    fn margins_count_cells_that_touch() {
        let geometry = square();
        let cells = [v(5, 5), v(5, 8), v(7, 7)];
        let close = within(&geometry, &cells, 2, |c| c == v(5, 3));
        assert_eq!(close, CellHashSet::from_iter([v(5, 5)]));
        assert_eq!(within(&geometry, &[v(7, 5)], 2, |c| c == v(5, 3)).len(), 1);
        assert!(within(&geometry, &[v(8, 5)], 2, |c| c == v(5, 3)).is_empty());
        assert_eq!(
            within(&geometry, &cells, 0, |c| c == v(5, 8)),
            CellHashSet::from_iter([v(5, 8)]),
            "a margin of 0 keeps the targets themselves"
        );
    }

    #[test]
    fn ways_go_around_what_is_closed_and_the_ends_are_free() {
        let geometry = square();
        // A wall at x=3 with a gap at y=4; the start is on the wall itself.
        let open = |c: IVec2| c.x >= 0 && c.y >= 0 && c.x < 7 && c.y < 7 && (c.x != 3 || c.y == 4);
        let found = way(&geometry, &[v(3, 0)], &[v(6, 0)], open).expect("a way");
        assert_eq!(found, [v(3, 0), v(4, 0), v(5, 0), v(6, 0)]);
        let walled = |c: IVec2| open(c) && c.x != 3;
        let from_left = way(&geometry, &[v(0, 0)], &[v(6, 0)], open).expect("through the gap");
        assert_eq!(from_left.len(), 6 + 4 + 4 + 1);
        assert_eq!(way(&geometry, &[v(0, 0)], &[v(6, 0)], walled), None);
        assert_eq!(
            way(&geometry, &[v(1, 1)], &[v(1, 1)], walled),
            Some(vec![v(1, 1)])
        );
        assert_eq!(
            turns(&geometry, &[v(0, 0), v(1, 0), v(2, 0), v(2, 1)]),
            [v(0, 0), v(2, 0), v(2, 1)]
        );
        // The gap and the cells into and out of it are on every way.
        let every = on_every_way(&geometry, &[v(0, 0)], &[v(6, 0)], open, &from_left);
        assert_eq!(every, [v(2, 4), v(3, 4), v(4, 4)]);
        let field = |c: IVec2| c.x >= 0 && c.y >= 0 && c.x < 7 && c.y < 7;
        let straight = way(&geometry, &[v(0, 0)], &[v(6, 0)], field).unwrap();
        assert!(on_every_way(&geometry, &[v(0, 0)], &[v(6, 0)], field, &straight).is_empty());
    }

    #[test]
    fn a_straight_way_on_a_staggered_map_has_no_turns() {
        let geometry = staggered(StaggerAxis::Y);
        let mut cells = vec![v(2, 0)];
        for _ in 0..5 {
            let last = *cells.last().unwrap();
            cells.push(geometry.bottom_right(last));
        }
        assert_eq!(turns(&geometry, &cells), [v(2, 0), *cells.last().unwrap()]);
        let all = |c: IVec2| c.x >= 0 && c.y >= 0 && c.x < 20 && c.y < 20;
        let found = way(&geometry, &[v(2, 0)], &[*cells.last().unwrap()], all).unwrap();
        assert_eq!(found.len(), 6, "five diagonal steps");
    }

    #[test]
    fn floods_and_nearest_cells_count_sides() {
        let geometry = square();
        let room = |c: IVec2| c.x >= 0 && c.y >= 0 && c.x < 3 && c.y < 3;
        let reached = flood(&geometry, &[v(9, 9)], |c| room(c) || c == v(9, 9));
        assert_eq!(reached.len(), 1, "the seed's sides are outside the room");
        let reached = flood(&geometry, &[v(0, 0)], room);
        assert_eq!(reached.len(), 9);
        let found = nearest(&geometry, &[v(6, 1)], &reached, |c| {
            c.x >= 0 && c.y >= 0 && c.x < 10 && c.y < 10
        });
        assert_eq!(
            found,
            Some(Nearest {
                cell: v(2, 1),
                steps: 4
            })
        );
        assert_eq!(
            sorted_cells([v(2, 1), v(0, 2), v(2, 1), v(1, 1)]),
            [v(1, 1), v(2, 1), v(0, 2)]
        );
    }
}
