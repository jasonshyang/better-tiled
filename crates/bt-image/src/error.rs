//! The crate's error type.

use std::path::PathBuf;

/// Why an image could not be read. Cloneable, so every caller waiting on an image gets the same
/// answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The file could not be read.
    #[error("couldn't read {}: {message}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// What the system said.
        message: String,
    },
    /// The bytes are not an image of a format it reads, or a broken one.
    #[error("{source_name} can't be decoded: {message}")]
    Decode {
        /// The file, or "an embedded image".
        source_name: String,
        /// What the decoder said.
        message: String,
    },
    /// The header names no format it reads, or gives no size.
    #[error(
        "{source_name} is not an image of a format better-tiled reads (PNG, JPEG, GIF, BMP, WebP, TGA, TIFF, PNM, ICO)"
    )]
    Unsupported {
        /// The file, or "an embedded image".
        source_name: String,
    },
    /// A map used as an image shows, through its own tilesets, itself.
    #[error("{} is drawn with itself as one of its images", path.display())]
    Recursive {
        /// The map.
        path: PathBuf,
    },
    /// A map is used as an image, and nothing that draws maps was given.
    #[error("{} is a map used as an image, and maps can't be drawn here", path.display())]
    NoMapSource {
        /// The map.
        path: PathBuf,
    },
    /// The worker threads are gone (the store is shutting down).
    #[error("the image workers stopped")]
    Stopped,
    /// A file is already where a new image was to be written, and is never replaced.
    #[error("{} already exists and is not replaced", path.display())]
    Exists {
        /// The file.
        path: PathBuf,
    },
    /// An image could not be encoded or written.
    #[error("couldn't write {}: {message}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
}

impl Error {
    /// The error for a file the system could not read.
    pub(crate) fn from_io(path: impl Into<PathBuf>, error: &std::io::Error) -> Error {
        Error::Io {
            path: path.into(),
            message: error.to_string(),
        }
    }

    /// The error for a file the system could not write.
    pub(crate) fn from_write(path: impl Into<PathBuf>, error: &std::io::Error) -> Error {
        Error::Write {
            path: path.into(),
            message: error.to_string(),
        }
    }
}
