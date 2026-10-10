//! The tools' editor-wide state.

mod kind;
mod painting;
mod state;
mod terrain;

pub use kind::{ToolFamily, ToolKind};
pub(crate) use painting::StrokeState;
pub use painting::{FillPreview, Stroke, StrokeKind};
pub use state::{MAX_BRUSH_SIZE, ToolState};
pub(crate) use terrain::TerrainStrokeState;
pub use terrain::{NoTerrain, TerrainChoice, TerrainInUse, TerrainStroke};
