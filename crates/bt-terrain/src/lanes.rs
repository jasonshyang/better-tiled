//! The lanes each kind of terrain set uses, as lists of lane indices in Tiled's order
//! ([`crate::wangid`] holds them as masks).

use bt_core::WangSetType;

use crate::wangid::{CORNER_LANES, EDGE_LANES};

/// Every lane, from the top edge clockwise.
const ALL_LANES: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// The lanes a set of `kind` labels: the four corners, the four edges, or all eight.
pub fn kind_lanes(kind: WangSetType) -> &'static [usize] {
    match kind {
        WangSetType::Corner => &CORNER_LANES,
        WangSetType::Edge => &EDGE_LANES,
        WangSetType::Mixed => &ALL_LANES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_lists_its_lanes() {
        assert_eq!(kind_lanes(WangSetType::Corner), [1, 3, 5, 7]);
        assert_eq!(kind_lanes(WangSetType::Edge), [0, 2, 4, 6]);
        assert_eq!(kind_lanes(WangSetType::Mixed).len(), 8);
    }
}
