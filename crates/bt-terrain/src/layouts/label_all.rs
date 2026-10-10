//! Label All: once blocks are found, blocks that look alike get the same terrain and the same
//! ground. Terrains are told apart by their colours (see [`Histogram`]), since mean colours
//! cannot separate terrains such as dirt and rooty dirt.

use std::collections::{BTreeMap, BTreeSet};

use bt_core::{TileId, WangId, WangSet};
use glam::UVec2;

use super::LAYOUTS;
use super::detect::Fit;
use super::histogram::Histogram;
use super::samples::Samples;
use crate::color::TerrainColor;

/// How much of its colours a side of a block shares with a terrain to be that terrain.
pub const SAME_TERRAIN: f64 = 0.8;

/// How much of its colours the ground of a block (the side its terrain is drawn over) shares
/// with a terrain that was a ground before to be that terrain: grounds are drawn loosely, like
/// grass showing through a hole.
pub const SAME_GROUND: f64 = 0.5;

/// Which of the `known` terrains (their colours, terrain `n` at `n - 1`; `None` for a terrain
/// nothing shows yet) a side of a block with colours `side` is: among those `among` lets in, the
/// one whose colours overlap the side's most, when they overlap by at least `least` (see
/// [`Histogram::overlap`]). On a tie the lower terrain wins.
pub fn recognize(
    side: &Histogram,
    known: &[Option<Histogram>],
    least: f64,
    among: impl Fn(TerrainColor) -> bool,
) -> Option<TerrainColor> {
    let mut best: Option<(TerrainColor, f64)> = None;
    for (index, colors) in known.iter().enumerate() {
        let Some(colors) = colors else { continue };
        let Some(terrain) = u8::try_from(index + 1).ok().map(TerrainColor) else {
            continue;
        };
        if !among(terrain) {
            continue;
        }
        let overlap = side.overlap(colors);
        if best.is_none_or(|(_, most)| overlap > most) {
            best = Some((terrain, overlap));
        }
    }
    best.filter(|&(_, overlap)| overlap >= least)
        .map(|(terrain, _)| terrain)
}

/// A block to label: a layout at a place, with the terrains of its two sides.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The layout: an index into [`LAYOUTS`].
    pub layout: usize,
    /// The block's top-left tile: its column and row in the atlas.
    pub origin: UVec2,
    /// The terrain (`#` in the layout's art). Past the set's terrains, a new one.
    pub terrain: TerrainColor,
    /// What it is drawn over (`.`): no terrain over nothing; past the set's terrains, a new one.
    pub under: TerrainColor,
}

impl Block {
    /// The labels of the block's tiles in an atlas `columns` tiles wide (see
    /// [`super::Layout::labels`]).
    pub fn labels(&self, columns: u32) -> BTreeMap<TileId, Option<WangId>> {
        LAYOUTS
            .get(self.layout)
            .map(|layout| layout.labels(self.origin, columns, self.terrain, self.under))
            .unwrap_or_default()
    }
}

/// What Label All knows of the terrains of the set it labels: how each looks, and which ones
/// blocks were drawn over. It learns both as it gives blocks their terrains.
#[derive(Clone, Debug, Default)]
pub struct KnownTerrains {
    /// Terrain `n` at `n - 1`; `None` for a terrain nothing shows yet.
    colors: Vec<Option<Histogram>>,
    /// The terrains blocks were drawn over.
    grounds: BTreeSet<TerrainColor>,
}

impl KnownTerrains {
    /// The terrains of `set` as its labelled tiles show them in `samples`; `grounds` are the
    /// terrains blocks were drawn over before (Label All learns them as it goes; a caller that
    /// labels block by block passes on the ones it saw).
    pub fn from_set(samples: &Samples, set: &WangSet, grounds: &[TerrainColor]) -> KnownTerrains {
        let colors = samples.terrain_colors(set.kind, &set.tiles, set.colors.len());
        let grounds = grounds
            .iter()
            .copied()
            .filter(|g| !g.is_none() && usize::from(g.0) <= colors.len())
            .collect();
        KnownTerrains { colors, grounds }
    }

    /// How many terrains it knows: the set's, and the new ones it gave out.
    pub fn terrain_count(&self) -> usize {
        self.colors.len()
    }

