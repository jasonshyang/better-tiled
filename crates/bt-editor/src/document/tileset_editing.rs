//! A document's tileset editor (`06-application.md`, §8): the tileset it edits, the tiles
//! selected in it, the one its collision and animation editors show, and which of them is open.
//! A tileset document always has one; a map document has one while Edit Tileset shows a tileset
//! of the map in place of the map.

use bt_core::TileId;

use crate::store::TilesetHome;

/// What the tileset editor shows of its tiles.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TilesetMode {
    /// The tiles, to select them and see their properties.
    #[default]
    Tiles,
    /// The current tile's collision shapes, edited with the object tools.
    Collision,
    /// The current tile's animation.
    Animation,
}

/// What a document's tileset editor edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilesetEditing {
    pub(crate) home: TilesetHome,
    pub(crate) tiles: Vec<TileId>,
    pub(crate) current: Option<TileId>,
    pub(crate) mode: TilesetMode,
}

impl TilesetEditing {
    /// The editor of the tileset at `home`, with nothing selected.
    pub(crate) fn of(home: TilesetHome) -> TilesetEditing {
        TilesetEditing {
            home,
            tiles: Vec::new(),
            current: None,
            mode: TilesetMode::Tiles,
        }
    }

    /// Where the tileset lives.
    pub fn home(&self) -> TilesetHome {
        self.home
    }

    /// The tiles selected.
    pub fn selected_tiles(&self) -> &[TileId] {
        &self.tiles
    }

    /// The tile the collision and animation editors show: the one chosen last.
    pub fn current_tile(&self) -> Option<TileId> {
        self.current
    }

    /// What is shown of the tiles.
    pub fn mode(&self) -> TilesetMode {
        self.mode
    }
}
