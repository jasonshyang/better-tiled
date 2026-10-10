//! Where the object tools edit objects: a map's object layers, or the collision shapes of the
//! tile the tileset editor shows in its Collision mode (Tiled's tile collision editor,
//! `reference/tiled/src/tiled/tilecollisiondock.cpp`). A tile's shapes live in its own pixel
//! space (`bt_edit::ObjectSpace::of_tile`), with ids unique in the tile and no locks; a change
//! to them is a change to the tileset's home, and a tile left without shapes has no collision
//! group, as in Tiled.

use std::sync::Arc;

use bt_core::{ObjectLayer, TileId, Tileset};
use bt_edit::ObjectSpace;

use crate::document::{DocumentId, TilesetMode};
use crate::editor::Editor;
use crate::error::EditError;
use crate::history::Tx;
use crate::store::{AssetId, TilesetHome};
use crate::tileset_commands::change_tile;

/// Where a document's object tools edit objects.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ObjectHost {
    /// The object layers of the document's map.
    Map(AssetId),
    /// The collision shapes of a tile.
    Tile {
        /// Where its tileset lives.
        home: TilesetHome,
        /// The tile.
        tile: TileId,
    },
}

/// A tile's collision shapes as the tools see them.
#[derive(Clone, Debug)]
pub(crate) struct TileShapes {
    /// The tile's tileset.
    pub(crate) tileset: Arc<Tileset>,
    /// The tile.
    pub(crate) tile: TileId,
    /// Its shapes; none when it has no collision group.
    pub(crate) shapes: ObjectLayer,
}

impl TileShapes {
    /// The space the shapes live in.
    pub(crate) fn space(&self) -> ObjectSpace<'static> {
        ObjectSpace::of_tile(&self.tileset, self.tile)
    }
}

impl Editor {
    /// Where `doc`'s object tools edit objects now: the current tile's collision shapes in the
    /// tileset editor's Collision mode, the map's object layers when no tileset editor is open,
    /// nowhere otherwise.
    pub(crate) fn object_host(&self, doc: DocumentId) -> Option<ObjectHost> {
        let document = self.documents.get(doc)?;
        match document.tileset_editing() {
            Some(editing) if editing.mode() == TilesetMode::Collision => Some(ObjectHost::Tile {
                home: editing.home(),
                tile: editing.current_tile()?,
            }),
            Some(_) => None,
            None => {
                self.store.map(document.asset())?;
                Some(ObjectHost::Map(document.asset()))
            }
        }
    }

    /// A tile's collision shapes.
    pub(crate) fn tile_shapes(&self, home: TilesetHome, tile: TileId) -> Option<TileShapes> {
        let tileset = Arc::clone(self.store.tileset_at(home)?);
        let shapes = tileset
            .tile(tile)
            .and_then(|data| data.collision.as_ref())
            .map(|group| group.layer.clone())
            .unwrap_or_default();
        Some(TileShapes {
            tileset,
            tile,
            shapes,
        })
    }
}

/// Changes a tile's collision shapes in a transaction: its group is made when it has none and
/// dropped when it is left empty.
pub(crate) fn change_tile_shapes<R>(
    tx: &mut Tx<'_>,
    home: TilesetHome,
    tile: TileId,
    change: impl FnOnce(&mut ObjectLayer, &ObjectSpace<'_>) -> Result<R, bt_edit::EditError>,
) -> Result<R, EditError> {
    let tileset = tx.tileset_at_mut(home)?;
    let space = ObjectSpace::of_tile(tileset, tile);
    let mut outcome = None;
    change_tile(tileset, tile, |data, _| {
        let mut group = data.collision.take().unwrap_or_default();
        outcome = Some(change(&mut group.layer, &space)?);
        if !group.layer.objects.is_empty() {
            data.collision = Some(group);
        }
        Ok(())
    })?;
    outcome.ok_or(EditError::NoSuchTile(tile))
}
