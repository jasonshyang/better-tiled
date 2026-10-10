//! The tileset images the Tiles panel shows, as egui textures: decoded by the image store the
//! renderer shares, uploaded once, shrunk when larger than the GPU allows.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use bt_core::ImageRef;
use bt_image::{DecodedImage, ImageKey, ImagePoll, ImageStore};
use bt_render::image_request;
use egui::{TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};
use glam::UVec2;

/// How tile images are sampled: crisp when enlarged, smooth when shrunk.
const SAMPLING: TextureOptions = TextureOptions {
    magnification: TextureFilter::Nearest,
    minification: TextureFilter::Linear,
    wrap_mode: TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

/// The textures uploaded so far.
#[derive(Default)]
pub(crate) struct TileImages {
    textures: HashMap<ImageKey, Upload>,
    pending: bool,
}

/// What became of an image.
enum Upload {
    Ready(Uploaded),
    Failed(String),
}

/// An image on the GPU.
pub(crate) struct Uploaded {
    /// The texture (smaller than the image when the image is larger than the GPU allows).
    pub(crate) texture: TextureHandle,
    /// The image's own size in pixels, which tile rectangles are measured in.
    pub(crate) image_size: UVec2,
}

/// An image as the panel can show it.
pub(crate) enum TileTexture<'a> {
    /// Ready to draw.
    Ready(&'a Uploaded),
    /// Still being decoded.
    Pending,
    /// It could not be decoded, and why.
    Failed(&'a str),
}

impl TileImages {
    /// The texture of an image of the model; asks the store to decode it the first time.
    pub(crate) fn texture(
        &mut self,
        ctx: &egui::Context,
        store: &Mutex<ImageStore>,
        image: &ImageRef,
    ) -> TileTexture<'_> {
        let request = image_request(image);
        let key = ImageKey::of(&request);
        if !self.textures.contains_key(&key) {
            let poll = {
                let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
                let asked = store.request(request);
                store.poll(&asked)
            };
            let upload = match poll {
                ImagePoll::Ready(decoded) => Upload::Ready(Uploaded {
                    texture: upload(ctx, &key, &decoded),
                    image_size: UVec2::new(decoded.width, decoded.height),
                }),
                ImagePoll::Failed(error) => Upload::Failed(error.to_string()),
                ImagePoll::Pending | ImagePoll::NotRequested => {
                    self.pending = true;
                    return TileTexture::Pending;
                }
            };
            self.textures.insert(key.clone(), upload);
        }
        match self.textures.get(&key) {
            Some(Upload::Ready(uploaded)) => TileTexture::Ready(uploaded),
            Some(Upload::Failed(error)) => TileTexture::Failed(error),
            None => TileTexture::Pending,
        }
    }

    /// Whether an image was still being decoded when asked for since the last call.
    pub(crate) fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

fn upload(ctx: &egui::Context, key: &ImageKey, image: &DecodedImage) -> TextureHandle {
    let max_side = ctx.input(|input| input.max_texture_side).max(1);
    let shrunk;
    let image = match shrink_to_fit(image, max_side) {
        Some(smaller) => {
            shrunk = smaller;
            &shrunk
        }
        None => image,
    };
    let size = [image.width as usize, image.height as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
    let name = key.path().map_or_else(
        || "embedded image".to_owned(),
        |path| path.display().to_string(),
    );
    ctx.load_texture(name, pixels, SAMPLING)
}

/// The image shrunk by a whole factor so no side exceeds `max_side`; `None` when it fits.
fn shrink_to_fit(image: &DecodedImage, max_side: usize) -> Option<DecodedImage> {
    let longest = image.width.max(image.height) as usize;
    if longest <= max_side {
        return None;
    }
    let factor = longest.div_ceil(max_side) as u32;
    let width = (image.width / factor).max(1);
    let height = (image.height / factor).max(1);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let pixel = image.pixel(x * factor, y * factor).unwrap_or([0; 4]);
            rgba.extend_from_slice(&pixel);
        }
    }
    Some(DecodedImage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_larger_than_the_gpu_allows_are_shrunk() {
        let image = DecodedImage {
            width: 10,
            height: 4,
            rgba: (0..40u8).flat_map(|i| [i, 0, 0, 255]).collect(),
        };
        assert!(shrink_to_fit(&image, 10).is_none());
        let shrunk = shrink_to_fit(&image, 4).unwrap();
        assert_eq!((shrunk.width, shrunk.height), (3, 1));
        assert_eq!(shrunk.pixel(1, 0), Some([3, 0, 0, 255]));
    }
}
