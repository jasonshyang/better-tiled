//! Image sizes from file headers, without decoding the pixels.

use std::path::Path;

use glam::UVec2;

use crate::decode::{EMBEDDED, decode_error, reader_of_bytes};
use crate::error::Error;

/// The size of an image file, read from its header.
pub fn probe_size(path: &Path) -> Result<UVec2, Error> {
    let reader = image::ImageReader::open(path).map_err(|e| Error::from_io(path, &e))?;
    let reader = reader
        .with_guessed_format()
        .map_err(|e| Error::from_io(path, &e))?;
    let source_name = path.display().to_string();
    sized(
        reader
            .into_dimensions()
            .map_err(|e| decode_error(&source_name, e))?,
        &source_name,
    )
}

/// The size of an embedded image, read from its header; `format` is the format the file names
/// (see [`crate::ImageInput::Bytes`]).
pub fn probe_size_of_bytes(bytes: &[u8], format: &str) -> Result<UVec2, Error> {
    sized(
        reader_of_bytes(bytes, format)?
            .into_dimensions()
            .map_err(|e| decode_error(EMBEDDED, e))?,
        EMBEDDED,
    )
}

/// A size, refusing an image with no pixels.
fn sized((width, height): (u32, u32), source_name: &str) -> Result<UVec2, Error> {
    if width == 0 || height == 0 {
        return Err(Error::Unsupported {
            source_name: source_name.to_owned(),
        });
    }
    Ok(UVec2::new(width, height))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use bt_test_data::Scratch;

    #[test]
    fn sizes_come_from_headers() {
        let scratch = Scratch::new("bt-image-probe").unwrap();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(265, 199)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let path = scratch.join("a.png");
        std::fs::write(&path, png.get_ref()).unwrap();
        assert_eq!(probe_size(&path).unwrap(), UVec2::new(265, 199));
        assert_eq!(
            probe_size_of_bytes(png.get_ref(), "png").unwrap(),
            UVec2::new(265, 199)
        );
        // The pixels are not read: a file cut short in its pixel data still has a size.
        let cut = png.get_ref().len() - 30;
        assert_eq!(
            probe_size_of_bytes(png.get_ref().get(..cut).unwrap(), "png").unwrap(),
            UVec2::new(265, 199)
        );
        assert!(image::load_from_memory(png.get_ref().get(..cut).unwrap()).is_err());
        let mut gif = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(106, 72)
            .write_to(&mut gif, image::ImageFormat::Gif)
            .unwrap();
        assert_eq!(
            probe_size_of_bytes(gif.get_ref(), "").unwrap(),
            UVec2::new(106, 72)
        );
        assert!(probe_size_of_bytes(b"hello", "").is_err());
        assert!(matches!(
            probe_size(&scratch.join("missing.png")),
            Err(Error::Io { .. })
        ));
    }

    /// Every image of Tiled's examples: the header gives the decoded size.
    #[test]
    fn tileds_example_images_probe_to_their_decoded_size() {
        let Some(examples) = bt_test_data::test_data("reference/tiled/examples") else {
            return;
        };
        let mut images = Vec::new();
        let mut folders = vec![examples];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                let image = path.extension().is_some_and(|e| {
                    ["png", "jpg", "gif", "bmp"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                });
                if path.is_dir() {
                    folders.push(path);
                } else if image {
                    images.push(path);
                }
            }
        }
        assert!(images.len() > 20, "found {} images", images.len());
        for path in images {
            let decoded = crate::decode_file(&path, None).unwrap();
            assert_eq!(
                probe_size(&path).unwrap(),
                UVec2::new(decoded.width, decoded.height),
                "{}",
                path.display()
            );
        }
    }
}
