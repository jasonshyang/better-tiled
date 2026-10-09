//! Decoding image files and embedded image bytes into straight-alpha RGBA pixels.

use std::io::Cursor;
use std::path::Path;

use crate::error::Error;
use crate::key::ColorKey;

/// An image as straight-alpha RGBA, eight bits a channel, row by row from the top-left.
/// Premultiplying is the renderer's business.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// Four bytes a pixel.
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    /// The memory the pixels take.
    pub fn byte_size(&self) -> usize {
        self.rgba.len()
    }

    /// The pixel at `x`, `y`, if inside.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = (usize::try_from(y).ok()? * usize::try_from(self.width).ok()?
            + usize::try_from(x).ok()?)
            * 4;
        let bytes = self.rgba.get(index..index + 4)?;
        <[u8; 4]>::try_from(bytes).ok()
    }

    /// Makes every pixel of the key's colour transparent, as Tiled's `trans` does; other pixels
    /// keep their alpha.
    pub(crate) fn apply_color_key(&mut self, key: ColorKey) {
        for pixel in self.rgba.chunks_exact_mut(4) {
            if let [r, g, b, a] = pixel
                && (*r, *g, *b) == (key.r, key.g, key.b)
            {
                *a = 0;
            }
        }
    }
}

/// Whether a file used as an image is a map, which is drawn rather than decoded.
pub fn is_map_file(path: &Path) -> bool {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    ["tmx", "tmj"]
        .iter()
        .any(|map| extension.eq_ignore_ascii_case(map))
}

/// Decodes an image file. A map file is not an image: the store draws it through its
/// [`crate::MapImageSource`].
pub fn decode_file(path: &Path, transparent: Option<ColorKey>) -> Result<DecodedImage, Error> {
    if is_map_file(path) {
        return Err(Error::NoMapSource {
            path: path.to_path_buf(),
        });
    }
    let reader = image::ImageReader::open(path).map_err(|e| Error::from_io(path, &e))?;
    let reader = reader
        .with_guessed_format()
        .map_err(|e| Error::from_io(path, &e))?;
    let decoded = reader
        .decode()
        .map_err(|e| decode_error(&path.display().to_string(), e))?;
    Ok(finished(decoded, transparent))
}

/// Decodes the bytes of an image embedded in a Tiled file; `format` is the format the file names
/// (see [`crate::ImageInput::Bytes`]).
pub fn decode_bytes(
    bytes: &[u8],
    format: &str,
    transparent: Option<ColorKey>,
) -> Result<DecodedImage, Error> {
    let decoded = reader_of_bytes(bytes, format)?
        .decode()
        .map_err(|e| decode_error(EMBEDDED, e))?;
    Ok(finished(decoded, transparent))
}

/// How errors name an embedded image.
pub(crate) const EMBEDDED: &str = "an embedded image";

/// A reader for embedded bytes, with the format the bytes show or else the one the file names.
pub(crate) fn reader_of_bytes<'b>(
    bytes: &'b [u8],
    format: &str,
) -> Result<image::ImageReader<Cursor<&'b [u8]>>, Error> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::Decode {
            source_name: EMBEDDED.to_owned(),
            message: e.to_string(),
        })?;
    if reader.format().is_none()
        && let Some(named) = image::ImageFormat::from_extension(format)
    {
        reader.set_format(named);
    }
    Ok(reader)
}

fn finished(decoded: image::DynamicImage, transparent: Option<ColorKey>) -> DecodedImage {
    let rgba = decoded.into_rgba8();
    let mut image = DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    };
    if let Some(key) = transparent {
        image.apply_color_key(key);
    }
    image
}

