//! Editing objects from the editor: which objects the tools pick, dragging them as one step,
//! inserting them, and the Objects menu's commands.

mod commands;
mod picking;
mod stroke;

pub(crate) use commands::Restack;
pub use picking::ObjectFrame;
pub(crate) use stroke::ObjectStrokeState;
pub use stroke::{ObjectStroke, ObjectStrokeKind};
