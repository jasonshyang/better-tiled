//! The clipboard and its commands on tiles: Copy and Cut put the selected tiles of the selected
//! layers aside, Paste makes them the brush (as Tiled's Paste does), Paste in Place puts them
//! back where they were copied from, and Delete empties the selection. Locked layers are left
//! alone and named.

use bt_core::{LayerId, Map};
use bt_edit::{Brush, TileClip, TileRegion, copy, cut, erase, find_target_layers, paste};

use crate::document::{DocumentId, Selection};
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;

/// What was copied, kept editor-wide so it pastes into any document.
#[derive(Clone, Debug, Default)]
pub(crate) struct Clipboard {
    /// Tiles.
    pub(crate) tiles: Option<TileClip>,
}

impl Editor {
    /// Copy: the selected tiles of the selected tile layers.
    pub(crate) fn copy_tiles(&mut self, doc: DocumentId) -> Vec<Effect> {
        let (Some(map), selection) = (self.map_of(doc).cloned(), self.selection_or_default(doc))
        else {
            return Vec::new();
        };
        match copy(&map, &clip_layers(&map, &selection), &selection.tiles) {
            Ok(clip) => {
                self.clipboard.tiles = Some(clip);
                Vec::new()
            }
            Err(error) => vec![warn(error.into())],
        }
    }

    /// Cut: Copy, and the tiles are erased from the unlocked layers, as one step.
    pub(crate) fn cut_tiles(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selection = self.selection_or_default(doc);
        let result = self.transact(doc, "Cut", |tx| {
            let map = tx.map_mut(asset)?;
            let layers = clip_layers(map, &selection);
            cut(map, &layers, &selection.tiles).map_err(EditError::from)
        });
        match result {
            Ok(taken) => {
                self.clipboard.tiles = Some(taken.clip);
                Vec::new()
            }
            Err(error) => vec![warn(error)],
        }
    }

    /// Delete: the selected tiles of the selected unlocked tile layers are erased, as one step.
    pub(crate) fn delete_tiles(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selection = self.selection_or_default(doc);
        let result = self.transact(doc, "Delete", |tx| {
            let map = tx.map_mut(asset)?;
            let layers = clip_layers(map, &selection);
            erase(map, &layers, &selection.tiles).map_err(EditError::from)
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Paste: what was copied becomes the brush, cut to its tiles.
    pub(crate) fn paste_tiles(&mut self) -> Vec<Effect> {
        let Some(clip) = self.clipboard.tiles.clone() else {
            return Vec::new();
        };
        match Brush::from_clip(clip.trimmed()) {
            Ok(brush) => {
                self.set_brush(brush);
                Vec::new()
            }
            Err(error) => vec![warn(error.into())],
        }
    }

    /// Paste in Place: what was copied, painted back where it was copied from, as one step.
    pub(crate) fn paste_tiles_in_place(&mut self, doc: DocumentId) -> Vec<Effect> {
        let (Some(clip), Some(asset)) = (
            self.clipboard.tiles.clone(),
            self.documents.get(doc).map(|d| d.asset()),
        ) else {
            return Vec::new();
        };
        let selection = self.selection_or_default(doc);
        let result = self.transact(doc, "Paste in Place", |tx| {
            let map = tx.map_mut(asset)?;
            let targets =
                find_target_layers(map, &clip, selection.current_layer, &selection.layers)
                    .map_err(EditError::from)?;
            paste(map, &clip, clip.origin, &targets, &TileRegion::new()).map_err(EditError::from)
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Whether tiles were copied.
    pub(crate) fn has_copied_tiles(&self) -> bool {
        self.clipboard.tiles.is_some()
    }

    /// Why Cut or Delete would leave the selected tiles where they are: every layer they would
    /// come from is locked.
    pub(crate) fn tiles_locked_reason(&self, doc: DocumentId, verb: &str) -> Option<String> {
        let map = self.map_of(doc)?;
        let selection = self.selection_or_default(doc);
        let layers = clip_layers(map, &selection);
        let locked = |id: &LayerId| map.effective_layer(*id).is_some_and(|l| l.locked);
        if layers.is_empty() || !layers.iter().all(locked) {
            return None;
        }
        let names: Vec<String> = layers
            .iter()
            .filter_map(|id| map.layer(*id).map(|l| l.name.clone()))
            .collect();
        let quoted = bt_utils::text::quoted_list(&names);
        Some(if names.len() == 1 {
            format!("{quoted} is locked: unlock it to {verb} its tiles")
        } else {
            format!("{quoted} are locked: unlock them to {verb} their tiles")
        })
    }

    fn selection_or_default(&self, doc: DocumentId) -> Selection {
        self.documents
            .get(doc)
            .map(|d| d.selection().clone())
            .unwrap_or_default()
    }
}

/// The layers Copy, Cut and Delete take tiles from: the selected tile layers, or the current
/// layer when no tile layer is selected (as in Tiled).
fn clip_layers(map: &Map, selection: &Selection) -> Vec<LayerId> {
    let selected: Vec<LayerId> = selection
        .layers
        .iter()
        .copied()
        .filter(|id| map.layer(*id).and_then(|l| l.as_tile_layer()).is_some())
        .collect();
    if selected.is_empty() {
        selection.current_layer.into_iter().collect()
    } else {
        selected
    }
}
