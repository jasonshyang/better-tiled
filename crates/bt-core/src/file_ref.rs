//! References from one file to another.

use std::path::PathBuf;

/// A file or folder named in another file: as written there, and resolved. Writers put the
/// written form back while it still names the same file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileRef {
    /// As written: relative to the containing file's folder, with `/` separators, or verbatim
    /// (`:/automap-tiles.tsx`).
    pub source: String,
    /// Resolved: absolute and lexically cleaned when the containing file's place is known.
    pub path: PathBuf,
}

impl FileRef {
    /// A reference written as `source`, resolved to `path`.
    pub fn new(source: impl Into<String>, path: impl Into<PathBuf>) -> FileRef {
        FileRef {
            source: source.into(),
            path: path.into(),
        }
    }
}
