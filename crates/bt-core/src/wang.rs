//! Terrain sets: Tiled's Wang sets.

use std::collections::BTreeMap;

use crate::color::Color;
use crate::ids::TileId;
use crate::property::Properties;

/// One of the eight places of a tile that carry a terrain, in Tiled's order. Odd lanes are
/// corners.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// The top edge.
    Top,
    /// The top-right corner.
    TopRight,
    /// The right edge.
    Right,
    /// The bottom-right corner.
    BottomRight,
    /// The bottom edge.
    Bottom,
    /// The bottom-left corner.
    BottomLeft,
    /// The left edge.
    Left,
    /// The top-left corner.
    TopLeft,
}

impl Lane {
    /// Every lane, in Tiled's order.
    pub const ALL: [Lane; 8] = [
        Lane::Top,
        Lane::TopRight,
        Lane::Right,
        Lane::BottomRight,
        Lane::Bottom,
        Lane::BottomLeft,
        Lane::Left,
        Lane::TopLeft,
    ];

    /// The lane's index in Tiled's order.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Whether the lane is a corner.
    pub fn is_corner(self) -> bool {
        self.index() % 2 == 1
    }
}

/// The terrains of a tile's eight lanes: each a 1-based colour index of its set, or 0 for none.
/// These are the eight values Tiled packs into its 64-bit Wang id.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct WangId {
    lanes: [u8; 8],
}

impl WangId {
    /// The id with these lanes, in Tiled's order.
    pub const fn from_lanes(lanes: [u8; 8]) -> WangId {
        WangId { lanes }
    }

    /// Reads Tiled's text forms: eight comma-separated numbers, or, from files before Tiled 1.5,
    /// a hexadecimal number with four bits a lane (`0x10101010`). `None` for anything else.
    pub fn from_tiled_text(text: &str) -> Option<WangId> {
        let text = text.trim();
        if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            let value = u32::from_str_radix(hex, 16).ok()?;
            let mut lanes = [0u8; 8];
            for (i, lane) in (0u32..).zip(lanes.iter_mut()) {
                *lane = u8::try_from((value >> (i * 4)) & 0xF).ok()?;
            }
            return Some(WangId { lanes });
        }
        let mut parts = text.split(',');
        let mut lanes = [0u8; 8];
        for lane in &mut lanes {
            *lane = parts.next()?.trim().parse().ok()?;
        }
        parts.next().is_none().then_some(WangId { lanes })
    }

    /// Tiled's text form: eight comma-separated numbers.
    pub fn to_tiled_text(self) -> String {
        let parts: Vec<String> = self.lanes.iter().map(u8::to_string).collect();
        parts.join(",")
    }

    /// The lanes, in Tiled's order.
    pub fn lanes(self) -> [u8; 8] {
        self.lanes
    }

    /// The terrain of one lane.
    pub fn lane(self, lane: Lane) -> u8 {
        self.lanes.get(lane.index()).copied().unwrap_or(0)
    }

    /// The same id with `lane` set to `color`.
    pub fn with_lane(mut self, lane: Lane, color: u8) -> WangId {
        if let Some(slot) = self.lanes.get_mut(lane.index()) {
            *slot = color;
        }
        self
    }

    /// Whether no lane has a terrain.
    pub fn is_empty(self) -> bool {
        self.lanes.iter().all(|&lane| lane == 0)
    }
}

/// Which lanes a terrain set uses.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum WangSetType {
    /// The four corners.
    #[default]
    Corner,
    /// The four edges.
    Edge,
    /// All eight.
    Mixed,
}

/// One terrain of a set.
#[derive(Clone, Debug, PartialEq)]
pub struct WangColor {
    /// The name.
    pub name: String,
    /// The class; empty for none.
    pub class: String,
    /// The colour the terrain is shown in.
    pub color: Color,
    /// The tile that stands for the terrain, if any.
    pub tile: Option<TileId>,
    /// Its relative weight when a fill picks terrains.
    pub probability: f64,
    /// Custom properties.
    pub properties: Properties,
}

/// A terrain set of a tileset.
#[derive(Clone, Debug, PartialEq)]
pub struct WangSet {
    /// The name.
    pub name: String,
    /// The class; empty for none.
    pub class: String,
    /// Which lanes it uses.
    pub kind: WangSetType,
    /// The tile that stands for the set, if any.
    pub tile: Option<TileId>,
    /// The terrains; lane value `n` is `colors[n - 1]`. At most 254.
    pub colors: Vec<WangColor>,
    /// The lanes of each labelled tile.
    pub tiles: BTreeMap<TileId, WangId>,
    /// Custom properties.
    pub properties: Properties,
}

impl WangSet {
    /// The most terrains a set can hold.
    pub const MAX_COLORS: usize = 254;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_text_forms_read() {
        assert_eq!(
            WangId::from_tiled_text("0,1,0,2,0,3,0,4"),
            Some(WangId::from_lanes([0, 1, 0, 2, 0, 3, 0, 4]))
        );
        assert_eq!(
            WangId::from_tiled_text("0x21"),
            Some(WangId::from_lanes([1, 2, 0, 0, 0, 0, 0, 0]))
        );
        assert_eq!(
            WangId::from_tiled_text("0x10101010").map(WangId::lanes),
            Some([0, 1, 0, 1, 0, 1, 0, 1])
        );
        assert_eq!(WangId::from_tiled_text("1,2"), None);
        assert_eq!(WangId::from_tiled_text("1,2,3,4,5,6,7,8,9"), None);
        assert_eq!(WangId::from_tiled_text("1,2,3,4,5,6,7,x"), None);
        assert_eq!(
            WangId::from_lanes([0, 1, 0, 2, 0, 3, 0, 255]).to_tiled_text(),
            "0,1,0,2,0,3,0,255"
        );
    }

    #[test]
    fn lanes_by_name() {
        let id = WangId::default().with_lane(Lane::TopLeft, 3);
        assert_eq!((id.lane(Lane::TopLeft), id.lane(Lane::Top)), (3, 0));
        assert!(Lane::TopLeft.is_corner() && !Lane::Left.is_corner());
        assert!(WangId::default().is_empty() && !id.is_empty());
    }
}
