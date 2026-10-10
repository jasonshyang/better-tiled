//! The lattice a map's cells sit on, on which brushes and clips are mirrored and turned: squares
//! on orthogonal and oblique maps, diamonds on isometric maps and behind the shifted rows or
//! columns of staggered maps, and hexagons on hexagonal maps, where a turn is a sixth. Mirrors
//! are mirrors on screen, so on an isometric map a left-right mirror swaps the grid's axes, and
//! a shape keeps its form on every grid. A cell's tile changes as Tiled turns it, through
//! `bt_core::Flip`'s tables. Tiled mirrors and turns stamps in grid coordinates
//! (`TileStamp::flipped` and `rotated`, `TileLayer::flip`, `flipHexagonal`, `rotate` and
//! `rotateHexagonal`, `reference/tiled/src/tiled/tilestamp.cpp` and
//! `reference/tiled/src/libtiled/tilelayer.cpp`), which agrees on orthogonal and hexagonal maps.

use bt_core::{Flip, Hex, HexParams, MapGeometry, Orientation, StaggerAxis, StaggerIndex};
use bt_utils::IRect;
use glam::IVec2;

/// Which way a mirror goes, on screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FlipDirection {
    /// Left and right swap.
    Horizontal,
    /// Top and bottom swap.
    Vertical,
}

/// Which way a turn goes, on screen: a quarter turn, or a sixth on hexagonal maps.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RotateDirection {
    /// Clockwise (Tiled's Rotate Right).
    Clockwise,
    /// Counter-clockwise (Tiled's Rotate Left).
    CounterClockwise,
}

/// A mirror or a turn of the lattice around its origin.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    /// A mirror on screen.
    Mirror(FlipDirection),
    /// A turn on screen.
    Turn(RotateDirection),
}

/// The shifted rows or columns of a staggered or hexagonal map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stagger {
    /// Rows (`Y`) or columns (`X`) are shifted.
    pub(crate) axis: StaggerAxis,
    index: StaggerIndex,
    params: HexParams,
}

impl Stagger {
    /// Whether row `k` (stagger axis y) or column `k` (axis x) is one of the shifted ones.
    pub(crate) fn shifts(self, k: i32) -> bool {
        match self.axis {
            StaggerAxis::Y => self.params.do_stagger_y(k),
            StaggerAxis::X => self.params.do_stagger_x(k),
        }
    }
}

/// The lattice of a map's cells.
///
/// Every cell has a point with integer coordinates `(a, b)` aligned with the screen, `a` across
/// and `b` down: cells on square grids; half cells on diamond grids (`x - y`, `x + y`) and on
/// staggered and hexagonal grids along the shifted axis (`2x` plus one on a shifted row, and
/// `y`). Mirrors and quarter turns are then integer maps of points that take cells to cells, and
/// moving one cell onto another moves every cell by the same lattice step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lattice {
    /// Orthogonal and oblique maps.
    Square,
    /// Isometric maps.
    Diamond,
    /// Staggered maps: diamonds in shifted rows or columns.
    Staggered(Stagger),
    /// Hexagonal maps.
    Hexagonal(Stagger),
}

impl Lattice {
    /// The lattice of the grid `geometry` describes.
    pub(crate) fn of(geometry: &MapGeometry) -> Lattice {
        let stagger = Stagger {
            axis: geometry.stagger_axis,
            index: geometry.stagger_index,
            params: geometry.hex_params(),
        };
        match geometry.orientation {
            Orientation::Orthogonal | Orientation::Oblique => Lattice::Square,
            Orientation::Isometric => Lattice::Diamond,
            Orientation::Staggered => Lattice::Staggered(stagger),
            Orientation::Hexagonal => Lattice::Hexagonal(stagger),
        }
    }

    /// The shifted rows or columns, on staggered and hexagonal maps.
    pub(crate) fn stagger(self) -> Option<Stagger> {
        match self {
            Lattice::Staggered(stagger) | Lattice::Hexagonal(stagger) => Some(stagger),
            Lattice::Square | Lattice::Diamond => None,
        }
    }

    /// Whether a mirror or a quarter turn takes a rectangle of cells to a rectangle: on square
    /// and diamond grids, not where rows or columns are shifted.
    pub(crate) fn keeps_rectangles(self) -> bool {
        self.stagger().is_none()
    }

    /// A cell's point on the lattice.
    fn point(self, cell: IVec2) -> IVec2 {
        match self {
            Lattice::Square => cell,
            Lattice::Diamond => IVec2::new(cell.x - cell.y, cell.x + cell.y),
            Lattice::Staggered(stagger) | Lattice::Hexagonal(stagger) => match stagger.axis {
                StaggerAxis::Y => {
                    IVec2::new(2 * cell.x + i32::from(stagger.shifts(cell.y)), cell.y)
                }
                StaggerAxis::X => {
                    IVec2::new(cell.x, 2 * cell.y + i32::from(stagger.shifts(cell.x)))
                }
            },
        }
    }

