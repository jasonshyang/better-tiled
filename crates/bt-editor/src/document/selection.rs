//! What a document has selected: its current layer, the layers selected in the Layers panel,
//! the selected tiles (and the tiles they carry) and the selected objects.

use bt_core::{LayerId, ObjectId};
use bt_edit::TileRegion;

use super::carried::Carried;

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
    /// The tiles the selected cells carry, lifted off the map and moved, mirrored or turned
    /// with them; their cells are `tiles`.
    pub(crate) carried: Option<Carried>,
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

    /// The same tiles and objects with `layer` current and the only layer selected; carried
    /// tiles are let go unless their layers stay the selected ones.
    pub fn with_current_layer(&self, layer: LayerId) -> Selection {
        self.with_layers(layer, &[layer])
    }

    /// The same tiles and objects with `current` current and `layers` selected, each once in
    /// the order given, `current` among them (last, when they do not name it); carried tiles
    /// are let go unless their layers stay the selected ones.
    pub fn with_layers(&self, current: LayerId, layers: &[LayerId]) -> Selection {
        let mut selected: Vec<LayerId> = Vec::with_capacity(layers.len() + 1);
        for &id in layers.iter().chain([&current]) {
            if !selected.contains(&id) {
                selected.push(id);
            }
        }
        let mut selection = Selection {
            current_layer: Some(current),
            layers: selected,
            ..self.clone()
        };
        selection.drop_stale_carry();
        selection
    }

    /// Whether the selected cells carry tiles (moved, mirrored or turned since they were
    /// selected).
    pub fn carries_tiles(&self) -> bool {
        self.carried.is_some()
    }

    /// Lets carried tiles go, where they are, when the selected cells or layers are no longer
    /// theirs.
    pub(crate) fn drop_stale_carry(&mut self) {
        if self
            .carried
            .as_ref()
            .is_some_and(|carried| !carried.belongs_to(self))
        {
            self.carried = None;
        }
    }
}
