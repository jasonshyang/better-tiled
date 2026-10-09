//! References to images: a file, or bytes embedded in the containing file.

use std::path::Path;
use std::sync::Arc;

use glam::UVec2;

use crate::color::Color;
use crate::file_ref::FileRef;

/// An image a tileset, a tile or an image layer shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageRef {
    /// Where the image is.
    pub source: ImageSource,
    /// The size the file declares: what "the image changed size since the file was written" is
    /// measured against. Zero when it declares none.
    pub declared_size: UVec2,
    /// The colour drawn as transparent (Tiled's `trans`), if any.
    pub transparent: Option<Color>,
}

/// Where an image's pixels come from.
#[derive(Clone, Debug, PartialEq)]
pub enum ImageSource {
    /// A file.
    File(FileRef),
    /// Bytes embedded in the containing file, written back as they were read.
    Embedded(EmbeddedImage),
}

/// An image embedded in a Tiled file (`<image format="png"><data encoding="base64">`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedImage {
    /// The format the file names (`png`).
    pub format: String,
    /// The encoded image, as stored.
    pub bytes: Arc<[u8]>,
}

impl ImageRef {
    /// A file image of the declared size, without a transparent colour.
    pub fn file(file: FileRef, declared_size: UVec2) -> ImageRef {
        ImageRef {
            source: ImageSource::File(file),
            declared_size,
            transparent: None,
        }
    }

    /// The resolved path of a file image; `None` for an embedded one.
    pub fn path(&self) -> Option<&Path> {
        match &self.source {
            ImageSource::File(file) => Some(&file.path),
            ImageSource::Embedded(_) => None,
        }
    }
}
