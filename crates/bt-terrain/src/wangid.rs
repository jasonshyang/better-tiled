//! Lane arithmetic on packed Wang ids, as Tiled's `WangId` keeps them
//! (`reference/tiled/src/libtiled/wangset.h`, `wangset.cpp`): eight lanes of eight bits in a
//! `u64`, lane `i` in bits `8i..8i + 8`, lanes clockwise from the top (`bt_core::Lane`'s order).
//! In what the solver wants of a cell, a lane of [`UNSET`] means "no wish".

use bt_core::{Lane, WangId, WangSetType};

/// A lane value that is no colour: "no wish" in a wanted id, "constrained" in a mask.
pub(crate) const UNSET: u8 = 0xFF;

/// How many lanes a tile has.
pub(crate) const LANE_COUNT: usize = 8;

/// The edge lanes, then the corner lanes, in Tiled's order of each.
pub(crate) const EDGE_LANES: [usize; 4] = [0, 2, 4, 6];
pub(crate) const CORNER_LANES: [usize; 4] = [1, 3, 5, 7];

/// Eight lanes packed in a `u64`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PackedWangId(pub(crate) u64);

impl PackedWangId {
    /// Every lane [`UNSET`]: no wish anywhere, or a mask over every lane.
    pub(crate) const FULL: PackedWangId = PackedWangId(u64::MAX);
    /// Every lane 0.
    pub(crate) const ZERO: PackedWangId = PackedWangId(0);

    /// The packed form of bt-core's lanes.
    pub(crate) fn from_wang_id(id: WangId) -> PackedWangId {
        PackedWangId::from_lanes(id.lanes())
    }

    /// The lanes as bt-core keeps them.
    pub(crate) fn to_wang_id(self) -> WangId {
        WangId::from_lanes(self.lanes())
    }

    pub(crate) fn from_lanes(lanes: [u8; LANE_COUNT]) -> PackedWangId {
        PackedWangId(u64::from_le_bytes(lanes))
    }

    pub(crate) fn lanes(self) -> [u8; LANE_COUNT] {
        self.0.to_le_bytes()
    }

    /// Lane `lane`'s value; lanes past the eighth read as 0.
    pub(crate) fn lane(self, lane: usize) -> u8 {
        self.lanes().get(lane).copied().unwrap_or(0)
    }

    /// Sets lane `lane` to `value`; lanes past the eighth are left alone.
    pub(crate) fn set_lane(&mut self, lane: usize, value: u8) {
        if lane < LANE_COUNT {
            let shift = lane * 8;
            self.0 = (self.0 & !(0xFF_u64 << shift)) | (u64::from(value) << shift);
        }
    }

    /// This id with lane `lane` set to `value`.
    pub(crate) fn with_lane(mut self, lane: usize, value: u8) -> PackedWangId {
        self.set_lane(lane, value);
        self
    }

    /// The lanes of `self` where `mask` is set, 0 elsewhere.
    pub(crate) fn masked(self, mask: PackedWangId) -> PackedWangId {
        PackedWangId(self.0 & mask.0)
    }

    /// `self` with the lanes `mask` covers taken from `other` (Tiled's `mergeWith`).
    pub(crate) fn merged_with(self, other: PackedWangId, mask: PackedWangId) -> PackedWangId {
        PackedWangId((self.0 & !mask.0) | (other.0 & mask.0))
    }

    /// The lanes `self` and `other` cover together.
    pub(crate) fn union(self, other: PackedWangId) -> PackedWangId {
        PackedWangId(self.0 | other.0)
    }

    /// The lanes `self` does not cover.
    pub(crate) fn inverted(self) -> PackedWangId {
        PackedWangId(!self.0)
    }

    /// A mask over the lanes whose value is `value` (Tiled's `mask(int)`).
    pub(crate) fn mask_of_value(self, value: u8) -> PackedWangId {
        let mut mask = PackedWangId::ZERO;
        for (lane, &v) in self.lanes().iter().enumerate() {
            if v == value {
                mask.set_lane(lane, UNSET);
            }
        }
        mask
    }

    /// Whether some lane is 0 (Tiled's `hasWildCards`).
    pub(crate) fn has_zero_lane(self) -> bool {
        self.lanes().contains(&0)
    }

    /// Turned clockwise by `quarters` quarter turns: the top lane becomes the right lane.
    pub(crate) fn rotated(self, quarters: u32) -> PackedWangId {
        PackedWangId(self.0.rotate_left((quarters % 4) * 16))
    }

    /// Mirrored left-right (Tiled's `flippedHorizontally`): right and left edges swap, and each
    /// corner swaps with the one across the vertical axis.
    pub(crate) fn flipped_horizontally(self) -> PackedWangId {
        let l = self.lanes();
        PackedWangId::from_lanes([l[0], l[7], l[6], l[5], l[4], l[3], l[2], l[1]])
    }

    /// Mirrored top-bottom (Tiled's `flippedVertically`: a left-right mirror, then a half turn).
    pub(crate) fn flipped_vertically(self) -> PackedWangId {
        self.flipped_horizontally().rotated(2)
    }
}

/// The lane across from `lane`.
pub(crate) fn opposite(lane: usize) -> usize {
    (lane + 4) % LANE_COUNT
}

/// The lane after `lane`, clockwise.
pub(crate) fn next(lane: usize) -> usize {
    (lane + 1) % LANE_COUNT
}

