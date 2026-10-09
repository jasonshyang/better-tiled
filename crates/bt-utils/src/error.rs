//! The crate's error type.

use std::path::PathBuf;

/// Why something in this crate failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file was not written because permissions did not allow it.
    #[error("{}: {reason}", path.display())]
    Refused {
        /// The file that was not written.
        path: PathBuf,
        /// Whether the file or its folder refused.
        reason: Refused,
    },
    /// Writing a file failed for another reason; the file is as it was.
    #[error("couldn't write {}: {source}", path.display())]
    Write {
        /// The file that was not written.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// A path could not be made absolute (the working folder is gone, say).
    #[error("couldn't resolve {}: {source}", path.display())]
    Resolve {
        /// The path as given.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// For a file that was not written because of permissions: whether the file or its folder
    /// refused. Callers show this rather than treating the write as done.
    pub fn refused(&self) -> Option<Refused> {
        match self {
            Error::Refused { reason, .. } => Some(*reason),
            Error::Write { .. } | Error::Resolve { .. } => None,
        }
    }
}

/// Why permissions kept a file from being written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    /// The file is read-only (locked by version control until it is checked out, say).
    ReadOnlyFile,
    /// The file's folder cannot be written to, so no file can be made there, not even the
    /// temporary file an atomic write puts in place of the target.
    ReadOnlyFolder,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::ReadOnlyFile => "the file is read-only",
            Refused::ReadOnlyFolder => "its folder can't be written to",
        })
    }
}
