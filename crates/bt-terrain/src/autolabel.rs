//! Auto-labelling: one terrain labelled on every tile at once from one tile that is all of it
//! (`docs/design/07-terrain.md`, §10). Each lane's part of each tile is compared with the same
//! part of the reference tile; the lanes that look like it are the terrain. Tiles with no part
//! of the terrain are left alone, and other terrains' labels are kept.

use std::collections::BTreeMap;

use bt_core::{TileId, WangId, WangSetType};
use bt_image::DecodedImage;
use bt_utils::IRect;
use glam::UVec2;

use crate::color::TerrainColor;
use crate::lanes::kind_lanes;
use crate::pixels::{MeanColor, distance, mean_color};

/// How the side of a lane that is not the terrain is told apart from it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Method {
    /// One of the two below, chosen from the tiles: over nothing when many lanes are see-through.
    #[default]
    Auto,
    /// The terrain is drawn over nothing: a lane is the terrain when it looks more like the
    /// reference than like nothing. A tile that looks like neither is left alone.
    Transparency,
    /// The terrain is drawn over another ground: a lane is the terrain when its distance to the
    /// reference is under a threshold learned from all the lanes (Otsu's method, on a log
    /// scale).
    Learned,
}

/// What auto-labelling proposes for one terrain.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// The terrain.
    pub color: TerrainColor,
    /// The kind of set the lanes are of.
    pub kind: WangSetType,
    /// The method used: [`Method::Auto`] says which it chose.
    pub method: Method,
    /// Each tile with some part of the terrain: its lanes that are the terrain hold it, the
    /// others no terrain.
    pub tiles: BTreeMap<TileId, WangId>,
}

impl Proposal {
    /// Applies the proposal to a set's labels: on each proposed tile the terrain's lanes follow
    /// the proposal, except where another terrain is labelled, which is kept; other terrains'
    /// labels stay everywhere, tiles the proposal leaves out are left alone, and a label left
    /// with no terrain at all goes. Returns how many lanes kept another terrain where the
    /// proposal had this one.
    pub fn apply_to(&self, labels: &mut BTreeMap<TileId, WangId>) -> usize {
        let mut kept = 0;
        for (&tile, proposed) in &self.tiles {
            let mut lanes = labels.get(&tile).copied().unwrap_or_default().lanes();
            let wanted = proposed.lanes();
            for &lane in kind_lanes(self.kind) {
                let (Some(current), Some(&want)) = (lanes.get_mut(lane), wanted.get(lane)) else {
                    continue;
                };
                if want == self.color.0 {
                    if *current == 0 {
                        *current = self.color.0;
                    } else if *current != self.color.0 {
                        kept += 1;
                    }
                } else if *current == self.color.0 {
                    *current = 0;
                }
            }
            let label = WangId::from_lanes(lanes);
            if label.is_empty() {
                labels.remove(&tile);
            } else {
                labels.insert(tile, label);
            }
        }
        kept
    }
}

/// The share of see-through lanes from which [`Method::Auto`] takes the terrain to be drawn over
/// nothing (calibrated on tilesets with hand-made labels).
const SEE_THROUGH_SHARE_PERCENT: usize = 37;

/// How far, relative to the distance between the reference and nothing, a lane may be from the
/// nearer of the two and still count as looking like one of them ([`Method::Transparency`]).
const LIKENESS: f64 = 0.45;

/// Labels terrain `color` on `tiles` (each tile's rectangle in `image`, tiles `tile_size` pixels)
/// of a set of `kind`, from `reference`, the rectangle of a tile that is all of it.
pub fn label_with(
    method: Method,
    image: &DecodedImage,
    kind: WangSetType,
    tile_size: UVec2,
    reference: IRect,
    color: TerrainColor,
    tiles: &BTreeMap<TileId, IRect>,
) -> Proposal {
    let sheet = Sheet {
        image,
        kind,
        tile_size,
        reference,
        color,
    };
    let method = match method {
        Method::Auto => sheet.choose(tiles),
        chosen => chosen,
    };
    let tiles = match method {
        Method::Transparency => sheet.over_nothing(tiles),
        Method::Auto | Method::Learned => sheet.learned(tiles),
    };
    Proposal {
        color,
        kind,
        method,
        tiles,
    }
}

