//! A tile selection that carries its tiles (`05-editor-core.md`, §4): the map as the lift left
//! it and the tiles where they are now. The carry is part of the selection, so a transaction
//! records it and undo brings it back with the selection; the map always holds the tiles where
//! they are, so everything else sees a normal map. Dragging inside the selection moves the
//! tiles, X and Y mirror them and Z turns them; any other edit, or other selected cells or
//! layers, lets them go where they are, and what they cover stays covered.

use std::sync::Arc;

use bt_core::{LayerId, Map};
use bt_edit::{Axis, FloatingTiles, RotateDirection};

use super::document::DocumentId;
use super::selection::Selection;
use crate::clipboard::clip_layers;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;

/// Tiles a selection carries.
#[derive(Clone, Debug)]
pub struct Carried {
    /// The map as the lift left it: without the tiles.
    pub(crate) base: Arc<Map>,
    /// The tiles where they are now.
    pub(crate) tiles: FloatingTiles,
    /// The layers selected when the tiles were lifted.
    pub(crate) layers: Vec<LayerId>,
}

/// Two carries are the same when they keep the same base and the same tiles in the same place.
impl PartialEq for Carried {
    fn eq(&self, other: &Carried) -> bool {
        Arc::ptr_eq(&self.base, &other.base)
            && self.tiles == other.tiles
            && self.layers == other.layers
    }
}

impl Eq for Carried {}

impl Carried {
    /// The same carry with the tiles elsewhere.
    pub(crate) fn with_tiles(&self, tiles: FloatingTiles) -> Carried {
        Carried {
            base: Arc::clone(&self.base),
            tiles,
            layers: self.layers.clone(),
        }
    }

    /// The map with the tiles placed where they are now.
    pub(crate) fn placed(&self) -> Result<Map, EditError> {
        let mut map = (*self.base).clone();
        self.tiles.place(&mut map)?;
        Ok(map)
    }

    /// Whether the selection still carries these tiles: its cells and layers are theirs.
    pub(crate) fn belongs_to(&self, selection: &Selection) -> bool {
        self.layers == selection.layers && self.tiles.region() == selection.tiles
    }
}

/// The tiles a change of the selected tiles starts from: those carried already, or those the
/// selected unlocked tile layers have in the selection, lifted off the map.
#[derive(Clone, Debug)]
pub(crate) struct CarryStart {
    /// The tiles, where they are.
    pub(crate) carried: Carried,
    /// The locked layers that kept their tiles when they were lifted.
    pub(crate) locked: Vec<LayerId>,
    /// The tiles were lifted for this change: the selection did not carry them before.
    pub(crate) lifted: bool,
}

/// The carry the selected tiles of `map` start from; `None` when no selected tile layer can let
/// its tiles go (every one is locked, or none is selected), with the locked ones.
pub(crate) fn carry_start(map: &Arc<Map>, selection: &Selection) -> Result<CarryStart, EditError> {
    if let Some(carried) = &selection.carried {
        return Ok(CarryStart {
            carried: carried.clone(),
            locked: Vec::new(),
            lifted: false,
        });
    }
    let mut base = (**map).clone();
    let layers = clip_layers(map, selection);
    let lift = FloatingTiles::lift(&mut base, &layers, &selection.tiles)?;
    Ok(CarryStart {
        carried: Carried {
            base: Arc::new(base),
            tiles: lift.tiles,
            layers: selection.layers.clone(),
        },
        locked: lift.erased.skipped_locked,
        lifted: true,
    })
}

/// The selected tile layers of `map` whose tiles can move: unlocked ones.
pub(crate) fn movable_layers(map: &Map, selection: &Selection) -> Vec<LayerId> {
    clip_layers(map, selection)
        .into_iter()
        .filter(|id| {
            map.layer(*id).is_some_and(|l| l.as_tile_layer().is_some())
                && map.effective_layer(*id).is_some_and(|l| !l.locked)
        })
        .collect()
}