pub(crate) fn decode_error(source_name: &str, error: image::ImageError) -> Error {
    match error {
        image::ImageError::Unsupported(_) => Error::Unsupported {
            source_name: source_name.to_owned(),
        },
        other => Error::Decode {
            source_name: source_name.to_owned(),
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::Scratch;
    use image::{ImageFormat, Rgba, RgbaImage};

    /// A 3×2 picture: magenta, red, green / blue, half-transparent white, black.
    pub(crate) fn sample() -> RgbaImage {
        let mut image = RgbaImage::new(3, 2);
        let pixels = [
            [255, 0, 255, 255],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 255, 128],
            [0, 0, 0, 255],
        ];
        for (i, p) in pixels.into_iter().enumerate() {
            image.put_pixel(i as u32 % 3, i as u32 / 3, Rgba(p));
        }
        image
    }

    fn encoded(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image.clone())
            .write_to(&mut bytes, format)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn lossless_formats_decode_to_the_same_pixels() {
        for format in [
            ImageFormat::Png,
            ImageFormat::Tiff,
            ImageFormat::Tga,
            ImageFormat::Bmp,
            ImageFormat::Ico,
            ImageFormat::WebP,
        ] {
            let name = format.extensions_str().first().copied().unwrap();
            let decoded = decode_bytes(&encoded(&sample(), format), name, None).unwrap();
            assert_eq!((decoded.width, decoded.height), (3, 2), "{format:?}");
            assert_eq!(decoded.rgba, sample().into_raw(), "{format:?}");
        }
        let opaque = image::DynamicImage::ImageRgba8(sample()).to_rgb8();
        for format in [ImageFormat::Pnm, ImageFormat::Gif, ImageFormat::Jpeg] {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(opaque.clone())
                .write_to(&mut bytes, format)
                .unwrap();
            let decoded = decode_bytes(bytes.get_ref(), "", None).unwrap();
            assert_eq!((decoded.width, decoded.height), (3, 2), "{format:?}");
        }
        // TGA has no signature: without the format the file names, nothing says what it is.
        let tga = encoded(&sample(), ImageFormat::Tga);
        assert_eq!(
            decode_bytes(&tga, "", None),
            Err(Error::Unsupported {
                source_name: EMBEDDED.to_owned()
            })
        );
        // The bytes win over a wrong name.
        let png = encoded(&sample(), ImageFormat::Png);
        assert_eq!(
            decode_bytes(&png, "jpg", None).unwrap().rgba,
            sample().into_raw()
        );
    }

    #[test]
    fn a_colour_key_clears_alpha_where_the_colour_is() {
        let decoded = decode_bytes(
            &encoded(&sample(), ImageFormat::Png),
            "png",
            Some(ColorKey {
                r: 255,
                g: 0,
                b: 255,
            }),
        )
        .unwrap();
        assert_eq!(decoded.pixel(0, 0), Some([255, 0, 255, 0]));
        assert_eq!(decoded.pixel(1, 0), Some([255, 0, 0, 255]));
        assert_eq!(
            decoded.pixel(1, 1),
            Some([255, 255, 255, 128]),
            "others keep their alpha"
        );
        assert_eq!(decoded.pixel(3, 0), None);
    }

    #[test]
    fn files_decode_and_failures_say_why() {
        let scratch = Scratch::new("bt-image-decode").unwrap();
        let path = scratch.join("tiles.png");
        std::fs::write(&path, encoded(&sample(), ImageFormat::Png)).unwrap();
        assert_eq!(decode_file(&path, None).unwrap().byte_size(), 24);
        std::fs::write(scratch.join("notes.png"), b"not an image").unwrap();
        assert!(matches!(
            decode_file(&scratch.join("notes.png"), None),
            Err(Error::Unsupported { .. } | Error::Decode { .. })
        ));
        assert!(matches!(
            decode_file(&scratch.join("missing.png"), None),
            Err(Error::Io { .. })
        ));
        assert!(matches!(
            decode_file(&scratch.join("level.tmx"), None),
            Err(Error::NoMapSource { .. })
        ));
        assert!(is_map_file(Path::new("a/B.TMX")) && !is_map_file(Path::new("a/b.png")));
    }
}