/// The lane before `lane`, clockwise.
pub(crate) fn previous(lane: usize) -> usize {
    (lane + LANE_COUNT - 1) % LANE_COUNT
}

/// Whether `lane` is a corner.
pub(crate) fn is_corner(lane: usize) -> bool {
    lane % 2 == 1
}

/// The lanes a set of `kind` uses, as a mask (Tiled's `typeMask`).
pub(crate) fn kind_mask(kind: WangSetType) -> PackedWangId {
    let mut mask = PackedWangId::ZERO;
    let lanes: &[usize] = match kind {
        WangSetType::Corner => &CORNER_LANES,
        WangSetType::Edge => &EDGE_LANES,
        WangSetType::Mixed => &[0, 1, 2, 3, 4, 5, 6, 7],
    };
    for &lane in lanes {
        mask.set_lane(lane, UNSET);
    }
    mask
}

/// The lanes of one side of a tile, with its two corners (Tiled's `MaskTopSide` and the rest).
pub(crate) fn side_mask(side: usize) -> PackedWangId {
    PackedWangId::ZERO
        .with_lane(previous(side), UNSET)
        .with_lane(side, UNSET)
        .with_lane(next(side), UNSET)
}

/// bt-core's lane for an index, which is always one of the eight.
pub(crate) fn lane_of(index: usize) -> Lane {
    match index % LANE_COUNT {
        0 => Lane::Top,
        1 => Lane::TopRight,
        2 => Lane::Right,
        3 => Lane::BottomRight,
        4 => Lane::Bottom,
        5 => Lane::BottomLeft,
        6 => Lane::Left,
        _ => Lane::TopLeft,
    }
}

/// The lane at column `x` and row `y` of a tile's 3×3 grid; the centre has none (Tiled's
/// `indexByGrid`).
pub(crate) fn lane_by_grid(x: i32, y: i32) -> Option<usize> {
    match (x, y) {
        (0, 0) => Some(7),
        (1, 0) => Some(0),
        (2, 0) => Some(1),
        (0, 1) => Some(6),
        (2, 1) => Some(2),
        (0, 2) => Some(5),
        (1, 2) => Some(4),
        (2, 2) => Some(3),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(lanes: [u8; 8]) -> PackedWangId {
        PackedWangId::from_lanes(lanes)
    }

    #[test]
    fn lanes_pack_as_tileds_bits() {
        let packed = id([1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(packed.0, 0x0807_0605_0403_0201);
        assert_eq!(packed.lane(2), 3);
        assert_eq!(packed.with_lane(2, 9).lane(2), 9);
        assert_eq!(packed.lane(8), 0, "past the eighth lane");
        assert_eq!(
            PackedWangId::from_wang_id(packed.to_wang_id()),
            packed,
            "bt-core's lanes round-trip"
        );
    }

    #[test]
    fn turns_and_mirrors_follow_tiled() {
        let top_and_top_right = id([1, 2, 0, 0, 0, 0, 0, 0]);
        // A quarter clockwise: the top edge becomes the right edge, the top-right corner the
        // bottom-right one.
        assert_eq!(top_and_top_right.rotated(1), id([0, 0, 1, 2, 0, 0, 0, 0]));
        assert_eq!(top_and_top_right.rotated(4), top_and_top_right);
        let corners = id([0, 1, 0, 2, 0, 3, 0, 4]);
        assert_eq!(corners.flipped_horizontally(), id([0, 4, 0, 3, 0, 2, 0, 1]));
        let edges = id([1, 0, 2, 0, 3, 0, 4, 0]);
        assert_eq!(edges.flipped_horizontally(), id([1, 0, 4, 0, 3, 0, 2, 0]));
        assert_eq!(edges.flipped_vertically(), id([3, 0, 2, 0, 1, 0, 4, 0]));
        assert_eq!(corners.flipped_vertically(), id([0, 2, 0, 1, 0, 4, 0, 3]));
    }

    #[test]
    fn masks_and_merges() {
        assert_eq!(kind_mask(WangSetType::Corner).0, 0xFF00_FF00_FF00_FF00);
        assert_eq!(kind_mask(WangSetType::Edge).0, 0x00FF_00FF_00FF_00FF);
        assert_eq!(kind_mask(WangSetType::Mixed), PackedWangId::FULL);
        assert_eq!(side_mask(0).lanes(), [UNSET, UNSET, 0, 0, 0, 0, 0, UNSET]);
        let wanted = id([1, UNSET, 3, UNSET, 5, 6, 7, 8]);
        assert_eq!(
            wanted.mask_of_value(UNSET).lanes(),
            [0, UNSET, 0, UNSET, 0, 0, 0, 0]
        );
        let merged = id([1; 8]).merged_with(id([2; 8]), side_mask(4));
        assert_eq!(merged.lanes(), [1, 1, 1, 2, 2, 2, 1, 1]);
        assert!(id([1, 1, 1, 1, 1, 1, 1, 0]).has_zero_lane());
        assert!(!id([1; 8]).has_zero_lane());
    }

    #[test]
    fn lane_neighbours_and_the_grid() {
        assert_eq!((opposite(1), next(7), previous(0)), (5, 0, 7));
        assert!(is_corner(3) && !is_corner(4));
        assert_eq!(lane_by_grid(0, 0), Some(7));
        assert_eq!(lane_by_grid(1, 1), None);
        assert_eq!(lane_of(5), Lane::BottomLeft);
        assert_eq!(lane_of(5).index(), 5);
    }
}