    /// The cell at a point of the lattice (every motion and step here gives one).
    fn cell(self, point: IVec2) -> IVec2 {
        match self {
            Lattice::Square => point,
            Lattice::Diamond => IVec2::new(
                (point.x + point.y).div_euclid(2),
                (point.y - point.x).div_euclid(2),
            ),
            Lattice::Staggered(stagger) | Lattice::Hexagonal(stagger) => match stagger.axis {
                StaggerAxis::Y => IVec2::new(
                    (point.x - i32::from(stagger.shifts(point.y))).div_euclid(2),
                    point.y,
                ),
                StaggerAxis::X => IVec2::new(
                    point.x,
                    (point.y - i32::from(stagger.shifts(point.x))).div_euclid(2),
                ),
            },
        }
    }

    /// Where a mirror or a turn around the lattice's origin takes a cell. Turns are quarter
    /// turns, sixths on hexagonal maps (through cube coordinates, as Tiled turns hexagons).
    pub(crate) fn moved(self, cell: IVec2, motion: Motion) -> IVec2 {
        if let (Lattice::Hexagonal(stagger), Motion::Turn(direction)) = (self, motion) {
            let hex = Hex::from_offset(cell, stagger.axis, stagger.index);
            let turned = match direction {
                RotateDirection::Clockwise => hex.rotated_cw(),
                RotateDirection::CounterClockwise => hex.rotated_ccw(),
            };
            return turned.to_offset(stagger.axis, stagger.index);
        }
        let p = self.point(cell);
        let moved = match motion {
            Motion::Mirror(FlipDirection::Horizontal) => IVec2::new(-p.x, p.y),
            Motion::Mirror(FlipDirection::Vertical) => IVec2::new(p.x, -p.y),
            Motion::Turn(RotateDirection::Clockwise) => IVec2::new(-p.y, p.x),
            Motion::Turn(RotateDirection::CounterClockwise) => IVec2::new(p.y, -p.x),
        };
        self.cell(moved)
    }

    /// A cell's flags after the same motion, so a mirrored or turned tile is drawn so.
    pub(crate) fn moved_flip(self, flip: Flip, motion: Motion) -> Flip {
        let hexagonal = matches!(self, Lattice::Hexagonal(_));
        match motion {
            Motion::Mirror(FlipDirection::Horizontal) if hexagonal => {
                flip.flipped_hex_horizontally()
            }
            Motion::Mirror(FlipDirection::Vertical) if hexagonal => flip.flipped_hex_vertically(),
            Motion::Turn(RotateDirection::Clockwise) if hexagonal => flip.rotated_hex_cw(),
            Motion::Turn(RotateDirection::CounterClockwise) if hexagonal => flip.rotated_hex_ccw(),
            Motion::Mirror(FlipDirection::Horizontal) => flip.flipped_horizontally(),
            Motion::Mirror(FlipDirection::Vertical) => flip.flipped_vertically(),
            Motion::Turn(RotateDirection::Clockwise) => flip.rotated_cw(),
            Motion::Turn(RotateDirection::CounterClockwise) => flip.rotated_ccw(),
        }
    }

    /// The lattice step that moves cell `from` onto cell `to`.
    pub(crate) fn step(self, from: IVec2, to: IVec2) -> IVec2 {
        self.point(to) - self.point(from)
    }

    /// A cell moved by a lattice step. On staggered and hexagonal maps a step across an odd
    /// number of rows (columns) moves the rows of each parity by a different number of cells,
    /// which is what keeps a shape's form.
    pub(crate) fn stepped(self, cell: IVec2, step: IVec2) -> IVec2 {
        self.cell(self.point(cell) + step)
    }

