//! The pictures `bt` writes (`bt render`, `bt catalog --sheet`; `10-agents.md`, §10.3, §10.4):
//! PNG files that replace the picture there, since agents draw to one file again and again;
//! written atomically, so a picture is never half written; and never over a read-only file or
//! into a folder that can't be written to (§4.3).

use std::path::Path;

use bt_image::{DecodedImage, encode_png};
use bt_utils::atomic_write::{WriteMode, write_file};

/// Writes `picture` to `path` as a PNG, replacing the file there.
pub(crate) fn write_picture(path: &Path, picture: &DecodedImage) -> anyhow::Result<()> {
    let bytes = encode_png(path, picture)?;
    write_file(path, &bytes, WriteMode::Atomic)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use bt_image::decode_file;
    use bt_test_data::Scratch;

    use super::*;
    use crate::commands::testing::lock;

    #[test]
    fn a_picture_replaces_the_one_there_and_never_a_read_only_file() {
        let scratch = Scratch::new("bt-picture-file").unwrap();
        let path = scratch.join("out.png");
        let red = DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
        };
        let blue = DecodedImage {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 255, 255, 0, 0, 255, 255],
        };
        write_picture(&path, &red).unwrap();
        write_picture(&path, &blue).unwrap();
        assert_eq!(decode_file(&path, None).unwrap(), blue);
        lock(&path);
        let error = write_picture(&path, &red).unwrap_err();
        assert!(
            format!("{error:#}").ends_with("out.png: the file is read-only"),
            "{error:#}"
        );
        assert_eq!(decode_file(&path, None).unwrap(), blue, "left as it was");
    }
}
