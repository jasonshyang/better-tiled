//! The panels beside the canvas. Each draws from the editor's queries and its own state (which
//! tileset is shown, a name being edited) and returns intents; none changes the editor.

mod custom_rows;
mod layers;
mod objects;
mod properties;
mod tile_grid;
mod tile_images;
mod tiles;
mod value_fields;

pub(crate) use layers::LayersPanel;
pub(crate) use objects::ObjectsPanel;
pub(crate) use properties::PropertiesPanel;
pub(crate) use tile_grid::{ShownTiles, TileGrid, TileSource, cell_size, paint_tile};
pub(crate) use tile_images::TileImages;
pub(crate) use tiles::TilesPanel;
