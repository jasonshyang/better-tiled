//! References from one file to another: how a written reference resolves, and how a reference
//! is written back.

use std::path::{Path, PathBuf};

use bt_core::FileRef;
use bt_utils::paths::{clean_path, relative_path};

/// Whether a reference is kept as written instead of resolved against a folder: a Qt resource
/// (`:/automap-tiles.tsx`) or a URL with a scheme of two or more letters (`ext:`, `qrc:`,
/// `http:`). One letter before the colon is a Windows drive.
pub(crate) fn is_verbatim_reference(reference: &str) -> bool {
    if reference.starts_with(":/") {
        return true;
    }
    match reference.split_once(':') {
        Some((scheme, _)) if scheme.len() >= 2 => {
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        }
        _ => false,
    }
}

/// Whether a reference is an absolute Windows path (`C:/art/a.png`).
fn is_windows_absolute(reference: &str) -> bool {
    let mut chars = reference.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(drive), Some(':'), Some('/')) if drive.is_ascii_alphabetic()
    )
}

/// Resolves a reference written in a file in the folder `dir`: backslashes become `/`,
/// `file://` is dropped, an absolute path is cleaned, a relative one is joined to `dir` and
/// cleaned; Windows drive paths and verbatim references are kept as they are. Links are not
/// followed.
pub(crate) fn resolve_reference(dir: &Path, reference: &str) -> PathBuf {
    let normalized = reference.trim().replace('\\', "/");
    if let Some(local) = normalized.strip_prefix("file://") {
        return clean_path(Path::new(local));
    }
    if is_windows_absolute(&normalized) || is_verbatim_reference(&normalized) {
        return PathBuf::from(normalized);
    }
    let path = Path::new(&normalized);
    if path.is_absolute() {
        clean_path(path)
    } else {
        clean_path(&dir.join(path))
    }
}

/// A reference written in a file in `dir`, with its resolved path.
pub(crate) fn file_ref(dir: &Path, reference: &str) -> FileRef {
    FileRef::new(reference, resolve_reference(dir, reference))
}

/// How a reference is written in a file in `dir`: as it was written when that still names the
/// same file, else relative to `dir`, else absolute. Verbatim references are written as they
/// are.
pub(crate) fn written_reference(dir: &Path, file: &FileRef) -> String {
    let source = file.source.as_str();
    if is_verbatim_reference(source.trim()) || file.path.as_os_str().is_empty() {
        return source.to_owned();
    }
    if !source.is_empty() && resolve_reference(dir, source) == file.path {
        return source.to_owned();
    }
    if file.path.is_absolute()
        && let Some(relative) = relative_path(dir, &file.path)
    {
        return relative;
    }
    file.path.to_string_lossy().replace('\\', "/")
}

/// A `file` property value written relative to `from_dir`, rewritten relative to `to_dir`, for
/// a value moving to another folder (Save As). Empty, absolute and verbatim values stay.
pub(crate) fn rebased_file_value(value: &str, from_dir: &Path, to_dir: &Path) -> Option<String> {
    if value.is_empty() || is_verbatim_reference(value) {
        return None;
    }
    let resolved = resolve_reference(from_dir, value);
    if !resolved.is_absolute() || Path::new(value).is_absolute() {
        return None;
    }
    relative_path(to_dir, &resolved).filter(|relative| relative != value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_resolve_against_their_folder() {
        let dir = Path::new("/maps/world");
        let resolve = |r: &str| resolve_reference(dir, r);
        assert_eq!(
            resolve("../tiles/a.tsx"),
            PathBuf::from("/maps/tiles/a.tsx")
        );
        assert_eq!(
            resolve("Rules\\r1.tmx"),
            PathBuf::from("/maps/world/Rules/r1.tmx")
        );
        assert_eq!(resolve("./x/./y.png"), PathBuf::from("/maps/world/x/y.png"));
        assert_eq!(resolve("/abs/z.png"), PathBuf::from("/abs/z.png"));
        assert_eq!(resolve("file:///abs/../z.png"), PathBuf::from("/z.png"));
        assert_eq!(
            resolve(":/automap-tiles.tsx"),
            PathBuf::from(":/automap-tiles.tsx")
        );
        assert_eq!(resolve("C:\\art\\a.png"), PathBuf::from("C:/art/a.png"));
        assert_eq!(resolve("ext:objects.tx"), PathBuf::from("ext:objects.tx"));
    }

    #[test]
    fn written_references_keep_their_text_while_it_still_fits() {
        let dir = Path::new("/maps/world");
        let kept = FileRef::new("./../tiles/a.tsx", "/maps/tiles/a.tsx");
        assert_eq!(written_reference(dir, &kept), "./../tiles/a.tsx");
        assert_eq!(written_reference(Path::new("/maps"), &kept), "tiles/a.tsx");
        let new = FileRef::new("", "/maps/world/b.png");
        assert_eq!(written_reference(dir, &new), "b.png");
        let builtin = FileRef::new(":/automap-tiles.tsx", ":/automap-tiles.tsx");
        assert_eq!(written_reference(dir, &builtin), ":/automap-tiles.tsx");
        let unresolved = FileRef::new("x.png", "");
        assert_eq!(written_reference(dir, &unresolved), "x.png");
    }

    #[test]
    fn file_values_move_with_their_file() {
        let (from, to) = (Path::new("/a/maps"), Path::new("/a/maps/sub"));
        assert_eq!(
            rebased_file_value("s.lua", from, to).as_deref(),
            Some("../s.lua")
        );
        assert_eq!(rebased_file_value("", from, to), None);
        assert_eq!(rebased_file_value("/abs.lua", from, to), None);
        assert_eq!(rebased_file_value("s.lua", from, from), None);
    }
}
