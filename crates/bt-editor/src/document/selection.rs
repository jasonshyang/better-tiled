//! What a document has selected: its current layer, the layers selected in the Layers panel,
//! the selected tiles and the selected objects.

use bt_core::{LayerId, ObjectId};
use bt_edit::TileRegion;

/// A document's selection. Part of every transaction, so undo brings it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// The layer tools act on.
    pub current_layer: Option<LayerId>,
    /// The layers selected in the Layers panel (the current one among them).
    pub layers: Vec<LayerId>,
    /// The selected cells, the same for every tile layer as in Tiled.
    pub tiles: TileRegion,
    /// The selected objects.
    pub objects: Vec<ObjectId>,
}

impl Selection {
    /// Only `layer`, current, with nothing else selected.
    pub fn of_layer(layer: LayerId) -> Selection {
        Selection {
            current_layer: Some(layer),
            layers: vec![layer],
            ..Selection::default()
        }
    }

    /// The same tiles and objects with `layer` current and the only layer selected.
    pub fn with_current_layer(&self, layer: LayerId) -> Selection {
        Selection {
            current_layer: Some(layer),
            layers: vec![layer],
            ..self.clone()
        }
    }
}
