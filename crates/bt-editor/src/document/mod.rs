//! Documents: tabs onto assets.

mod areas;
mod carried;
#[allow(
    clippy::module_inception,
    reason = "the document module holds the document"
)]
mod document;
mod documents;
mod selecting;
mod selection;
mod tileset_editing;

pub(crate) use carried::{Carried, CarryStart, Reorient, carry_start, movable_layers};
pub use document::{Document, DocumentId, ViewState, ViewToggles};
pub use documents::Documents;
pub use selection::Selection;
pub use tileset_editing::{TilesetEditing, TilesetMode};