    /// The lattice step that puts the top-left of `cells` at `corner`: the topmost row at
    /// `corner.y` and then the leftmost column at `corner.x` (the leftmost column first where
    /// columns are shifted). `None` when there are no cells.
    pub(crate) fn anchoring_step(self, cells: &[IVec2], corner: IVec2) -> Option<IVec2> {
        let bounds = IRect::bounding(cells.iter().copied())?;
        let Some(stagger) = self.stagger() else {
            return Some(self.step(bounds.min(), corner));
        };
        // Whole rows (columns) first, which may move the rows of each parity differently; then
        // along them, which moves every cell alike.
        Some(match stagger.axis {
            StaggerAxis::Y => {
                let rows = self.step(IVec2::new(0, bounds.y), IVec2::new(0, corner.y));
                let left = cells.iter().map(|&c| self.stepped(c, rows).x).min()?;
                rows + self.step(IVec2::new(left, corner.y), corner)
            }
            StaggerAxis::X => {
                let columns = self.step(IVec2::new(bounds.x, 0), IVec2::new(corner.x, 0));
                let top = cells.iter().map(|&c| self.stepped(c, columns).y).min()?;
                columns + self.step(IVec2::new(corner.x, top), corner)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec2;

    use super::*;
    use crate::testutil::every_grid;

    fn cells() -> impl Iterator<Item = IVec2> {
        IRect {
            x: -5,
            y: -4,
            w: 11,
            h: 9,
        }
        .points()
    }

    #[test]
    fn points_and_cells_convert_both_ways_and_motions_undo() {
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            for cell in cells() {
                assert_eq!(lattice.cell(lattice.point(cell)), cell, "{geometry:?}");
                for (motion, back) in [
                    (
                        Motion::Mirror(FlipDirection::Horizontal),
                        Motion::Mirror(FlipDirection::Horizontal),
                    ),
                    (
                        Motion::Mirror(FlipDirection::Vertical),
                        Motion::Mirror(FlipDirection::Vertical),
                    ),
                    (
                        Motion::Turn(RotateDirection::Clockwise),
                        Motion::Turn(RotateDirection::CounterClockwise),
                    ),
                ] {
                    let there = lattice.moved(cell, motion);
                    assert_eq!(
                        lattice.moved(there, back),
                        cell,
                        "{motion:?} on {geometry:?}"
                    );
                }
            }
        }
    }

    /// Where a cell is drawn: the top-left of its box, which moves with its centre.
    fn screen(geometry: &MapGeometry, cell: IVec2) -> DVec2 {
        geometry.tile_to_screen(cell.as_dvec2())
    }

    #[test]
    fn mirrors_are_mirrors_on_screen() {
        for geometry in every_grid() {
            if geometry.orientation == Orientation::Oblique {
                // A sheared grid has no screen mirror; it is mirrored as the grid.
                continue;
            }
            let lattice = Lattice::of(&geometry);
            let base = IVec2::new(1, 2);
            for cell in cells() {
                let before = screen(&geometry, cell) - screen(&geometry, base);
                for (direction, expected) in [
                    (FlipDirection::Horizontal, DVec2::new(-before.x, before.y)),
                    (FlipDirection::Vertical, DVec2::new(before.x, -before.y)),
                ] {
                    let motion = Motion::Mirror(direction);
                    let after = screen(&geometry, lattice.moved(cell, motion))
                        - screen(&geometry, lattice.moved(base, motion));
                    assert_eq!(after, expected, "{direction:?} {cell} on {geometry:?}");
                }
            }
        }
    }

    #[test]
    fn turns_go_the_way_they_say_on_screen() {
        // A square grid turns exactly; a hexagonal one by 60° within the rounding of its
        // drawing, so the angle is checked with a tolerance.
        let square = MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::ONE);
        let lattice = Lattice::of(&square);
        let turned = lattice.moved(IVec2::new(1, 0), Motion::Turn(RotateDirection::Clockwise));
        assert_eq!(turned, IVec2::new(0, 1), "right turns to down, clockwise");
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            // Nearly regular hexagons: side 16, across the flats 28.
            let tile = match axis {
                StaggerAxis::Y => IVec2::new(28, 32),
                StaggerAxis::X => IVec2::new(32, 28),
            };
            let geometry = MapGeometry {
                stagger_axis: axis,
                hex_side_length: 16,
                ..MapGeometry::new(Orientation::Hexagonal, tile, IVec2::ONE)
            };
            let lattice = Lattice::of(&geometry);
            for cell in [IVec2::new(1, 0), IVec2::new(0, 1), IVec2::new(-2, 3)] {
                let before = screen(&geometry, cell) - screen(&geometry, IVec2::ZERO);
                let motion = Motion::Turn(RotateDirection::Clockwise);
                let after = screen(&geometry, lattice.moved(cell, motion))
                    - screen(&geometry, lattice.moved(IVec2::ZERO, motion));
                let turn = (after.y.atan2(after.x) - before.y.atan2(before.x)).to_degrees();
                let turn = turn.rem_euclid(360.0);
                assert!((turn - 60.0).abs() < 4.0, "{turn}° for {cell} on {axis:?}");
            }
        }
    }

    #[test]
    fn steps_keep_shapes_and_anchor_them() {
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            let shape = [IVec2::new(3, 5), IVec2::new(4, 6), IVec2::new(2, 7)];
            let corner = IVec2::new(-1, 0);
            let step = lattice.anchoring_step(&shape, corner).unwrap();
            let moved: Vec<IVec2> = shape.iter().map(|&c| lattice.stepped(c, step)).collect();
            let bounds = IRect::bounding(moved.iter().copied()).unwrap();
            assert_eq!(bounds.min(), corner, "{geometry:?}");
            // Moved cells stand to each other as before: the steps between them are equal.
            for (a, b) in shape.iter().zip(&moved) {
                for (c, d) in shape.iter().zip(&moved) {
                    assert_eq!(lattice.step(*a, *c), lattice.step(*b, *d), "{geometry:?}");
                }
            }
        }
        assert_eq!(Lattice::Square.anchoring_step(&[], IVec2::ZERO), None);
    }
}
