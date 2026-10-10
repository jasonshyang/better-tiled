//! Where carried tiles are, from where they were lifted: one exact affine map of doubled lattice
//! coordinates (`Lattice::coordinates`, twice over), made of a mirror, turns and a shift.
//! Doubled coordinates name the centres of cells and the points halfway between them, so a
//! mirror or a turn about the centre of any selection is exact, and four quarter turns (six
//! sixth turns on hexagonal maps) or two mirrors about one point give back exactly what was
//! lifted. A cell lands where the map takes its centre; where that is halfway between cells (a
//! quarter turn of a block with an odd and an even side), every cell goes half a step down the
//! lattice's axes alike, which keeps the shape.

use bt_core::Flip;
use glam::IVec2;

use crate::axis::Axis;
use crate::lattice::{Lattice, Linear, Motion, RotateDirection};

/// A left-right mirror, which every mirror of a placement is made of.
const MIRROR: Motion = Motion::Mirror(Axis::Horizontal);

/// A clockwise turn, which every turn of a placement is made of.
const TURN: Motion = Motion::Turn(RotateDirection::Clockwise);

/// A point that carried tiles are mirrored or turned about: the centre of a cell or a point
/// halfway between cells, on the lattice of the map the tiles came from. Its coordinates are
/// doubled, so the centre of any selection is one exactly; [`crate::FloatingTiles::centre`] is
/// the centre of a carried selection.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TilePivot {
    /// Twice the point's lattice coordinates.
    pub(crate) doubled: IVec2,
}

/// Where carried tiles are, from where they were lifted: mirrored left-right on screen or not,
/// then turned clockwise a number of times, then shifted, as one affine map of doubled lattice
/// coordinates. Any sequence of mirrors and turns is one of these: a top-bottom mirror is a
/// left-right one and a half turn, and a mirror after turns is the mirror before as many turns
/// the other way.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Placement {
    /// Mirrored left-right first.
    mirrored: bool,
    /// Then turned clockwise this many times, a quarter turn each or a sixth on hexagonal
    /// lattices; fewer than make a whole turn.
    turns: u8,
    /// Then shifted by this, in doubled lattice coordinates.
    shift: IVec2,
}

impl Placement {
    /// A mirror or a turn about the lattice's origin.
    fn of_motion(lattice: Lattice, motion: Motion) -> Placement {
        let circle = lattice.turns_in_circle();
        match motion {
            Motion::Mirror(Axis::Horizontal) => Placement {
                mirrored: true,
                ..Placement::default()
            },
            Motion::Mirror(Axis::Vertical) => Placement {
                mirrored: true,
                turns: circle / 2,
                ..Placement::default()
            },
            Motion::Turn(RotateDirection::Clockwise) => Placement {
                turns: 1,
                ..Placement::default()
            },
            Motion::Turn(RotateDirection::CounterClockwise) => Placement {
                turns: circle - 1,
                ..Placement::default()
            },
        }
    }

    /// The mirror and the turns, as a linear map of lattice coordinates.
    fn linear(self, lattice: Lattice) -> Linear {
        let mirror = if self.mirrored {
            lattice.linear(MIRROR)
        } else {
            Linear::IDENTITY
        };
        let turn = lattice.linear(TURN);
        (0..self.turns).fold(mirror, |linear, _| turn.after(linear))
    }

    /// This placement, then a mirror or a turn about `pivot`.
    pub(crate) fn then_moved(
        self,
        lattice: Lattice,
        motion: Motion,
        pivot: TilePivot,
    ) -> Placement {
        let circle = lattice.turns_in_circle();
        let motion = Placement::of_motion(lattice, motion);
        let turns_before = if motion.mirrored {
            (circle - self.turns) % circle
        } else {
            self.turns
        };
        let around = pivot.doubled;
        Placement {
            mirrored: motion.mirrored != self.mirrored,
            turns: (motion.turns + turns_before) % circle,
            shift: motion
                .linear(lattice)
                .apply(self.shift.saturating_sub(around))
                .saturating_add(around),
        }
    }

    /// This placement, then a step of whole cells, in lattice coordinates.
    pub(crate) fn then_stepped(self, step: IVec2) -> Placement {
        Placement {
            shift: self
                .shift
                .saturating_add(step.saturating_mul(IVec2::splat(2))),
            ..self
        }
    }

    /// Where a point of the lattice as lifted is now.
    pub(crate) fn pivot(self, lattice: Lattice, pivot: TilePivot) -> TilePivot {
        TilePivot {
            doubled: self
                .linear(lattice)
                .apply(pivot.doubled)
                .saturating_add(self.shift),
        }
    }

    /// The shift in whole cells: halfway is rounded down.
    fn cell_shift(self) -> IVec2 {
        self.shift.div_euclid(IVec2::splat(2))
    }

