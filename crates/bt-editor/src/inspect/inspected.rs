//! What the Properties panel shows and changes for a document (Tiled's `currentObjects`:
//! `MapDocument::currentObjects` and `TilesetDocument::currentObjects`,
//! `reference/tiled/src/tiled/mapdocument.cpp` and `tilesetdocument.cpp`): while the tileset
//! editor is open, its selected tiles, or its tileset when none is selected; else the map, the
//! selected layers or the selected objects, whichever was chosen last.

use bt_core::{LayerId, ObjectId, TileId};

use crate::document::{DocumentId, Inspecting};
use crate::editor::Editor;

/// What the Properties panel shows and changes: things of one kind, of a document's map or of
/// the tileset its tileset editor edits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Inspected {
    /// The map.
    Map,
    /// Layers of the map: the selected ones.
    Layers(Vec<LayerId>),
    /// Objects of the map: the selected ones, none when nothing is selected.
    Objects(Vec<ObjectId>),
    /// The tileset the tileset editor edits.
    Tileset,
    /// Tiles of that tileset: the selected ones.
    Tiles(Vec<TileId>),
}

impl Inspected {
    /// Whether nothing is inspected: no layer or no object selected.
    pub fn is_empty(&self) -> bool {
        match self {
            Inspected::Map | Inspected::Tileset => false,
            Inspected::Layers(layers) => layers.is_empty(),
            Inspected::Objects(objects) => objects.is_empty(),
            Inspected::Tiles(tiles) => tiles.is_empty(),
        }
    }

    /// The tiles inspected; none for anything else.
    pub(crate) fn tiles(&self) -> &[TileId] {
        match self {
            Inspected::Tiles(tiles) => tiles,
            _ => &[],
        }
    }
}

impl Editor {
    /// What `doc`'s Properties panel shows: the tileset editor's selected tiles, or its tileset
    /// when none is selected, while it is open; else the map, the selected layers or the
    /// selected objects, whichever was chosen last. `None` for a document that is not open.
    pub fn inspected(&self, doc: DocumentId) -> Option<Inspected> {
        let document = self.documents.get(doc)?;
        if let Some(editing) = document.tileset_editing() {
            let tiles = editing.selected_tiles();
            return Some(if tiles.is_empty() {
                Inspected::Tileset
            } else {
                Inspected::Tiles(tiles.to_vec())
            });
        }
        let selection = document.selection();
        Some(match document.inspecting {
            Inspecting::Map => Inspected::Map,
            Inspecting::Layers => Inspected::Layers(selection.layers.clone()),
            Inspecting::Objects => Inspected::Objects(selection.objects.clone()),
        })
    }

    /// Makes `doc`'s Properties panel inspect the map, the selected layers or the selected
    /// objects (Map ▸ Map Properties, Layer ▸ Layer Properties, a choice in the Layers panel).
    pub(crate) fn set_inspecting(&mut self, doc: DocumentId, inspecting: Inspecting) {
        if let Some(document) = self.documents.get_mut(doc) {
            document.inspecting = inspecting;
        }
    }
}
