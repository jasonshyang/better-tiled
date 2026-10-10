//! Why a cell no tile fits stayed as it was: the lanes it needed and what forced them.

use bt_core::Lane;
use glam::IVec2;

use crate::wangid::{LANE_COUNT, PackedWangId, UNSET};

/// What a cell no tile fits needed: a pattern of lanes no tile of the set has.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Needed {
    /// The cell.
    pub cell: IVec2,
    /// The terrain each lane of the set's kind needed; `None` where anything would do.
    pub lanes: [Option<u8>; LANE_COUNT],
    /// What forced the need.
    pub cause: Cause,
}

/// What made a cell need what no tile has.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cause {
    /// No tile has the lanes the cell must have: what was painted, what placed neighbours share
    /// with it, and (for the brush) lanes whose wanted terrain no tile of the cell's could reach.
    NoTile,
    /// Every best tile would have left the neighbour across this lane with no tile at all; the
    /// lanes are what that neighbour would have needed.
    Neighbour(Lane),
    /// The best tile has probability zero, so it is never placed; the lanes are its own.
    ZeroProbability,
}

/// The lanes a mask constrains, with the wanted terrain of each, within `kind_mask`.
pub(crate) fn pattern_of(
    wanted: PackedWangId,
    mask: PackedWangId,
    kind_mask: PackedWangId,
) -> [Option<u8>; LANE_COUNT] {
    let mask = mask.masked(kind_mask).lanes();
    let wanted = wanted.lanes();
    std::array::from_fn(|lane| {
        let constrained = mask.get(lane).is_some_and(|&m| m == UNSET);
        constrained.then(|| wanted.get(lane).copied().unwrap_or(0))
    })
}
