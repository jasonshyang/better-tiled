//! The cells across a tile's lanes on every grid, as Tiled's `WangFiller` and `WangBrush` find
//! them (`reference/tiled/src/tiled/wangfiller.cpp`, `getSurroundingPoints`, `setCorner`,
//! `setEdge`). On staggered and hexagonal maps a terrain tile is a diamond whose top faces the
//! upper-right neighbour; Tiled's staggered renderer is its hexagonal one, so both take that path.

use bt_core::{Map, MapGeometry, StaggerAxis};
use glam::IVec2;

use crate::wangid::LANE_COUNT;

/// The offsets to the cells across each lane on square grids, clockwise from the top.
const AROUND: [IVec2; LANE_COUNT] = [
    IVec2::new(0, -1),
    IVec2::new(1, -1),
    IVec2::new(1, 0),
    IVec2::new(1, 1),
    IVec2::new(0, 1),
    IVec2::new(-1, 1),
    IVec2::new(-1, 0),
    IVec2::new(-1, -1),
];

/// A cell and one of its lanes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CellLane {
    pub(crate) cell: IVec2,
    pub(crate) lane: usize,
}

/// How a map's cells neighbour each other across lanes.
#[derive(Clone, Debug)]
pub(crate) struct Topology {
    geometry: MapGeometry,
    hex: bool,
    axis: StaggerAxis,
}

impl Topology {
    pub(crate) fn of(map: &Map) -> Topology {
        let geometry = map.geometry();
        Topology {
            hex: geometry.is_hex_grid(),
            geometry,
            axis: map.stagger_axis,
        }
    }

    /// Whether the grid is staggered or hexagonal.
    pub(crate) fn is_hex(&self) -> bool {
        self.hex
    }

    /// The cells across each lane of `p`, in lane order.
    pub(crate) fn around(&self, p: IVec2) -> [IVec2; LANE_COUNT] {
        if !self.hex {
            return AROUND.map(|offset| p + offset);
        }
        let g = &self.geometry;
        let (corner_right, corner_down) = match self.axis {
            StaggerAxis::X => (IVec2::new(2, 0), IVec2::new(0, 1)),
            StaggerAxis::Y => (IVec2::new(1, 0), IVec2::new(0, 2)),
        };
        [
            g.top_right(p),
            p + corner_right,
            g.bottom_right(p),
            p + corner_down,
            g.bottom_left(p),
            p - corner_right,
            g.top_left(p),
            p - corner_down,
        ]
    }

    /// The cell across edge `lane` of `p`, or `None` for a corner lane.
    pub(crate) fn across_edge(&self, p: IVec2, lane: usize) -> Option<IVec2> {
        if lane % 2 == 1 {
            return None;
        }
        self.around(p).get(lane).copied()
    }

    /// The four cells meeting at the vertex at the top-left of cell `vertex`, each with the
    /// corner lane that touches it, in the order Tiled's `setCorner` sets them.
    pub(crate) fn around_vertex(&self, vertex: IVec2) -> [CellLane; 4] {
        let at = |cell: IVec2, lane: usize| CellLane { cell, lane };
        if self.hex {
            let g = &self.geometry;
            let top_left = g.top_left(vertex);
            [
                at(g.top_right(vertex), 5),
                at(vertex, 7),
                at(top_left, 1),
                at(g.top_right(top_left), 3),
            ]
        } else {
            [
                at(vertex + IVec2::new(0, -1), 5),
                at(vertex, 7),
                at(vertex + IVec2::new(-1, 0), 1),
                at(vertex + IVec2::new(-1, -1), 3),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Orientation, StaggerIndex};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    #[test]
    fn square_grids_neighbour_by_offsets() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .build()
            .unwrap();
        let topology = Topology::of(&map);
        let p = IVec2::new(1, 1);
        assert_eq!(topology.around(p)[0], IVec2::new(1, 0));
        assert_eq!(topology.around(p)[3], IVec2::new(2, 2));
        assert_eq!(topology.across_edge(p, 6), Some(IVec2::new(0, 1)));
        assert_eq!(topology.across_edge(p, 1), None);
        let vertex = topology.around_vertex(p);
        assert_eq!(
            vertex[0],
            CellLane {
                cell: IVec2::new(1, 0),
                lane: 5
            }
        );
        assert_eq!(
            vertex[3],
            CellLane {
                cell: IVec2::new(0, 0),
                lane: 3
            }
        );
    }

    #[test]
    fn staggered_grids_neighbour_across_the_diamond() {
        let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(4))
            .orientation(Orientation::Staggered)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd)
            .build()
            .unwrap();
        let topology = Topology::of(&map);
        assert!(topology.is_hex());
        // Row 1 is shifted right: its upper-right neighbour is in the next column.
        let around = topology.around(IVec2::new(1, 1));
        assert_eq!(around[0], IVec2::new(2, 0), "top: the upper-right diamond");
        assert_eq!(
            around[1],
            IVec2::new(2, 1),
            "top-right corner: the next in the row"
        );
        assert_eq!(
            around[3],
            IVec2::new(1, 3),
            "bottom-right corner: two rows down"
        );
        assert_eq!(around[6], IVec2::new(1, 0), "left: the upper-left diamond");
    }
}
