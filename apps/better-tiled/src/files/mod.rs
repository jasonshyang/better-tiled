//! Files: what was dropped on the window, file dialogs, the watcher, and the writer that keeps
//! recovery snapshots, the session and the heartbeat on disk.

mod drop;
mod pick;
mod watcher;
mod writer;

pub(crate) use drop::dropped_files;
pub(crate) use pick::FileDialogs;
pub(crate) use watcher::{WATCH_INTERVAL, Watcher};
pub(crate) use writer::BackgroundWriter;
