//! Documents: tabs onto assets.

#[allow(
    clippy::module_inception,
    reason = "the document module holds the document"
)]
mod document;
mod documents;
mod selection;

pub use document::{Document, DocumentId, ViewState, ViewToggles};
pub use documents::Documents;
pub use selection::Selection;
