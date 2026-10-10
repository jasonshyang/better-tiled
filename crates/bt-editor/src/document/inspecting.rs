//! What a map document's Properties panel inspects: the map, the selected layers or the
//! selected objects, whichever was chosen last (Tiled's current object, `Document::
//! setCurrentObject` and `MapDocument::currentObjects`, `reference/tiled/src/tiled/document.cpp`
//! and `mapdocument.cpp`). A map document starts on its map, as Tiled's does; selecting objects
//! inspects them (`MapDocument::setSelectedObjects` makes the first of them current); choosing
//! layers in the Layers panel inspects the selected layers (`LayerView::indexPressed`); Map ▸ Map
//! Properties and Layer ▸ Layer Properties choose the map or the layers.

/// What a map document's Properties panel inspects.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Inspecting {
    /// The map.
    #[default]
    Map,
    /// The selected layers.
    Layers,
    /// The selected objects.
    Objects,
}
