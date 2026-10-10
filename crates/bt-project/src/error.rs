//! The crate's error type.

use std::path::PathBuf;

/// Why a project file could not be found, read or written. Every one names the file or path it
/// is about; callers stop on it rather than starting afresh.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A path could not be made absolute (the working folder is gone, say).
    #[error(transparent)]
    Resolve {
        /// What `bt-utils` said.
        source: bt_utils::Error,
    },
    /// A path has no key in a project file: no relative path leads from the file's folder to it
    /// (it is on another Windows drive).
    #[error("{} can't be named from {}", path.display(), file.display())]
    Unrelated {
        /// The project file.
        file: PathBuf,
        /// The path.
        path: PathBuf,
    },
}
