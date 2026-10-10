//! What a document has selected: its current layer and the layers selected in the Layers panel.

use bt_core::LayerId;

/// A document's selection. Part of every transaction, so undo brings it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// The layer tools act on.
    pub current_layer: Option<LayerId>,
    /// The layers selected in the Layers panel (the current one among them).
    pub layers: Vec<LayerId>,
}

impl Selection {
    /// Only `layer`, current.
    pub fn of_layer(layer: LayerId) -> Selection {
        Selection {
            current_layer: Some(layer),
            layers: vec![layer],
        }
    }
}
