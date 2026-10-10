//! Editing objects from the editor: which objects the tools pick, dragging them as one step,
//! inserting them, and the Objects menu's commands.

mod commands;
mod host;
mod picking;
mod snapping;
mod stroke;

pub(crate) use commands::Restack;
pub(crate) use host::ObjectHost;
pub use picking::ObjectFrame;
pub use snapping::SnapToggle;
pub(crate) use stroke::ObjectStrokeState;
pub use stroke::{ObjectStroke, ObjectStrokeKind};
