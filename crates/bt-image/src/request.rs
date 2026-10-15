//! Model image references translated once for the renderer and diagnostic workers.
use crate::{ColorKey, ImageInput, ImageRequest};
use bt_core::{ImageRef, ImageSource};
use std::sync::Arc;

/// The request that decodes an image of the model (a file, or bytes embedded in a Tiled file),
/// with its colour key.
pub fn image_request(image: &ImageRef) -> ImageRequest {
    let input = match &image.source {
        ImageSource::File(file) => ImageInput::File(file.path.clone()),
        ImageSource::Embedded(embedded) => ImageInput::Bytes {
            bytes: Arc::clone(&embedded.bytes),
            format: embedded.format.clone(),
        },
    };
    ImageRequest {
        input,
        transparent: image.transparent.map(|c| ColorKey {
            r: c.r,
            g: c.g,
            b: c.b,
        }),
    }
}
