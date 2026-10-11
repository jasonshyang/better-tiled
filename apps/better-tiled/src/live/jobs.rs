//! What a connection asks the window to do, and what the window answers.

use std::path::PathBuf;

use bt_editor::{ExternalBase, ExternalEdit, ExternalEditError, MarkCells};
use bt_live::Response;
use bt_utils::IRect;

/// A job for the window's thread.
#[derive(Debug)]
pub(crate) enum LiveJob {
    /// The editor's open documents.
    Status,
    /// A map as the editor shows it, as text.
    Snapshot {
        /// The map's canonical path.
        path: PathBuf,
    },
    /// What a script runs on, for the map at `path`.
    Prepare {
        /// The map's canonical path.
        path: PathBuf,
    },
    /// A script's result to land as one step; it waits while a stroke is under way.
    Apply {
        /// The result.
        edit: Box<ExternalEdit>,
        /// The cells it changed, outlined on the canvas while the outline fades.
        area: Option<IRect>,
    },
    /// The last external edit of a map taken back.
    Undo {
        /// The map's canonical path.
        path: PathBuf,
    },
    /// Cells outlined on a map's canvas.
    Mark {
        /// The map's canonical path.
        path: PathBuf,
        /// The marks.
        marks: Vec<MarkCells>,
        /// Whether they join the map's marks.
        add: bool,
        /// Whether the view moves to show them.
        focus: bool,
    },
    /// A map opened, or shown when it is open.
    Open {
        /// The map's canonical path.
        path: PathBuf,
    },
    /// A map saved through the editor's own save.
    Save {
        /// The map's canonical path.
        path: PathBuf,
    },
    /// The window's picture, written to `out`.
    Screenshot {
        /// The PNG to write.
        out: PathBuf,
    },
}

/// The window's answer to a job, or why it could not do it.
pub(crate) type LiveAnswer = Result<Answered, String>;

/// What the window answers.
#[derive(Debug)]
pub(crate) enum Answered {
    /// An answer for `bt` as it is.
    Response(Response),
    /// What a script runs on.
    Base(Box<ExternalBase>),
    /// Whether the script's result landed.
    Applied(Result<(), ExternalEditError>),
}
