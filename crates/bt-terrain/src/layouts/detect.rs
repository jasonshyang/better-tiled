//! Finding the blocks of a tileset image that look like one of the layouts: a block fits a
//! layout when, with the layout's labels, the colours near the terrain's lanes and near the
//! other lanes separate cleanly.

use std::cmp::Reverse;

use bt_core::WangSetType;
use bt_utils::IRect;
use glam::UVec2;

use super::samples::Samples;
use super::{LAYOUTS, Layout, Slot, block_tile};
use crate::lanes::kind_lanes;
use crate::pixels::{MeanColor, distance};

/// Where a layout fits the tiles of a tileset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fit {
    /// The layout: an index into [`LAYOUTS`].
    pub layout: usize,
    /// The block's top-left tile: its column and row in the atlas.
    pub origin: UVec2,
    /// Whether what the terrain is drawn over is see-through: no terrain.
    pub over_nothing: bool,
}

impl Fit {
    /// The block's tiles, as columns and rows of the atlas.
    pub fn area(&self) -> IRect {
        let size = LAYOUTS.get(self.layout).map_or(UVec2::ZERO, Layout::size);
        IRect::from_min_size(self.origin.as_ivec2(), size.as_ivec2())
    }

    fn overlaps(&self, other: &Fit) -> bool {
        !self.area().intersection(other.area()).is_empty()
    }
}

/// The least squared distance between the mean colours of a block's two sides (premultiplied
/// RGBA from 0 to 1).
const MIN_SEPARATION: f64 = 0.004;

/// How far apart the two sides must be compared with how much each varies.
const MIN_CONTRAST: f64 = 6.0;

/// At most one lane in this many may look more like the other side: loosely drawn terrains
/// (scattered paving stones) spill over a few lanes.
const MAX_WRONG_ONE_IN: usize = 15;

/// The mean alpha under which a side, or a lane, counts as see-through: mostly nothing, though a
/// ragged edge of the terrain may reach into it.
const SEE_THROUGH: f64 = 0.5;

/// How far from the terrain the opaque lanes of a partly see-through side must be to count as a
/// look of their own (a raised island's wall, the water around it) rather than bits of the
/// terrain spilling over.
const OTHER_LOOK: f64 = 0.02;

/// How many lanes a layout needs before what its terrain stands on may have two looks: a second
/// look explains more, so it takes a big block to tell a raised island from chance.
const MIN_LANES_FOR_TWO_LOOKS: usize = 64;

/// A layout as detection reads it, parsed once per search.
struct Parsed {
    index: usize,
    kind: WangSetType,
    size: UVec2,
    slots: Vec<Option<Slot>>,
}

/// One lane of a block: its tile (slot), which side the layout puts it on, and its mean colour.
struct LanePoint {
    slot: usize,
    terrain: bool,
    color: MeanColor,
}

/// How what the terrain is drawn over looks: one look (a ground, or nothing), or nothing in
/// places and something else that is not the terrain in others (a raised island over nothing,
/// with its own wall below it).
enum UnderLooks {
    One(MeanColor),
    Two { opaque: MeanColor, clear: MeanColor },
}

impl UnderLooks {
    fn of(points: &[LanePoint], terrain: &MeanColor, under: MeanColor, two_allowed: bool) -> Self {
        let lanes = |opaque: bool| {
            points
                .iter()
                .filter(move |p| !p.terrain && (alpha(&p.color) >= SEE_THROUGH) == opaque)
        };
        match (mean_of(lanes(true)), mean_of(lanes(false))) {
            (Some(opaque), Some(clear))
                if two_allowed && distance(&opaque, terrain) >= OTHER_LOOK =>
            {
                UnderLooks::Two { opaque, clear }
            }
            _ => UnderLooks::One(under),
        }
    }

    /// The look of an under lane of `color`.
    fn own(&self, color: &MeanColor) -> MeanColor {
        match *self {
            UnderLooks::One(look) => look,
            UnderLooks::Two { opaque, clear } => {
                if alpha(color) >= SEE_THROUGH {
                    opaque
                } else {
                    clear
                }
            }
        }
    }