    /// Gives a side of a block a terrain: one whose colours it shares by [`SAME_TERRAIN`], or for
    /// a ground (`ground`) one that was a ground before and that it shares by [`SAME_GROUND`];
    /// else the first terrain nothing shows yet; else a new one. Never `except`, the other side
    /// of the block. The side's colours are then known as the terrain's.
    fn assign_side(
        &mut self,
        side: &Histogram,
        ground: bool,
        except: Option<TerrainColor>,
    ) -> TerrainColor {
        let allowed = |t: TerrainColor| Some(t) != except;
        let known = recognize(side, &self.colors, SAME_TERRAIN, allowed).or_else(|| {
            if ground {
                recognize(side, &self.colors, SAME_GROUND, |t| {
                    allowed(t) && self.grounds.contains(&t)
                })
            } else {
                None
            }
        });
        let unseen = || {
            self.colors
                .iter()
                .enumerate()
                .filter(|(_, colors)| colors.is_none())
                .filter_map(|(index, _)| u8::try_from(index + 1).ok().map(TerrainColor))
                .find(|&t| allowed(t))
        };
        let terrain = known.or_else(unseen).unwrap_or_else(|| {
            self.colors.push(None);
            TerrainColor(u8::try_from(self.colors.len()).unwrap_or(u8::MAX))
        });
        if let Some(slot) = usize::from(terrain.0)
            .checked_sub(1)
            .and_then(|index| self.colors.get_mut(index))
        {
            slot.get_or_insert_default().add(side);
        }
        if ground {
            self.grounds.insert(terrain);
        }
        terrain
    }

    /// Gives the block of `fit` its terrains by its looks, learning them.
    fn assign_block(&mut self, samples: &Samples, fit: &Fit) -> Block {
        let sides = samples.side_colors(fit.layout, fit.origin);
        let terrain = self.assign_side(&sides.terrain, false, None);
        let under = if fit.over_nothing {
            TerrainColor::NONE
        } else {
            self.assign_side(&sides.under, true, Some(terrain))
        };
        Block {
            layout: fit.layout,
            origin: fit.origin,
            terrain,
            under,
        }
    }

    /// The terrains the block of `fit` looks like, without learning them: what labelling that
    /// block alone would give it.
    pub fn guess(&self, samples: &Samples, fit: &Fit) -> Block {
        self.clone().assign_block(samples, fit)
    }

    /// Label All: every block of `fits`, in order, with terrains by their looks. Blocks that look
    /// alike get the same terrain and the same ground, also when the terrain is new; a block over
    /// nothing gets no terrain under it. Terrains past the set's are new: the caller adds them.
    pub fn label_all(&mut self, samples: &Samples, fits: &[Fit]) -> Vec<Block> {
        fits.iter()
            .map(|fit| self.assign_block(samples, fit))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(pixels: &[[u8; 4]]) -> Histogram {
        Histogram::of_pixels(pixels.iter().copied())
    }

    #[test]
    fn a_side_is_the_terrain_it_overlaps_most() {
        let grass = colors(&[[60, 170, 60, 255], [70, 180, 70, 255]]);
        let dirt = colors(&[[150, 100, 50, 255]]);
        let known = [Some(dirt.clone()), None, Some(grass.clone())];
        let c = TerrainColor;
        assert_eq!(
            recognize(&grass, &known, SAME_TERRAIN, |_| true),
            Some(c(3))
        );
        assert_eq!(recognize(&dirt, &known, SAME_TERRAIN, |_| true), Some(c(1)));
        assert_eq!(
            recognize(&dirt, &known, SAME_TERRAIN, |t| t != c(1)),
            None,
            "only among the terrains let in"
        );
        let half = colors(&[[60, 170, 60, 255], [9, 9, 200, 255]]);
        assert_eq!(recognize(&half, &known, SAME_TERRAIN, |_| true), None);
        assert_eq!(recognize(&half, &known, SAME_GROUND, |_| true), Some(c(3)));
        // A tie goes to the lower terrain.
        let twice = [Some(grass.clone()), Some(grass.clone())];
        assert_eq!(
            recognize(&grass, &twice, SAME_TERRAIN, |_| true),
            Some(c(1))
        );
    }

    #[test]
    fn sides_take_known_unseen_then_new_terrains() {
        let mut known = KnownTerrains {
            colors: vec![None, Some(colors(&[[200, 0, 0, 255]]))],
            grounds: BTreeSet::new(),
        };
        let c = TerrainColor;
        let blue = colors(&[[0, 0, 200, 255]]);
        let red = colors(&[[200, 0, 0, 255]]);
        assert_eq!(known.assign_side(&red, false, None), c(2), "known");
        assert_eq!(known.assign_side(&blue, false, None), c(1), "unseen");
        let green = colors(&[[0, 200, 0, 255]]);
        assert_eq!(known.assign_side(&green, true, None), c(3), "new");
        assert_eq!(known.terrain_count(), 3);
        assert_eq!(
            known.assign_side(&green, true, Some(c(3))),
            c(4),
            "never the block's other side"
        );
        // A ground that looks half like a ground seen before is that ground.
        let grassy = colors(&[[0, 200, 0, 255], [90, 90, 90, 255]]);
        assert_eq!(known.assign_side(&grassy, true, None), c(3));
        assert_eq!(
            known.assign_side(&colors(&[[0, 200, 0, 255], [90, 90, 90, 255]]), false, None),
            c(3),
            "learned: the ground's colours now include the grey"
        );
    }
}
