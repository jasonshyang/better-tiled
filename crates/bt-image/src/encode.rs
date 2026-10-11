//! Writing images: PNG files, made new and never written over another file, and the PNG bytes of
//! an image for a caller that writes its files its own way.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write as _};
use std::path::Path;

use image::ImageEncoder as _;
use image::codecs::png::PngEncoder;

use crate::decode::DecodedImage;
use crate::error::Error;

/// Writes `image` to a new PNG file at `path`. A file already there is never replaced: that is
/// [`Error::Exists`], so a caller looking for a free name tries the next one. A write that fails
/// leaves no file behind.
pub fn write_png(path: &Path, image: &DecodedImage) -> Result<(), Error> {
    let bytes = encode_png(path, image)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            if error.kind() == ErrorKind::AlreadyExists {
                Error::Exists {
                    path: path.to_path_buf(),
                }
            } else {
                Error::from_write(path, &error)
            }
        })?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        // The file is this call's own, so a half-written one goes; one that cannot go is left
        // for the user to see, and the error says what happened.
        let _ = std::fs::remove_file(path);
        return Err(Error::from_write(path, &error));
    }
    Ok(())
}

/// The PNG bytes of `image`, for the file at `path`, which errors name.
pub fn encode_png(path: &Path, image: &DecodedImage) -> Result<Vec<u8>, Error> {
    let expected = u64::from(image.width) * u64::from(image.height) * 4;
    if u64::try_from(image.rgba.len()).ok() != Some(expected) {
        return Err(Error::Write {
            path: path.to_path_buf(),
            message: format!(
                "{} bytes of pixels do not make a {}×{} image",
                image.rgba.len(),
                image.width,
                image.height
            ),
        });
    }
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            &image.rgba,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| Error::Write {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_file;
    use bt_test_data::Scratch;

    fn picture() -> DecodedImage {
        DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 100],
        }
    }

    #[test]
    fn a_png_reads_back_as_written() {
        let scratch = Scratch::new("bt-image-write-png").unwrap();
        let path = scratch.join("out.png");
        write_png(&path, &picture()).unwrap();
        assert_eq!(decode_file(&path, None).unwrap(), picture());
    }

    #[test]
    fn a_file_already_there_is_never_replaced() {
        let scratch = Scratch::new("bt-image-write-png-exists").unwrap();
        let path = scratch.join("taken.png");
        std::fs::write(&path, b"mine").unwrap();
        assert_eq!(
            write_png(&path, &picture()),
            Err(Error::Exists { path: path.clone() })
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        let wrong = DecodedImage {
            width: 3,
            height: 3,
            rgba: vec![0; 8],
        };
        let other = scratch.join("wrong.png");
        assert!(matches!(
            write_png(&other, &wrong),
            Err(Error::Write { .. })
        ));
        assert!(!other.exists(), "nothing is written");
        assert!(matches!(
            write_png(&scratch.join("no folder").join("a.png"), &picture()),
            Err(Error::Write { .. })
        ));
    }
}
