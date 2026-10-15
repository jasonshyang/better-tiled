//! The one place image files are decoded: PNG, JPEG, GIF, BMP, WebP, TGA, TIFF, PNM and ICO, the
//! raster formats Tiled's images use (SVG and Qt's rarer formats are not read). It reads an image's
//! size from its header without decoding it, applies a colour key, measures alpha bounds (where a
//! sprite's feet are), and keeps decoded images in a store that decodes on worker threads, so a
//! tileset's images never stall a frame. A map file can be the image of a tileset, a tile or an
//! image layer; the store draws it through a [`MapImageSource`] it is given. Images made here
//! (RPG Maker autotiles composed into a tileset) are written as new PNG files ([`write_png`]).

mod decode;
mod encode;
mod error;
mod key;
mod measure;
mod probe;
mod request;
mod store;

pub use decode::{DecodedImage, decode_bytes, decode_file, is_map_file};
pub use encode::{encode_png, replace_png, write_png};
pub use error::Error;
pub use key::{ColorKey, ImageInput, ImageKey, ImageRequest};
pub use measure::{alpha_bounds, bottom_center};
pub use probe::{probe_size, probe_size_of_bytes};
pub use request::image_request;
pub use store::{ImagePoll, ImageStore, ImageStoreBuilder, MapImageSource, NestedImages};