    /// Where each lifted cell is now, worked out once for many cells.
    pub(crate) fn cells(self, lattice: Lattice) -> impl Fn(IVec2) -> IVec2 + use<> {
        let linear = self.linear(lattice);
        let shift = self.cell_shift();
        move |cell| {
            lattice.cell_at(
                linear
                    .apply(lattice.coordinates(cell))
                    .saturating_add(shift),
            )
        }
    }

    /// How each lifted tile is drawn now: its flags after the mirror and then the turns, changed
    /// as a brush's are (`Lattice::moved_flip`), which draws as the mirrors and turns one after
    /// the other did. A tile that was neither mirrored nor turned keeps its flags as they were.
    pub(crate) fn flips(self, lattice: Lattice) -> impl Fn(Flip) -> Flip + use<> {
        move |flip| {
            let mirrored = if self.mirrored {
                lattice.moved_flip(flip, MIRROR)
            } else {
                flip
            };
            (0..self.turns).fold(mirrored, |flip, _| lattice.moved_flip(flip, TURN))
        }
    }

    /// Whether every lifted cell is where it was and every tile as it was.
    pub(crate) fn at_rest(self) -> bool {
        !self.mirrored && self.turns == 0 && self.cell_shift() == IVec2::ZERO
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Fuzz;
    use bt_utils::rng::Rng;

    use super::*;
    use crate::testutil::every_grid;

    /// Every mirror and turn.
    const MOTIONS: [Motion; 4] = [
        Motion::Mirror(Axis::Horizontal),
        Motion::Mirror(Axis::Vertical),
        Motion::Turn(RotateDirection::Clockwise),
        Motion::Turn(RotateDirection::CounterClockwise),
    ];

    /// The motion that undoes `motion`.
    fn undoing(motion: Motion) -> Motion {
        match motion {
            Motion::Mirror(_) => motion,
            Motion::Turn(RotateDirection::Clockwise) => {
                Motion::Turn(RotateDirection::CounterClockwise)
            }
            Motion::Turn(RotateDirection::CounterClockwise) => {
                Motion::Turn(RotateDirection::Clockwise)
            }
        }
    }

    /// Flags in the form the hexagonal tables give (never a 60° and a 120° turn together) on
    /// hexagonal lattices, any flags on the others.
    fn usual_flips(lattice: Lattice) -> Vec<Flip> {
        (0..16u8)
            .map(Flip::from_bits_truncate)
            .filter(|flip| {
                !matches!(lattice, Lattice::Hexagonal(_)) || !flip.contains(Flip::D | Flip::HEX120)
            })
            .collect()
    }

    /// Cells around the origin.
    fn cells() -> impl Iterator<Item = IVec2> {
        bt_utils::IRect {
            x: -4,
            y: -3,
            w: 9,
            h: 7,
        }
        .points()
    }

    #[test]
    fn a_motion_about_a_cell_moves_cells_and_flags_as_the_lattice_does() {
        // Turning about a cell's centre is the lattice's motion with that cell held still.
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            for centre in [IVec2::ZERO, IVec2::new(3, -2), IVec2::new(-1, 5)] {
                let pivot = TilePivot {
                    doubled: lattice.coordinates(centre) * 2,
                };
                for motion in MOTIONS {
                    let placement = Placement::default().then_moved(lattice, motion, pivot);
                    let now = placement.cells(lattice);
                    let back = lattice.step(lattice.moved(centre, motion), centre);
                    for cell in cells() {
                        let expected = lattice.stepped(lattice.moved(cell, motion), back);
                        assert_eq!(now(cell), expected, "{motion:?} of {cell} on {geometry:?}");
                    }
                    assert_eq!(now(centre), centre);
                    assert_eq!(placement.pivot(lattice, pivot), pivot);
                    let drawn = placement.flips(lattice);
                    for flip in usual_flips(lattice) {
                        assert_eq!(drawn(flip), lattice.moved_flip(flip, motion), "{motion:?}");
                    }
                }
            }
        }
    }

    /// A motion made, and the point it was made about.
    struct Made {
        motion: Motion,
        pivot: TilePivot,
    }

