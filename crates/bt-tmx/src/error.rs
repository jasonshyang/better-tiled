//! What stops a file from being read or written at all.

use std::path::{Path, PathBuf};

use bt_utils::Refused;

/// A failure that prevents a file from being read or written. Problems inside a readable file
/// (a missing tileset, an unknown gid, an invalid value) are not errors: they are [`crate::Warning`]s
/// next to the value that was read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The file could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: bt_utils::Error,
    },
    /// The file is not well-formed XML.
    #[error("invalid XML in {}: {source}", path.display())]
    Xml {
        /// The file.
        path: PathBuf,
        /// Where and why.
        #[source]
        source: roxmltree::Error,
    },
    /// The file is not well-formed JSON.
    #[error("invalid JSON in {}: {source}", path.display())]
    Json {
        /// The file.
        path: PathBuf,
        /// Where and why.
        #[source]
        source: serde_json::Error,
    },
    /// The file is not the kind asked for, its text cannot be decoded, or a value cannot be
    /// written in the format asked for.
    #[error("{}: {message}", path.display())]
    Format {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
}

impl Error {
    /// The file the error is about.
    pub fn path(&self) -> &Path {
        match self {
            Error::Io { path, .. }
            | Error::Write { path, .. }
            | Error::Xml { path, .. }
            | Error::Json { path, .. }
            | Error::Format { path, .. } => path,
        }
    }

    /// Why a write was refused before anything was written (a read-only file or folder).
    pub fn refused(&self) -> Option<Refused> {
        match self {
            Error::Write { source, .. } => source.refused(),
            _ => None,
        }
    }

    /// A [`Error::Format`] about `path`.
    pub(crate) fn format(path: impl Into<PathBuf>, message: impl Into<String>) -> Error {
        Error::Format {
            path: path.into(),
            message: message.into(),
        }
    }

    /// An [`Error::Io`] about `path`.
    pub(crate) fn from_io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