    /// How far `color` is from the nearest look.
    fn nearest(&self, color: &MeanColor) -> f64 {
        match self {
            UnderLooks::One(look) => distance(color, look),
            UnderLooks::Two { opaque, clear } => {
                distance(color, opaque).min(distance(color, clear))
            }
        }
    }
}

/// How layout `layout` fits the block at `origin`: `None` when its terrain and the rest do not
/// look apart, when more than one lane in fifteen looks like the other side, when two or more
/// lanes of one side of a tile all look like the other side (a tile the layout needs is not
/// there, such as an empty corner where a layout has a diagonal tile), or when the terrain is the
/// see-through side (that is the layout the other way around: an island of nothing is a hole,
/// and props over nothing often have the shape of a small island).
fn fit(samples: &Samples, layout: &Parsed, origin: UVec2) -> Option<Fit> {
    let points = lane_points(samples, layout, origin)?;
    let terrain = mean_of(points.iter().filter(|p| p.terrain))?;
    let under = mean_of(points.iter().filter(|p| !p.terrain))?;
    let separation = distance(&terrain, &under);
    if separation < MIN_SEPARATION || alpha(&terrain) < SEE_THROUGH {
        return None;
    }
    let looks = UnderLooks::of(
        &points,
        &terrain,
        under,
        points.len() >= MIN_LANES_FOR_TWO_LOOKS,
    );
    let mut wrong = Vec::with_capacity(points.len());
    let mut spread = 0.0;
    for point in &points {
        let (to_own, to_other) = if point.terrain {
            (
                distance(&point.color, &terrain),
                looks.nearest(&point.color),
            )
        } else {
            (
                distance(&point.color, &looks.own(&point.color)),
                distance(&point.color, &terrain),
            )
        };
        wrong.push(to_own >= to_other);
        spread += to_own;
    }
    let wrong_count = wrong.iter().filter(|&&w| w).count();
    let contrast = separation / (spread / points.len() as f64 + 1e-4);
    if wrong_count * MAX_WRONG_ONE_IN > points.len()
        || has_lost_tile(&points, &wrong)
        || contrast < MIN_CONTRAST
    {
        return None;
    }
    Some(Fit {
        layout: layout.index,
        origin,
        over_nothing: alpha(&under) < SEE_THROUGH,
    })
}

/// The lanes of the block of `layout` at `origin`, tile by tile; `None` when the block does not
/// fit in the atlas or a tile is missing.
fn lane_points(samples: &Samples, layout: &Parsed, origin: UVec2) -> Option<Vec<LanePoint>> {
    let size = layout.size;
    let atlas = samples.size();
    if origin.x + size.x > atlas.x || origin.y + size.y > atlas.y {
        return None;
    }
    let lanes = kind_lanes(layout.kind);
    let mut points = Vec::with_capacity(layout.slots.len() * lanes.len());
    for (slot_index, slot) in layout.slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        let tile = block_tile(origin, atlas.x, size.x, slot_index)?;
        let means = samples.means(tile)?;
        for &lane in lanes {
            points.push(LanePoint {
                slot: slot_index,
                terrain: slot.is_terrain(lane),
                color: *means.get(lane)?,
            });
        }
    }
    Some(points)
}

/// A mean colour's alpha.
fn alpha(color: &MeanColor) -> f64 {
    color.get(3).copied().unwrap_or(0.0)
}

/// The mean colour of some lanes; `None` for none.
fn mean_of<'a>(points: impl Iterator<Item = &'a LanePoint>) -> Option<MeanColor> {
    let mut sum = [0.0; 4];
    let mut count = 0_u32;
    for point in points {
        for (total, value) in sum.iter_mut().zip(point.color) {
            *total += value;
        }
        count += 1;
    }
    (count > 0).then(|| sum.map(|total| total / f64::from(count)))
}

