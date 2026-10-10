//! Where the renderer gets decoded images: the image store's workers, or (for a map drawn as an
//! image) the images decoded for it on the spot by the store's nested decoder.

use std::collections::HashMap;
use std::sync::Arc;

use bt_core::{ImageRef, ImageSource};
use bt_image::{ColorKey, ImageInput, ImageKey, ImagePoll, ImageRequest, ImageStore, NestedImages};

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

/// A source of decoded images.
pub trait ImageSupply {
    /// Asks for an image; its key.
    fn request(&mut self, request: ImageRequest) -> ImageKey;
    /// What there is for a key now.
    fn poll(&mut self, key: &ImageKey) -> ImagePoll;
    /// Waits until an image asked for is decoded or has failed.
    fn wait(&mut self, request: ImageRequest);
}

impl ImageSupply for ImageStore {
    fn request(&mut self, request: ImageRequest) -> ImageKey {
        ImageStore::request(self, request)
    }

    fn poll(&mut self, key: &ImageKey) -> ImagePoll {
        ImageStore::poll(self, key)
    }

    fn wait(&mut self, request: ImageRequest) {
        // A failure is kept by the store and seen at the next poll.
        let _ = self.decode_now(request);
    }
}

/// The images of a map being drawn as an image, decoded when asked for.
pub struct NestedSupply<'n> {
    nested: &'n NestedImages<'n>,
    decoded: HashMap<ImageKey, ImagePoll>,
}

impl<'n> NestedSupply<'n> {
    /// Decodes through `nested`.
    pub fn new(nested: &'n NestedImages<'n>) -> NestedSupply<'n> {
        NestedSupply {
            nested,
            decoded: HashMap::new(),
        }
    }
}

impl ImageSupply for NestedSupply<'_> {
    fn request(&mut self, request: ImageRequest) -> ImageKey {
        let key = ImageKey::of(&request);
        if !self.decoded.contains_key(&key) {
            let result = match self.nested.decode(&request) {
                Ok(image) => ImagePoll::Ready(Arc::new(image)),
                Err(error) => ImagePoll::Failed(error),
            };
            self.decoded.insert(key.clone(), result);
        }
        key
    }

    fn poll(&mut self, key: &ImageKey) -> ImagePoll {
        self.decoded
            .get(key)
            .cloned()
            .unwrap_or(ImagePoll::NotRequested)
    }

    fn wait(&mut self, _request: ImageRequest) {}
}
