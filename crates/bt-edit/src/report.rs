//! What operations report: what they changed, made, removed and left alone, so the editor can
//! tell the user and the live link can answer. Each is a value with named fields.

use bt_core::{LayerId, ObjectId, TilesetRef, TilesetSlot};
use bt_utils::IRect;
use glam::DVec2;

/// The cells of a layer an operation may have changed: the bounding rectangle of what it wrote.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayerChange {
    /// The layer.
    pub layer: LayerId,
    /// The cells.
    pub rect: IRect,
}

/// What a paste did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PasteReport {
    /// The cells written, per layer.
    pub changed: Vec<LayerChange>,
    /// The layers made for clip layers that had no target.
    pub created_layers: Vec<LayerId>,
    /// The tilesets added to the map for the clip's tiles.
    pub added_tilesets: Vec<TilesetSlot>,
    /// The targets left alone because they are locked.
    pub skipped_locked: Vec<LayerId>,
}

/// What an erase did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EraseReport {
    /// The cells emptied, per layer.
    pub changed: Vec<LayerChange>,
    /// The layers left alone because they are locked.
    pub skipped_locked: Vec<LayerId>,
}

/// What a resize did besides changing the size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResizeReport {
    /// The objects removed because they ended up outside the map.
    pub removed_objects: Vec<ObjectId>,
    /// How far objects and image layers moved, in pixels.
    pub pixel_offset: DVec2,
}

/// What a layer command did to the layer tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayersReport {
    /// The layers made, in the order they were made.
    pub created: Vec<LayerId>,
    /// The layers taken out of the map (a removed group takes its children with it).
    pub removed: Vec<LayerId>,
    /// The layers that changed or moved, in the order they did.
    pub changed: Vec<LayerId>,
}

impl LayersReport {
    /// Whether the command changed nothing.
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// A tileset taken out of a map, with what went with it.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedTileset {
    /// The tileset as the map referred to it.
    pub tileset: TilesetRef,
    /// How many cells were emptied.
    pub cells: usize,
    /// The tile objects removed.
    pub objects: Vec<ObjectId>,
}

/// The objects an operation changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectsReport {
    /// Their ids, in the order they were changed.
    pub changed: Vec<ObjectId>,
}
