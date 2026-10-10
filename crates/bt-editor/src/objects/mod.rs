//! Editing objects from the editor: which objects the tools pick, dragging them as one step,
//! inserting them, the Objects menu's commands, and what the Objects panel lists and asks.

mod commands;
mod handles;
mod host;
mod listing;
mod picking;
mod snapping;
mod stroke;

pub(crate) use commands::Restack;
pub use handles::HandleFrame;
pub(crate) use host::ObjectHost;
pub use listing::ObjectRow;
pub use picking::ObjectFrame;
pub use snapping::SnapToggle;
pub(crate) use stroke::ObjectStrokeState;
pub use stroke::{ObjectStroke, ObjectStrokeKind};