/// A mirror or a turn of the selected tiles about their centre.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reorient {
    /// Mirrored on screen.
    Flip(Axis),
    /// Turned a quarter (a sixth on hexagonal maps).
    Rotate(RotateDirection),
}

impl Editor {
    /// Mirrors or turns the selected tiles about their centre, lifting them first when they are
    /// not carried yet, as one step.
    pub(crate) fn reorient_selected_tiles(
        &mut self,
        doc: DocumentId,
        change: Reorient,
    ) -> Vec<Effect> {
        let (Some(map), Some(asset), Some(selection)) = (
            self.map_of(doc).cloned(),
            self.documents.get(doc).map(|d| d.asset()),
            self.documents.get(doc).map(|d| d.selection().clone()),
        ) else {
            return Vec::new();
        };
        if selection.carried.is_none() && movable_layers(&map, &selection).is_empty() {
            return self
                .locked_notice(doc, &clip_layers(&map, &selection))
                .into_iter()
                .collect();
        }
        let label = match change {
            Reorient::Flip(_) => "Flip Tiles",
            Reorient::Rotate(_) => "Rotate Tiles",
        };
        let result = self.transact(doc, label, |tx| {
            let start = carry_start(&map, &tx.selection)?;
            let pivot = start.carried.tiles.centre();
            let tiles = match change {
                Reorient::Flip(axis) => start.carried.tiles.flipped(axis, pivot),
                Reorient::Rotate(direction) => start.carried.tiles.rotated(direction, pivot),
            };
            let carried = start.carried.with_tiles(tiles);
            *tx.map_mut(asset)? = carried.placed()?;
            tx.carry(carried);
            Ok::<_, EditError>(start.locked)
        });
        match result {
            Ok(locked) => self.locked_notice(doc, &locked).into_iter().collect(),
            Err(error) => vec![warn(error)],
        }
    }

    /// Drops the carried tiles, showing what they covered (Delete and Cut while carrying), as
    /// one step; `false` when the selection carries none.
    pub(crate) fn drop_carried_tiles(
        &mut self,
        doc: DocumentId,
        label: &str,
    ) -> Result<bool, EditError> {
        let Some(document) = self.documents.get(doc) else {
            return Ok(false);
        };
        let Some(carried) = document.selection().carried.clone() else {
            return Ok(false);
        };
        let asset = document.asset();
        self.transact(doc, label, |tx| {
            *tx.map_mut(asset)? = (*carried.base).clone();
            Ok::<_, EditError>(())
        })?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    #[test]
    fn a_carry_belongs_to_the_selection_with_its_cells_and_layers() {
        let atlas = Atlas::builder().build_embedded().unwrap();
        let map = Arc::new(
            MapBuilder::new(UVec2::splat(16), UVec2::new(3, 2))
                .tileset(atlas.tileset)
                .tile_layer("ground", "1 2 .\n. . .")
                .build()
                .unwrap(),
        );
        let ground = map.layers[0].id;
        let selection = Selection {
            tiles: bt_edit::TileRegion::from_rect(bt_utils::IRect::from_min_size(
                glam::IVec2::ZERO,
                glam::IVec2::new(2, 1),
            )),
            ..Selection::of_layer(ground)
        };
        let start = carry_start(&map, &selection).unwrap();
        assert!(start.locked.is_empty());
        let carried = start.carried;
        assert!(carried.belongs_to(&selection));
        assert_eq!(
            carried.placed().unwrap(),
            *map,
            "lifted and placed: as it was"
        );
        let moved = carried.with_tiles(carried.tiles.moved(glam::IVec2::new(1, 1)));
        assert!(!moved.belongs_to(&selection), "the cells moved on");
        assert_ne!(moved, carried);
        assert_eq!(moved.with_tiles(carried.tiles.clone()), carried);
    }
}
