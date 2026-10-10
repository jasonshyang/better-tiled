//! The crate's error type.

use std::path::PathBuf;

use bt_utils::Refused;

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
    /// The project file exists but could not be read.
    #[error("couldn't read {}: {source}", path.display())]
    Read {
        /// The project file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The project file is not JSON, or a key this version knows holds a value of the wrong
    /// kind.
    #[error("{} can't be read: {source}", path.display())]
    Parse {
        /// The project file.
        path: PathBuf,
        /// What is wrong, and where.
        #[source]
        source: serde_json::Error,
    },
    /// The project file's write lock could not be taken.
    #[error("couldn't lock {} for writing ({}): {source}", path.display(), lock.display())]
    Lock {
        /// The project file.
        path: PathBuf,
        /// The lock file, in the runtime folder.
        lock: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The document could not be turned into text.
    #[error("couldn't write {}: {source}", path.display())]
    Serialize {
        /// The project file.
        path: PathBuf,
        /// What `serde_json` said.
        #[source]
        source: serde_json::Error,
    },
    /// The project file was not written, and is as it was: a read-only file or folder
    /// ([`Error::refused`] says which), or another failure.
    #[error(transparent)]
    Write {
        /// What `bt-utils` said; it names the file.
        source: bt_utils::Error,
    },
}

impl Error {
    /// For a project file that was not written because of permissions: whether the file or its
    /// folder refused. The editor then still saves the map, with a notice that the painted state
    /// was not saved.
    pub fn refused(&self) -> Option<Refused> {
        match self {
            Error::Write { source } => source.refused(),
            Error::Resolve { .. }
            | Error::Unrelated { .. }
            | Error::Read { .. }
            | Error::Parse { .. }
            | Error::Lock { .. }
            | Error::Serialize { .. } => None,
        }
    }
}