/// What one auto-labelling run reads.
struct Sheet<'a> {
    image: &'a DecodedImage,
    kind: WangSetType,
    tile_size: UVec2,
    reference: IRect,
    color: TerrainColor,
}

/// One lane of one tile and how far it is from the reference's.
struct LaneDistance {
    tile: TileId,
    lane: usize,
    log_distance: f64,
}

impl Sheet<'_> {
    /// The mean colour near lane `lane` of the tile at `rect`.
    fn lane_color(&self, rect: IRect, lane: usize) -> MeanColor {
        mean_color(
            self.image,
            lane_region(lane, self.tile_size).translated(rect.min()),
        )
    }

    /// [`Method::Transparency`] when enough of the tiles' lanes are see-through, else
    /// [`Method::Learned`].
    fn choose(&self, tiles: &BTreeMap<TileId, IRect>) -> Method {
        let mut see_through = 0_usize;
        let mut total = 0_usize;
        for &rect in tiles.values() {
            for &lane in kind_lanes(self.kind) {
                total += 1;
                let alpha = self.lane_color(rect, lane).get(3).copied().unwrap_or(0.0);
                see_through += usize::from(alpha < 0.5);
            }
        }
        if see_through * 100 >= total * SEE_THROUGH_SHARE_PERCENT && total > 0 {
            Method::Transparency
        } else {
            Method::Learned
        }
    }

    /// Each lane is the terrain when it looks more like the reference than like nothing; a tile
    /// with a lane that looks like neither is left alone.
    fn over_nothing(&self, tiles: &BTreeMap<TileId, IRect>) -> BTreeMap<TileId, WangId> {
        let nothing: MeanColor = [0.0; 4];
        let mut labels = BTreeMap::new();
        'tiles: for (&tile, &rect) in tiles {
            let mut lanes = [0_u8; 8];
            for &lane in kind_lanes(self.kind) {
                let target = self.lane_color(rect, lane);
                let reference = self.lane_color(self.reference, lane);
                let to_reference = distance(&target, &reference);
                let to_nothing = distance(&target, &nothing);
                let apart = distance(&reference, &nothing);
                if apart <= 1e-9 || to_reference.min(to_nothing) > apart * LIKENESS {
                    continue 'tiles;
                }
                if to_reference <= to_nothing
                    && let Some(value) = lanes.get_mut(lane)
                {
                    *value = self.color.0;
                }
            }
            if lanes.iter().any(|&v| v != 0) {
                labels.insert(tile, WangId::from_lanes(lanes));
            }
        }
        labels
    }

    /// Each lane is the terrain when its distance to the reference's lane is under the threshold
    /// that best splits all the lanes' distances (Otsu's method on a log scale, floored, so that
    /// lanes identical to the reference do not form a class of their own).
    fn learned(&self, tiles: &BTreeMap<TileId, IRect>) -> BTreeMap<TileId, WangId> {
        let mut distances = Vec::new();
        for (&tile, &rect) in tiles {
            for &lane in kind_lanes(self.kind) {
                let d = distance(
                    &self.lane_color(rect, lane),
                    &self.lane_color(self.reference, lane),
                );
                distances.push(LaneDistance {
                    tile,
                    lane,
                    log_distance: (d + 1e-6).ln().max(-8.0),
                });
            }
        }
        let values: Vec<f64> = distances.iter().map(|d| d.log_distance).collect();
        let threshold = otsu(&values);
        let mut labels: BTreeMap<TileId, [u8; 8]> = BTreeMap::new();
        for d in distances.iter().filter(|d| d.log_distance < threshold) {
            if let Some(value) = labels.entry(d.tile).or_default().get_mut(d.lane) {
                *value = self.color.0;
            }
        }
        labels
            .into_iter()
            .map(|(tile, lanes)| (tile, WangId::from_lanes(lanes)))
            .collect()
    }
}

