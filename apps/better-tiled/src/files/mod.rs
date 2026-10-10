//! Files: what was dropped on the window, file dialogs, and the writer that keeps recovery
//! snapshots, the session and the heartbeat on disk.

mod drop;
mod pick;
mod writer;

pub(crate) use drop::dropped_files;
pub(crate) use pick::pick_file;
pub(crate) use writer::BackgroundWriter;
