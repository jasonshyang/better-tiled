//! Image layers.

use crate::image_ref::ImageRef;

/// The payload of an image layer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageLayer {
    /// The image; `None` for a layer without one.
    pub image: Option<ImageRef>,
    /// Whether the image repeats horizontally.
    pub repeat_x: bool,
    /// Whether the image repeats vertically.
    pub repeat_y: bool,
}
