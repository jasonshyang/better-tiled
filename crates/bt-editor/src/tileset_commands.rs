//! The tileset editor's commands (`06-application.md`, §8): Edit Tileset shows a tileset of a
//! map in place of the map (Tiled opens it in a tab of its own; here an embedded tileset stays
//! with its map, whose history holds its changes), the tiles selected and the one the editors
//! show, the editors' modes, and a tile's animation set as one step. Every change is a
//! transaction on the tileset's home, recorded in the document that made it.

use std::sync::Arc;

use bt_core::{Frame, TileData, TileId, Tileset, TilesetSlot};

use crate::document::{DocumentId, TilesetEditing, TilesetMode};
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

impl Editor {
    /// The tileset `doc`'s tileset editor edits.
    pub fn edited_tileset(&self, doc: DocumentId) -> Option<&Arc<Tileset>> {
        let home = self.documents.get(doc)?.tileset_editing()?.home();
        self.store.tileset_at(home)
    }

    /// Edit Tileset: the tileset in `slot` of `doc`'s map, shown in place of the map.
    pub(crate) fn edit_tileset(&mut self, doc: DocumentId, slot: TilesetSlot) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let Some(home) = self.store.home_of_slot(asset, slot) else {
            return vec![Effect::Notify(Notice::error(
                "The tileset cannot be edited: its file is not open",
            ))];
        };
        if let Some(document) = self.documents.get_mut(doc) {
            document.tileset = Some(TilesetEditing::of(home));
            document.selection.objects.clear();
        }
        self.follow_document(doc);
        Vec::new()
    }

    /// Back to the map from its tileset editor; a tileset's own tab keeps its editor.
    pub(crate) fn stop_editing_tileset(&mut self, doc: DocumentId) -> Vec<Effect> {
        let in_map = self.map_of(doc).is_some();
        if let Some(document) = self.documents.get_mut(doc)
            && in_map
        {
            document.tileset = None;
            document.selection.objects.clear();
        }
        self.follow_document(doc);
        Vec::new()
    }

    /// Selects tiles in the tileset editor; `current` is the one its editors show. Tiles the
    /// tileset lacks are refused; another current tile drops the selected collision shapes.
    pub(crate) fn select_tileset_tiles(
        &mut self,
        doc: DocumentId,
        tiles: Vec<TileId>,
        current: Option<TileId>,
    ) -> Vec<Effect> {
        let Some(tileset) = self.edited_tileset(doc).cloned() else {
            return Vec::new();
        };
        if let Some(missing) = tiles
            .iter()
            .chain(&current)
            .find(|id| !tileset.has_tile(**id))
        {
            return vec![warn(EditError::NoSuchTile(*missing))];
        }
        if let Some(document) = self.documents.get_mut(doc)
            && let Some(editing) = &mut document.tileset
        {
            if editing.current != current {
                document.selection.objects.clear();
            }
            editing.tiles = tiles;
            editing.current = current;
        }
        Vec::new()
    }

    /// Shows another of the tileset editor's modes; the tools follow (the object tools for
    /// collision shapes).
    pub(crate) fn set_tileset_mode(&mut self, doc: DocumentId, mode: TilesetMode) -> Vec<Effect> {
        if let Some(document) = self.documents.get_mut(doc)
            && let Some(editing) = &mut document.tileset
        {
            if editing.mode != mode {
                document.selection.objects.clear();
            }
            editing.mode = mode;
        }
        self.follow_document(doc);
        Vec::new()
    }

    /// Sets a tile's animation, as one step; frames must show tiles of the same tileset.
    pub(crate) fn set_tile_animation(
        &mut self,
        doc: DocumentId,
        tile: TileId,
        frames: Vec<Frame>,
    ) -> Vec<Effect> {
        let Some(home) = self
            .documents
            .get(doc)
            .and_then(|d| d.tileset_editing())
            .map(|editing| editing.home())
        else {
            return Vec::new();
        };
        let result = self.transact(doc, "Change Tile Animation", |tx| {
            change_tile(tx.tileset_at_mut(home)?, tile, |data, tileset| {
                if let Some(missing) = frames.iter().find(|f| !tileset.has_tile(f.tile)) {
                    return Err(EditError::NoSuchTile(missing.tile));
                }
                data.animation = frames;
                Ok(())
            })
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Makes the tools follow what a document edits: the object tools for collision shapes, the
    /// current layer's family on a map.
    pub(crate) fn follow_document(&mut self, doc: DocumentId) {
        let family = self.tool_family(doc);
        self.tools.follow_layer(family);
    }
}

/// Changes a tile's data, made when it has none; data left as it would be made is removed, so
/// an atlas keeps entries only for tiles that have some.
pub(crate) fn change_tile(
    tileset: &mut Tileset,
    tile: TileId,
    change: impl FnOnce(&mut TileData, &Tileset) -> Result<(), EditError>,
) -> Result<(), EditError> {
    if !tileset.has_tile(tile) {
        return Err(EditError::NoSuchTile(tile));
    }
    let mut data = tileset.tile(tile).cloned().unwrap_or_default();
    change(&mut data, tileset)?;
    if data == TileData::default() && !tileset.is_collection() {
        tileset.tiles.remove(&tile);
    } else {
        tileset.tiles.insert(tile, data);
    }
    Ok(())
}