/// Whether two or more lanes of one side of some tile all look like the other side. (A single
/// lane, such as an outer corner's tip of terrain, may well look like the other side.)
fn has_lost_tile(points: &[LanePoint], wrong: &[bool]) -> bool {
    let mut start = 0;
    while let Some(first) = points.get(start) {
        let end = points
            .iter()
            .skip(start)
            .position(|p| p.slot != first.slot)
            .map_or(points.len(), |offset| start + offset);
        let tile = points.get(start..end).unwrap_or_default();
        let tile_wrong = wrong.get(start..end).unwrap_or_default();
        for side in [true, false] {
            let side_wrong: Vec<bool> = tile
                .iter()
                .zip(tile_wrong)
                .filter(|(point, _)| point.terrain == side)
                .map(|(_, &w)| w)
                .collect();
            if side_wrong.len() >= 2 && side_wrong.iter().all(|&w| w) {
                return true;
            }
        }
        start = end;
    }
    false
}

/// Sorts fits best first: the most tiles, then in reading order.
fn rank(fits: &mut [Fit]) {
    fits.sort_by_cached_key(|f| {
        (
            Reverse(LAYOUTS.get(f.layout).map_or(0, Layout::tile_count)),
            f.origin.y,
            f.origin.x,
        )
    });
}

/// The ranked fits without those that overlap a better one.
fn apart(ranked: Vec<Fit>) -> Vec<Fit> {
    let mut kept: Vec<Fit> = Vec::new();
    for candidate in ranked {
        if !kept.iter().any(|f| f.overlaps(&candidate)) {
            kept.push(candidate);
        }
    }
    kept
}

/// Every block of the atlas where layout `index` (into [`LAYOUTS`]) fits, best first, none
/// overlapping another.
pub fn places(samples: &Samples, index: usize) -> Vec<Fit> {
    let Some(layout) = LAYOUTS.get(index) else {
        return Vec::new();
    };
    let parsed = Parsed {
        index,
        kind: layout.kind,
        size: layout.size(),
        slots: layout.slots(),
    };
    let atlas = samples.size();
    let mut fits: Vec<Fit> = (0..atlas.y)
        .flat_map(|y| (0..atlas.x).map(move |x| UVec2::new(x, y)))
        .filter_map(|origin| fit(samples, &parsed, origin))
        .collect();
    rank(&mut fits);
    apart(fits)
}

/// The blocks of the atlas that look like one of the layouts, best first, none overlapping
/// another: a bigger layout wins over the smaller ones inside it, and between two opaque
/// terrains, where an island of one is a hole in the other, the island.
pub fn detect(samples: &Samples) -> Vec<Fit> {
    let mut fits: Vec<Fit> = (0..LAYOUTS.len())
        .flat_map(|index| places(samples, index))
        .collect();
    rank(&mut fits);
    apart(fits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(slot: usize, terrain: bool) -> LanePoint {
        LanePoint {
            slot,
            terrain,
            color: [0.0; 4],
        }
    }

    #[test]
    fn a_tile_is_lost_when_two_of_its_lanes_of_a_side_look_wrong() {
        // Tile 0: one terrain lane, wrong (an outer corner's tip); tile 1: two terrain lanes.
        let points = [
            point(0, true),
            point(0, false),
            point(0, false),
            point(1, true),
            point(1, true),
            point(1, false),
        ];
        assert!(!has_lost_tile(
            &points,
            &[true, false, false, true, false, false]
        ));
        assert!(has_lost_tile(
            &points,
            &[false, false, false, true, true, false]
        ));
        assert!(has_lost_tile(
            &points,
            &[false, true, true, false, false, false]
        ));
    }

    #[test]
    fn bigger_layouts_win_and_overlaps_go() {
        let (island, _) = super::super::tests::layout("3×3 island");
        let (blob, _) = super::super::tests::layout("Blob 7×7");
        let fit = |layout: usize, x: u32, y: u32| Fit {
            layout,
            origin: UVec2::new(x, y),
            over_nothing: false,
        };
        let mut fits = vec![fit(island, 0, 0), fit(blob, 2, 2), fit(island, 9, 0)];
        rank(&mut fits);
        assert_eq!(
            apart(fits),
            [fit(blob, 2, 2), fit(island, 9, 0)],
            "the island at the origin overlaps the blob"
        );
        assert_eq!(
            fit(blob, 2, 2).area(),
            IRect {
                x: 2,
                y: 2,
                w: 7,
                h: 7
            }
        );
    }
}
