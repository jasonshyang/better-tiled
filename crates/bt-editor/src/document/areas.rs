//! The areas the selection tools preview: worked out here, so the window shows exactly what
//! the tool will select.

use bt_core::{Cell, LayerKind};
use bt_edit::{TileRegion, magic_wand, select_same_tile};
use glam::IVec2;

use super::document::DocumentId;
use crate::editor::Editor;

impl Editor {
    /// The Magic Wand's area from `at` on the current tile layer of `doc`: the connected
    /// cells holding what `at` holds or any of `extra`. Empty off a finite layer or when the
    /// current layer holds no tiles.
    pub fn wand_area(&self, doc: DocumentId, at: IVec2, extra: &[Cell]) -> TileRegion {
        let Some(map) = self.map_of(doc) else {
            return TileRegion::new();
        };
        match self.current_tiles(doc) {
            Some(layer) if layer.contains(at) => magic_wand(layer, at, extra, &map.geometry()),
            _ => TileRegion::new(),
        }
    }

    /// Select Same Tile's area: every cell of the current tile layer holding what `at` holds or
    /// any of `extra`.
    pub fn same_tile_area(&self, doc: DocumentId, at: IVec2, extra: &[Cell]) -> TileRegion {
        match self.current_tiles(doc) {
            Some(layer) if layer.contains(at) => {
                let mut cells = extra.to_vec();
                cells.push(layer.cell(at));
                select_same_tile(layer, &cells)
            }
            _ => TileRegion::new(),
        }
    }

    /// The cells of the current layer of `doc`, when it is a tile layer.
    fn current_tiles(&self, doc: DocumentId) -> Option<&bt_core::TileLayer> {
        let id = self.document(doc)?.selection().current_layer?;
        match &self.map_of(doc)?.layer(id)?.kind {
            LayerKind::Tile(tiles) => Some(tiles),
            _ => None,
        }
    }
}
