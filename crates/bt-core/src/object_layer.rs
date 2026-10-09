//! Object layers.

use std::sync::Arc;

use crate::color::Color;
use crate::object::MapObject;

/// The payload of an object layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectLayer {
    /// The colour objects are drawn in when their class has none.
    pub color: Option<Color>,
    /// The order objects are drawn in.
    pub draw_order: DrawOrder,
    /// The objects, shared copy-on-write so that a snapshot of a tile edit does not copy them.
    pub objects: Arc<Vec<MapObject>>,
}

/// The order an object layer draws its objects in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrawOrder {
    /// By their position on screen, top to bottom (Tiled's default).
    #[default]
    TopDown,
    /// In the order of the list.
    Index,
}
