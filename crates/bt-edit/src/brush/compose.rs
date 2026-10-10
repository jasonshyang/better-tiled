//! A clip put together from a brush's variations put down side by side: the n-th layer of a name
//! in a variation goes to the n-th layer of that name in the result, made the first time it is
//! needed, and the tilesets of all of them are listed once (Tiled's `targetLayersByName` in
//! `StampBrush::drawPreviewLayer` and `AbstractTileFillTool::fillWithStamp`,
//! `reference/tiled/src/tiled/stampbrush.cpp`, `abstracttilefilltool.cpp`).

use std::collections::BTreeMap;

use bt_core::PlacedCell;
use glam::IVec2;

use crate::clip::{ClipLayer, ClipTilesets, TileClip};
use crate::region::TileRegion;

/// A variation's tiles as they are put down.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    /// The rectangle a placement centres.
    pub(crate) size: IVec2,
    /// For each layer of the variation, its tiles relative to the top-left.
    pub(crate) layers: Vec<Vec<PlacedCell>>,
    /// Where any layer has a tile, relative to the top-left.
    pub(crate) region: TileRegion,
}

impl Shape {
    /// A clip's tiles where they are.
    pub(crate) fn of(clip: &TileClip) -> Shape {
        Shape::moved(clip, clip.size, |at| at)
    }

    /// A clip's tiles each moved by `to`, in a rectangle of `size`.
    pub(crate) fn moved(clip: &TileClip, size: IVec2, to: impl Fn(IVec2) -> IVec2) -> Shape {
        let layers: Vec<Vec<PlacedCell>> = clip
            .layers
            .iter()
            .map(|layer| {
                layer
                    .cells
                    .cells()
                    .map(|placed| PlacedCell {
                        at: to(placed.at),
                        cell: placed.cell,
                    })
                    .collect()
            })
            .collect();
        let region = layers.iter().flatten().map(|placed| placed.at).collect();
        Shape {
            size,
            layers,
            region,
        }
    }
}

/// A clip being put together from variations.
pub(crate) struct Composer {
    /// Where the result's top-left is.
    origin: IVec2,
    layers: Vec<ClipLayer>,
    tilesets: ClipTilesets,
    /// For each variation put down so far, the result layer each of its layers goes to.
    targets: BTreeMap<usize, Vec<usize>>,
}

impl Composer {
    /// Nothing yet, for a result whose top-left is `origin`.
    pub(crate) fn new(origin: IVec2) -> Composer {
        Composer {
            origin,
            layers: Vec::new(),
            tilesets: ClipTilesets::new(),
            targets: BTreeMap::new(),
        }
    }

    /// Puts down the tiles of variation `variation` (`clip`, as `shape` places it) with the
    /// shape's top-left at `top_left`, the cells `keep` refuses left out. Later tiles cover
    /// earlier ones.
    pub(crate) fn place(
        &mut self,
        variation: usize,
        clip: &TileClip,
        shape: &Shape,
        top_left: IVec2,
        keep: impl Fn(IVec2) -> bool,
    ) {
        let targets = self.targets_of(variation, clip);
        for (cells, target) in shape.layers.iter().zip(targets) {
            let Some(layer) = self.layers.get_mut(target) else {
                continue;
            };
            for placed in cells {
                let at = top_left + placed.at;
                if !keep(at) {
                    continue;
                }
                // A brush's cells name tilesets its clips list (its constructors check), so
                // adopting one cannot fail.
                let Ok(cell) = self.tilesets.adopt(&clip.tilesets, placed.cell) else {
                    continue;
                };
                layer.put(at - self.origin, cell);
            }
        }
    }

    /// The result layers `clip`'s layers go to, made with their names, opacities and tints when
    /// missing.
    fn targets_of(&mut self, variation: usize, clip: &TileClip) -> Vec<usize> {
        if let Some(targets) = self.targets.get(&variation) {
            return targets.clone();
        }
        let mut occurrences: BTreeMap<&str, usize> = BTreeMap::new();
        let mut targets = Vec::with_capacity(clip.layers.len());
        for layer in &clip.layers {
            let occurrence = occurrences.entry(layer.name.as_str()).or_default();
            let existing = self
                .layers
                .iter()
                .enumerate()
                .filter(|(_, made)| made.name == layer.name)
                .nth(*occurrence)
                .map(|(index, _)| index);
            *occurrence += 1;
            let target = existing.unwrap_or_else(|| {
                self.layers.push(layer.emptied());
                self.layers.len() - 1
            });
            targets.push(target);
        }
        self.targets.insert(variation, targets.clone());
        targets
    }

    /// The clip put together, `size` from its origin; `None` when it holds no tile.
    pub(crate) fn finish(self, size: IVec2) -> Option<TileClip> {
        let clip = TileClip {
            origin: self.origin,
            size,
            layers: self.layers,
            tilesets: self.tilesets.into_list(),
        };
        (!clip.is_empty()).then_some(clip)
    }
}

#[cfg(test)]
mod tests {
    use bt_core::TilesetRef;

    use super::*;
    use crate::testutil::{atlas, clip_grid, tile, trimmed};

    fn variation(names: &[&str], id: u32, tileset: &TilesetRef) -> TileClip {
        let layers = names
            .iter()
            .map(|&name| {
                let mut layer = ClipLayer::named(name);
                layer.put(IVec2::ZERO, tile(id));
                layer
            })
            .collect();
        TileClip {
            origin: IVec2::ZERO,
            size: IVec2::ONE,
            layers,
            tilesets: vec![tileset.clone()],
        }
    }

    #[test]
    fn layers_go_to_result_layers_by_name_and_occurrence() {
        let tileset = TilesetRef::embedded(atlas("a"));
        let first = variation(&["ground", "decor"], 1, &tileset);
        let second = variation(&["decor", "ground", "ground"], 2, &tileset);
        let mut composer = Composer::new(IVec2::new(10, 0));
        composer.place(0, &first, &Shape::of(&first), IVec2::new(10, 0), |_| true);
        composer.place(1, &second, &Shape::of(&second), IVec2::new(11, 0), |_| true);
        composer.place(0, &first, &Shape::of(&first), IVec2::new(12, 0), |at| {
            at.x != 12
        });
        let clip = composer.finish(IVec2::new(3, 1)).unwrap();
        let names: Vec<&str> = clip
            .layers
            .iter()
            .map(|layer| layer.name.as_str())
            .collect();
        assert_eq!(names, ["ground", "decor", "ground"]);
        assert_eq!(clip_grid(&clip, 0), trimmed("1 2 ."));
        assert_eq!(clip_grid(&clip, 1), trimmed("1 2 ."));
        assert_eq!(clip_grid(&clip, 2), trimmed(". 2 ."));
        assert_eq!(clip.tilesets.len(), 1, "one tileset, listed once");
        assert_eq!(Composer::new(IVec2::ZERO).finish(IVec2::ONE), None);
    }
}
