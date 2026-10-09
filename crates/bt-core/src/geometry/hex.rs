//! Cube coordinates for hexagonal grids (libtiled's `hex.cpp`), for turning stamps by 60° and
//! for finding neighbours.

use std::ops::{Add, Sub};

use glam::IVec2;

use crate::map::{StaggerAxis, StaggerIndex};

/// A hexagon's position in cube coordinates, where `x + y + z == 0`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hex {
    /// The x axis.
    pub x: i32,
    /// The y axis.
    pub y: i32,
    /// The z axis.
    pub z: i32,
}

impl Hex {
    /// The hexagon at these cube coordinates.
    pub const fn new(x: i32, y: i32, z: i32) -> Hex {
        Hex { x, y, z }
    }

    /// The hexagon at a staggered map's tile position.
    pub fn from_offset(tile: IVec2, axis: StaggerAxis, index: StaggerIndex) -> Hex {
        let (col, row) = (tile.x, tile.y);
        let even = index == StaggerIndex::Even;
        let (x, z) = match axis {
            StaggerAxis::X if even => (col, row - (col + (col & 1)) / 2),
            StaggerAxis::X => (col, row - (col - (col & 1)) / 2),
            StaggerAxis::Y if even => (col - (row + (row & 1)) / 2, row),
            StaggerAxis::Y => (col - (row - (row & 1)) / 2, row),
        };
        Hex::new(x, -x - z, z)
    }

    /// The tile position of the hexagon on a staggered map.
    pub fn to_offset(self, axis: StaggerAxis, index: StaggerIndex) -> IVec2 {
        let even = index == StaggerIndex::Even;
        let (x, z) = (self.x, self.z);
        match axis {
            StaggerAxis::X if even => IVec2::new(x, z + (x + (x & 1)) / 2),
            StaggerAxis::X => IVec2::new(x, z + (x - (x & 1)) / 2),
            StaggerAxis::Y if even => IVec2::new(x + (z + (z & 1)) / 2, z),
            StaggerAxis::Y => IVec2::new(x + (z - (z & 1)) / 2, z),
        }
    }

    /// The hexagon turned 60° clockwise on screen around the origin.
    pub fn rotated_cw(self) -> Hex {
        Hex::new(-self.z, -self.x, -self.y)
    }

    /// The hexagon turned 60° counter-clockwise on screen around the origin.
    pub fn rotated_ccw(self) -> Hex {
        Hex::new(-self.y, -self.z, -self.x)
    }
}

impl Add for Hex {
    type Output = Hex;

    fn add(self, other: Hex) -> Hex {
        Hex::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Hex {
    type Output = Hex;

    fn sub(self, other: Hex) -> Hex {
        Hex::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}
