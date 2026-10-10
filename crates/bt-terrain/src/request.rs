//! What a terrain paint or fill asks for, and what comes back.

use bt_core::{Lane, LayerId, Map, PlacedCell};
use bt_utils::IRect;
use glam::IVec2;

use crate::color::TerrainColor;
use crate::diagnose::Needed;
use crate::terrain::Terrain;
use crate::topology::Topology;

/// What the brush touched.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BrushTarget {
    /// The vertex at the top-left corner of this cell: the four corners that meet there.
    Corner(IVec2),
    /// One edge of a cell, and so the edge of the cell across it.
    Edge {
        /// The cell.
        cell: IVec2,
        /// Which of its edges.
        side: Lane,
    },
    /// The whole tile (Tiled's "fill full tiles"): its lanes and those of its neighbours that
    /// touch it.
    Tile(IVec2),
}

impl BrushTarget {
    /// The point painting happens around: the vertex's cell, or the cell.
    pub fn point(self) -> IVec2 {
        match self {
            BrushTarget::Corner(cell)
            | BrushTarget::Edge { cell, .. }
            | BrushTarget::Tile(cell) => cell,
        }
    }

    /// The cells the target constrains, for outlines: the four around a vertex, the two either
    /// side of an edge, the tile.
    pub fn cells(self, map: &Map) -> Vec<IVec2> {
        let topology = Topology::of(map);
        match self {
            BrushTarget::Corner(vertex) => topology
                .around_vertex(vertex)
                .iter()
                .map(|at| at.cell)
                .collect(),
            BrushTarget::Edge { cell, side } => std::iter::once(cell)
                .chain(topology.across_edge(cell, side.index()))
                .collect(),
            BrushTarget::Tile(cell) => vec![cell],
        }
    }
}

/// A terrain brush stroke, or part of one: the targets painted with one terrain, the neighbours
/// whose tiles no longer fit re-solved out to the set's maximum terrain distance.
#[derive(Clone, Debug)]
pub struct PaintRequest<'t> {
    pub(crate) layer: LayerId,
    pub(crate) terrain: &'t Terrain,
    pub(crate) color: TerrainColor,
    pub(crate) targets: Vec<BrushTarget>,
    pub(crate) seed: u64,
    pub(crate) mirror: bool,
}

impl<'t> PaintRequest<'t> {
    /// Painting `color` of `terrain` on the tile layer `layer`, with no targets yet.
    pub fn new(layer: LayerId, terrain: &'t Terrain, color: TerrainColor) -> PaintRequest<'t> {
        PaintRequest {
            layer,
            terrain,
            color,
            targets: Vec::new(),
            seed: 0,
            mirror: false,
        }
    }

    /// What the brush touched.
    pub fn targets(mut self, targets: Vec<BrushTarget>) -> PaintRequest<'t> {
        self.targets = targets;
        self
    }

    /// The seed every random choice is derived from, with each cell's position.
    pub fn seed(mut self, seed: u64) -> PaintRequest<'t> {
        self.seed = seed;
        self
    }

    /// Whether each target's 180° turn around the map's centre is painted too (finite maps
    /// only).
    pub fn mirror(mut self, mirror: bool) -> PaintRequest<'t> {
        self.mirror = mirror;
        self
    }
}

/// A terrain fill of cells: terrain that connects to the tiles around them, preferring one
/// terrain when one is given.
#[derive(Clone, Debug)]
pub struct FillRequest<'t> {
    pub(crate) layer: LayerId,
    pub(crate) terrain: &'t Terrain,
    pub(crate) color: TerrainColor,
    pub(crate) cells: Vec<IVec2>,
    pub(crate) allow_empty: bool,
    pub(crate) seed: u64,
}

impl<'t> FillRequest<'t> {
    /// Filling with `terrain` on the tile layer `layer`, preferring `color` (none: any terrain
    /// of the set, as Tiled's fill), with no cells yet.
    pub fn new(layer: LayerId, terrain: &'t Terrain, color: TerrainColor) -> FillRequest<'t> {
        FillRequest {
            layer,
            terrain,
            color,
            cells: Vec::new(),
            allow_empty: true,
            seed: 0,
        }
    }

    /// The cells to fill.
    pub fn cells(mut self, cells: Vec<IVec2>) -> FillRequest<'t> {
        self.cells = cells;
        self
    }

    /// Whether a cell may be left empty where that fits best (Tiled's bucket and shape fills
    /// allow it; its stamp brush in terrain mode does not). On by default.
    pub fn allow_empty(mut self, allow: bool) -> FillRequest<'t> {
        self.allow_empty = allow;
        self
    }

    /// The seed every random choice is derived from, with each cell's position.
    pub fn seed(mut self, seed: u64) -> FillRequest<'t> {
        self.seed = seed;
        self
    }
}

/// What a paint or fill did to the layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// The bounds of the cells that changed; `None` when nothing did.
    pub changed: Option<IRect>,
    /// The cells no tile fits, left as they were, in row-major order.
    pub invalid: Vec<IVec2>,
    /// What each invalid cell needed, in the same order.
    pub needed: Vec<Needed>,
}

/// What a paint or fill would do, without doing it: with the same request and seed, exactly
/// what the paint or fill will place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    /// The cells that would be placed, in row-major order; [`bt_core::Cell::EMPTY`] erases.
    pub cells: Vec<PlacedCell>,
    /// The cells no tile fits, in row-major order.
    pub invalid: Vec<IVec2>,
    /// What each invalid cell needed, in the same order.
    pub needed: Vec<Needed>,
}
