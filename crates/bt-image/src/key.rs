//! What identifies a decoded image.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// A colour drawn as transparent (Tiled's `trans`); alpha does not take part.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ColorKey {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

/// Where an image's bytes are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageInput {
    /// A file (an image, or a map drawn as one).
    File(PathBuf),
    /// An image embedded in a Tiled file.
    Bytes {
        /// The encoded image.
        bytes: Arc<[u8]>,
        /// The format the file names (`png`), or empty. The bytes say which format they are;
        /// this is used only when they do not, as TGA's do not.
        format: String,
    },
}

/// An image to decode: its bytes and its colour key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRequest {
    /// Where the bytes are.
    pub input: ImageInput,
    /// The colour drawn as transparent, if any.
    pub transparent: Option<ColorKey>,
}

/// Identifies a decoded image: a file by its path and modification time, embedded bytes by
/// their content hash, and the colour key, which changes the pixels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey {
    identity: Identity,
    transparent: Option<ColorKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Identity {
    File {
        path: PathBuf,
        modified: Option<SystemTime>,
    },
    Bytes {
        hash: u64,
        len: usize,
        format: String,
    },
}

impl ImageKey {
    /// The key of a request as things are now: a file's key changes when the file does.
    pub fn of(request: &ImageRequest) -> ImageKey {
        let identity = match &request.input {
            ImageInput::File(path) => Identity::File {
                path: path.clone(),
                modified: modified(path),
            },
            ImageInput::Bytes { bytes, format } => Identity::Bytes {
                hash: bt_utils::hash::fnv1a_64(bytes),
                len: bytes.len(),
                format: format.clone(),
            },
        };
        ImageKey {
            identity,
            transparent: request.transparent,
        }
    }

    /// The file the image comes from, for a file image.
    pub fn path(&self) -> Option<&Path> {
        match &self.identity {
            Identity::File { path, .. } => Some(path),
            Identity::Bytes { .. } => None,
        }
    }
}

/// A file's modification time; `None` when the system does not say (the file is then keyed by
/// its path alone).
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_keyed_by_content_and_colour_key() {
        let request = |bytes: &[u8], transparent| ImageRequest {
            input: ImageInput::Bytes {
                bytes: Arc::from(bytes),
                format: "png".to_owned(),
            },
            transparent,
        };
        let key = ImageKey::of(&request(b"abc", None));
        assert_eq!(key, ImageKey::of(&request(b"abc", None)));
        assert_ne!(key, ImageKey::of(&request(b"abd", None)));
        assert_ne!(
            key,
            ImageKey::of(&request(
                b"abc",
                Some(ColorKey {
                    r: 255,
                    g: 0,
                    b: 255
                })
            ))
        );
        assert_eq!(key.path(), None);
    }
}
