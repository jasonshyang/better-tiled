//! The panels beside the canvas. Each draws from the editor's queries and its own state (which
//! tileset is shown, a name being edited) and returns intents; none changes the editor.

mod automap;
mod automap_explain;
mod automap_live;
mod custom_rows;
mod layers;
mod objects;
mod problem_line;
mod properties;
mod stamp_previews;
mod stamps;
mod terrains;
mod tile_grid;
mod tile_images;
mod tiles;
mod value_fields;

pub(crate) use automap::AutoMapPanel;
pub(crate) use custom_rows::{CustomEdit, CustomRows, RowsSetup};
pub(crate) use layers::LayersPanel;
pub(crate) use objects::ObjectsPanel;
pub(crate) use properties::PropertiesPanel;
pub(crate) use stamps::StampsPanel;
pub(crate) use terrains::TerrainsPanel;
pub(crate) use tile_grid::{ShownTiles, TileGrid, TileSource, cell_size, paint_tile};
pub(crate) use tile_images::TileImages;
pub(crate) use tiles::TilesPanel;
pub(crate) use value_fields::{Field, Fields};

mod property_draft;
pub(crate) use property_draft::PropertyDraft;

mod project;
pub(crate) use project::ProjectPanel;