/// The threshold that best splits `values` in two (the largest variance between the two
/// groups); with fewer than two values, everything is under it.
fn otsu(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n < 2 {
        return f64::INFINITY;
    }
    let total: f64 = sorted.iter().sum();
    let mut best_between = f64::MIN;
    let mut best = f64::INFINITY;
    let mut left = 0.0;
    for (i, pair) in sorted.windows(2).enumerate() {
        let [low, high] = pair else { continue };
        left += low;
        let (n0, n1) = ((i + 1) as f64, (n - i - 1) as f64);
        let (m0, m1) = (left / n0, (total - left) / n1);
        let between = n0 * n1 * (m0 - m1).powi(2);
        if between > best_between {
            best_between = between;
            best = (low + high) * 0.5;
        }
    }
    best
}

/// The part of a tile of `size` a lane is judged on, relative to the tile: the two fifths
/// nearest a corner, or the middle of a side, where only one terrain should show.
fn lane_region(lane: usize, size: UVec2) -> IRect {
    let w = i32::try_from(size.x).unwrap_or(i32::MAX);
    let h = i32::try_from(size.y).unwrap_or(i32::MAX);
    let rect = |x: i32, y: i32, w: i32, h: i32| IRect { x, y, w, h };
    let (corner_w, corner_h) = ((w * 2 / 5).max(1), (h * 2 / 5).max(1));
    let (along_w, across_h) = ((w * 2 / 5).max(1), (h * 3 / 10).max(1));
    let (across_w, along_h) = ((w * 3 / 10).max(1), (h * 2 / 5).max(1));
    match lane {
        0 => rect((w - along_w) / 2, 0, along_w, across_h),
        1 => rect(w - corner_w, 0, corner_w, corner_h),
        2 => rect(w - across_w, (h - along_h) / 2, across_w, along_h),
        3 => rect(w - corner_w, h - corner_h, corner_w, corner_h),
        4 => rect((w - along_w) / 2, h - across_h, along_w, across_h),
        5 => rect(0, h - corner_h, corner_w, corner_h),
        6 => rect(0, (h - along_h) / 2, across_w, along_h),
        _ => rect(0, 0, corner_w, corner_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREEN: [u8; 4] = [40, 200, 40, 255];
    const BROWN: [u8; 4] = [140, 90, 40, 255];
    const BLUE: [u8; 4] = [40, 60, 220, 255];

    /// A strip of 8×8 tiles, each drawn by `color(tile, x, y)`.
    fn strip(count: u32, color: impl Fn(u32, u32, u32) -> [u8; 4]) -> DecodedImage {
        let (width, height) = (count * 8, 8);
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend(color(x / 8, x % 8, y));
            }
        }
        DecodedImage {
            width,
            height,
            rgba,
        }
    }

    fn rects(count: u32) -> BTreeMap<TileId, IRect> {
        (0..count)
            .map(|i| {
                (
                    TileId(i),
                    IRect {
                        x: i as i32 * 8,
                        y: 0,
                        w: 8,
                        h: 8,
                    },
                )
            })
            .collect()
    }

    fn corners(tr: u8, br: u8, bl: u8, tl: u8) -> WangId {
        WangId::from_lanes([0, tr, 0, br, 0, bl, 0, tl])
    }

    #[test]
    fn a_learned_threshold_tells_the_terrain_from_another_ground() {
        // Green, brown, green on the left half, green on the top-left quarter, then blue.
        let image = strip(5, |tile, x, y| match tile {
            0 => GREEN,
            1 => BROWN,
            2 if x < 4 => GREEN,
            3 if x < 4 && y < 4 => GREEN,
            4 => BLUE,
            _ => BROWN,
        });
        let reference = IRect {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        };
        let proposal = label_with(
            Method::Auto,
            &image,
            WangSetType::Corner,
            UVec2::splat(8),
            reference,
            TerrainColor(1),
            &rects(5),
        );
        assert_eq!(proposal.method, Method::Learned, "nothing is see-through");
        let expected: BTreeMap<TileId, WangId> = [
            (TileId(0), corners(1, 1, 1, 1)),
            (TileId(2), corners(0, 0, 1, 1)),
            (TileId(3), corners(0, 0, 0, 1)),
        ]
        .into();
        assert_eq!(proposal.tiles, expected, "tiles with no green are left out");
    }

    #[test]
    fn over_nothing_the_other_side_is_no_terrain() {
        // Red; red on the top half; nothing twice; then blue, which looks like neither.
        let image = strip(5, |tile, _, y| match tile {
            0 => [220, 30, 30, 255],
            1 if y < 4 => [220, 30, 30, 255],
            4 => BLUE,
            _ => [0, 0, 0, 0],
        });
        let reference = IRect {
            x: 0,
            y: 0,
            w: 8,
            h: 8,
        };
        let proposal = label_with(
            Method::Auto,
            &image,
            WangSetType::Corner,
            UVec2::splat(8),
            reference,
            TerrainColor(2),
            &rects(5),
        );
        assert_eq!(proposal.method, Method::Transparency);
        let expected: BTreeMap<TileId, WangId> = [
            (TileId(0), corners(2, 2, 2, 2)),
            (TileId(1), corners(2, 0, 0, 2)),
        ]
        .into();
        assert_eq!(proposal.tiles, expected);
    }

    #[test]
    fn applying_keeps_other_terrains_and_leaves_other_tiles_alone() {
        let mut labels: BTreeMap<TileId, WangId> = [
            (TileId(1), corners(1, 2, 2, 1)),
            (TileId(2), corners(1, 1, 1, 1)),
            (TileId(4), corners(1, 0, 0, 0)),
        ]
        .into();
        let proposal = Proposal {
            color: TerrainColor(1),
            kind: WangSetType::Corner,
            method: Method::Learned,
            tiles: [
                (TileId(1), corners(1, 1, 0, 0)),
                (TileId(3), corners(0, 1, 0, 0)),
                (TileId(4), corners(0, 1, 0, 0)),
            ]
            .into(),
        };
        let kept = proposal.apply_to(&mut labels);
        assert_eq!(
            labels[&TileId(1)],
            corners(1, 2, 2, 0),
            "terrain 2 stays where the proposal has 1; terrain 1 follows the proposal"
        );
        assert_eq!(kept, 1);
        assert_eq!(
            labels[&TileId(2)],
            corners(1, 1, 1, 1),
            "not proposed: left alone"
        );
        assert_eq!(labels[&TileId(3)], corners(0, 1, 0, 0));
        assert_eq!(
            labels[&TileId(4)],
            corners(0, 1, 0, 0),
            "moved to another corner"
        );
        let gone = Proposal {
            tiles: [(TileId(3), corners(0, 0, 0, 0))].into(),
            ..proposal
        };
        gone.apply_to(&mut labels);
        assert!(!labels.contains_key(&TileId(3)), "a label left empty goes");
    }

    #[test]
    fn otsu_splits_two_groups() {
        assert_eq!(otsu(&[1.0, 1.1, 0.9, 5.0, 5.2]), (1.1 + 5.0) / 2.0);
        assert_eq!(otsu(&[3.0]), f64::INFINITY);
    }

    #[test]
    fn lanes_are_judged_near_their_corner_or_side() {
        let size = UVec2::splat(20);
        assert_eq!(
            lane_region(7, size),
            IRect {
                x: 0,
                y: 0,
                w: 8,
                h: 8
            }
        );
        assert_eq!(
            lane_region(0, size),
            IRect {
                x: 6,
                y: 0,
                w: 8,
                h: 6
            }
        );
        assert_eq!(
            lane_region(2, size),
            IRect {
                x: 14,
                y: 6,
                w: 6,
                h: 8
            }
        );
        assert_eq!(
            lane_region(3, size),
            IRect {
                x: 12,
                y: 12,
                w: 8,
                h: 8
            }
        );
    }
}