    #[test]
    fn sequences_of_motions_compose_into_one_placement() {
        // Any sequence about any cells gives the cells and flags the motions give one after
        // the other; undone in reverse it is exactly where it started.
        let fuzz = Fuzz::from_env(0x7_10A7, 40);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            for geometry in every_grid() {
                let lattice = Lattice::of(&geometry);
                let mut placement = Placement::default();
                let mut made = Vec::new();
                let mut cells_now: Vec<IVec2> = cells().collect();
                let mut flips_now = usual_flips(lattice);
                for _ in 0..rng.below(7).unwrap() {
                    let motion = MOTIONS[rng.below(4).unwrap() as usize];
                    let centre = IVec2::new(
                        rng.below(9).unwrap() as i32 - 4,
                        rng.below(9).unwrap() as i32 - 4,
                    );
                    let pivot = TilePivot {
                        doubled: lattice.coordinates(centre) * 2,
                    };
                    placement = placement.then_moved(lattice, motion, pivot);
                    let back = lattice.step(lattice.moved(centre, motion), centre);
                    for cell in &mut cells_now {
                        *cell = lattice.stepped(lattice.moved(*cell, motion), back);
                    }
                    for flip in &mut flips_now {
                        *flip = lattice.moved_flip(*flip, motion);
                    }
                    made.push(Made { motion, pivot });
                    let context = || format!("case {case} ({}) on {geometry:?}", fuzz.reproduce());
                    let now = placement.cells(lattice);
                    let moved: Vec<IVec2> = cells().map(&now).collect();
                    assert_eq!(moved, cells_now, "{}", context());
                    let drawn = placement.flips(lattice);
                    let flipped: Vec<Flip> = usual_flips(lattice).into_iter().map(&drawn).collect();
                    assert_eq!(flipped, flips_now, "{}", context());
                }
                for Made { motion, pivot } in made.into_iter().rev() {
                    placement = placement.then_moved(lattice, undoing(motion), pivot);
                }
                assert_eq!(placement, Placement::default(), "{geometry:?}");
            }
        }
    }

    #[test]
    fn whole_turns_and_double_mirrors_about_any_point_are_the_identity() {
        for geometry in every_grid() {
            let lattice = Lattice::of(&geometry);
            let start = Placement::default().then_stepped(IVec2::new(2, -3));
            // Centres of cells, points halfway between them, and points between four of them.
            for doubled in [IVec2::new(0, 0), IVec2::new(3, 2), IVec2::new(-1, -5)] {
                let pivot = TilePivot { doubled };
                for direction in [
                    RotateDirection::Clockwise,
                    RotateDirection::CounterClockwise,
                ] {
                    let mut turned = start;
                    for count in 1..=lattice.turns_in_circle() {
                        turned = turned.then_moved(lattice, Motion::Turn(direction), pivot);
                        assert_eq!(
                            turned == start,
                            count == lattice.turns_in_circle(),
                            "{count} turns on {geometry:?}"
                        );
                    }
                }
                for direction in [Axis::Horizontal, Axis::Vertical] {
                    let once = start.then_moved(lattice, Motion::Mirror(direction), pivot);
                    assert_ne!(once, start);
                    assert_eq!(
                        once.then_moved(lattice, Motion::Mirror(direction), pivot),
                        start
                    );
                }
            }
        }
    }

    #[test]
    fn a_turn_halfway_between_cells_moves_every_cell_alike() {
        // A 3 × 2 block turned about its middle: its cells land half a cell up and left of
        // where the turn takes their centres, all alike; turned twice it is back in place.
        let lattice = Lattice::Square;
        let block = bt_utils::IRect {
            x: 0,
            y: 0,
            w: 3,
            h: 2,
        };
        let middle = TilePivot {
            doubled: IVec2::new(2, 1),
        };
        let quarter = Placement::default().then_moved(lattice, TURN, middle);
        let turned: Vec<IVec2> = block.points().map(quarter.cells(lattice)).collect();
        let expected: Vec<IVec2> = block
            .points()
            .map(|p| IVec2::new(1 - p.y, p.x - 1))
            .collect();
        assert_eq!(turned, expected);
        let half = quarter.then_moved(lattice, TURN, quarter.pivot(lattice, middle));
        let back: Vec<IVec2> = block.points().map(half.cells(lattice)).collect();
        let expected: Vec<IVec2> = block
            .points()
            .map(|p| IVec2::new(2 - p.x, 1 - p.y))
            .collect();
        assert_eq!(back, expected, "the half turn keeps the block where it was");
        assert!(!quarter.at_rest() && !half.at_rest());
        assert!(Placement::default().then_stepped(IVec2::ZERO).at_rest());
    }

    #[test]
    fn at_rest_means_every_cell_and_tile_as_lifted() {
        let lattice = Lattice::Square;
        let pivot = TilePivot {
            doubled: IVec2::new(1, 0),
        };
        // A turn about the point halfway between cells (0, 0) and (1, 0), and back about the
        // centre of (1, 0), leaves half a cell of shift on each axis, which rounds to nothing:
        // the cells are back.
        let there = Placement::default().then_moved(lattice, TURN, pivot);
        let back = there.then_moved(
            lattice,
            Motion::Turn(RotateDirection::CounterClockwise),
            TilePivot {
                doubled: IVec2::new(2, 0),
            },
        );
        assert_ne!(back, Placement::default());
        let now = back.cells(lattice);
        assert!(cells().all(|cell| now(cell) == cell));
        assert!(back.at_rest());
        assert!(!Placement::default().then_stepped(IVec2::X).at_rest());
        assert!(!there.at_rest());
    }
}
