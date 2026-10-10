//! The panels beside the canvas. Each draws from the editor's queries and its own state (which
//! tileset is shown, a name being edited) and returns intents; none changes the editor.

mod layers;
mod tile_grid;
mod tile_images;
mod tiles;

pub(crate) use layers::LayersPanel;
pub(crate) use tile_grid::{TileGrid, TileSource, cell_size, paint_tile};
pub(crate) use tile_images::TileImages;
pub(crate) use tiles::TilesPanel;
