//! What the editor asks the window to do: things that block, things that need a person, and
//! things only the window can do.

use std::path::PathBuf;

use crate::document::DocumentId;
use crate::files::LoadJob;
use crate::notice::Notice;
use crate::recovery::Snapshot;

/// A request to the window.
#[derive(Clone, Debug)]
pub enum Effect {
    /// Run a load on a worker and hand the result back as [`crate::Intent::Loaded`].
    Load(LoadJob),
    /// Ask for a file; the answer comes back as [`crate::Intent::FilePicked`].
    PickFile(FileRequest),
    /// Ask a question; the answer comes back as [`crate::Intent::Decided`].
    Ask(Decision),
    /// Tell the user something.
    Notify(Notice),
    /// Move the current document's camera.
    View(ViewRequest),
    /// Write a recovery snapshot on a worker.
    WriteSnapshot(Snapshot),
    /// Delete a recovery snapshot and its description.
    DeleteSnapshot(PathBuf),
    /// Images changed on disk: decode them again.
    ReloadImages(Vec<PathBuf>),
    /// Show a dialog of the window's; it reports back with intents.
    Show(Dialog),
    /// Close the window: nothing unsaved is left, or the user chose to lose it.
    Quit,
}

/// A dialog of the window's.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// New Map.
    NewMap,
    /// New Tileset.
    NewTileset,
    /// The preferences.
    Preferences,
    /// The command palette.
    SearchActions,
}

/// A file to ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRequest {
    /// Files to open.
    Open,
    /// Where to save a document.
    SaveAs {
        /// The document.
        doc: DocumentId,
        /// The file suggested.
        suggested: PathBuf,
    },
}

/// A camera move the window carries out.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ViewRequest {
    /// Zoom in a step.
    ZoomIn,
    /// Zoom out a step.
    ZoomOut,
    /// Zoom to 100 %.
    ZoomNormal,
    /// Fit the map in the view.
    Fit,
}

/// A question's identity.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DecisionId(pub u64);

/// A question for the user, with the answers they may give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// Its identity.
    pub id: DecisionId,
    /// The question.
    pub question: Question,
    /// The answers, the default first.
    pub choices: Vec<Choice>,
}

/// What is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// Work is unsaved: save it before closing or quitting?
    Unsaved {
        /// The names of what is unsaved.
        names: Vec<String>,
        /// Why it is asked.
        purpose: Purpose,
    },
    /// Unsaved work of a run that did not end cleanly was found: open it again?
    Recover {
        /// The names of the files.
        names: Vec<String>,
    },
    /// A file with unsaved changes changed on disk (or came back changed after it was gone):
    /// keep the changes, or take what is on disk?
    Conflict {
        /// The file's name.
        name: String,
        /// Whether the file had been gone.
        returned: bool,
    },
}

/// Why unsaved work is asked about.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A document is closing.
    Close,
    /// The editor is quitting.
    Quit,
}

/// An answer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Save first.
    Save,
    /// Go on and lose the changes.
    DontSave,
    /// Do nothing.
    Cancel,
    /// Open the snapshots again.
    Recover,
    /// Delete the snapshots.
    Discard,
    /// Keep the unsaved changes over what is on disk.
    KeepMine,
    /// Take what is on disk, losing the unsaved changes (the reload can be undone).
    Reload,
}
