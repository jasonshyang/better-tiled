//! Keys and paths in a project file: relative to the file's folder, written with `/`, and
//! resolved lexically, in the form the map's own path has.
//!
//! Nothing here follows a symbolic link or makes a path canonical. A project file is found in
//! the form its map's path has (through the same links), and the paths it gives back are in that
//! form too, so the rules of an AutoMap record and the map read their tilesets and images by the
//! same paths. A canonical path (through `/private/var` on macOS, say) would be another path to
//! the same file and would not match. A map's key comes from the same normalised form, so one
//! file always gets one key.

use std::path::{Path, PathBuf};

use bt_core::{FileRef, TilesetRef};
use bt_utils::paths::{absolute_path, clean_path, relative_path};

use crate::error::Error;

/// The key of `path` in the project file `file`: `path` relative to the file's folder, written
/// with `/`, with `.` removed and `..` folded lexically (`caves/deep.tmx`, `../rules.txt`). This
/// is a map's key, and how the file writes every other path it holds (the rules of an AutoMap
/// record, the Godot project). Relative paths are taken from the working folder; both paths
/// should be in the same form, as a file found from the map is.
pub fn key(file: &Path, path: &Path) -> Result<String, Error> {
    let folder = folder_of(&absolute(file)?).to_path_buf();
    let path = absolute(path)?;
    relative_path(&folder, &path).ok_or_else(|| Error::Unrelated {
        file: file.to_path_buf(),
        path,
    })
}

/// A key or path the project file `file` holds, as a full path: joined to the file's folder and
/// cleaned lexically, in the form the file's own path has. An absolute path stays as it is.
pub fn resolve(file: &Path, key: &str) -> PathBuf {
    clean_path(&folder_of(file).join(key))
}

/// How painted rows and checksums name one of a map's tilesets, so that neither depends on the
/// order of the map's tilesets: the tileset's path as the map writes it (relative to the map),
/// with `/` and `.` and `..` folded, or `#` and its name for a tileset embedded in the map. A
/// reference the map has not written yet is named by its resolved path.
pub fn tileset_key(tileset: &TilesetRef) -> String {
    match &tileset.file {
        None => format!("#{}", tileset.tileset.name),
        Some(file) => reference_key(file),
    }
}

/// How checksums name a file a map refers to (a tileset, a template): its path as the map writes
/// it, with `/` and `.` and `..` folded, or its resolved path when the map has not written it yet.
pub(crate) fn reference_key(file: &FileRef) -> String {
    if file.source.is_empty() {
        clean_reference(&file.path.to_string_lossy())
    } else {
        clean_reference(&file.source)
    }
}

/// `path` made absolute against the working folder and cleaned, without following links.
fn absolute(path: &Path) -> Result<PathBuf, Error> {
    absolute_path(path).map_err(|source| Error::Resolve { source })
}

/// The folder of the project file `file`.
fn folder_of(file: &Path) -> &Path {
    file.parent().unwrap_or(Path::new(""))
}

/// A reference as written, the same on every platform: `\` read as `/`, empty and `.` parts
/// dropped, and `..` folded into the part before it (kept at the start of a relative
/// reference, dropped at the root of an absolute one).
fn clean_reference(reference: &str) -> String {
    let unified = reference.replace('\\', "/");
    let absolute = unified.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => match parts.last() {
                Some(&last) if last != ".." => {
                    parts.pop();
                }
                _ if absolute => {}
                _ => parts.push(".."),
            },
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::Tileset;

    use super::*;

    #[test]
    fn keys_are_relative_to_the_files_folder_with_slashes_and_folded_dots() {
        let file = Path::new("/game/levels/better-tiled.json");
        let key = |path: &str| key(file, Path::new(path)).unwrap();
        assert_eq!(key("/game/levels/caves/deep.tmx"), "caves/deep.tmx");
        assert_eq!(key("/game/levels/./caves/x/../deep.tmx"), "caves/deep.tmx");
        assert_eq!(key("/game/levels/top.tmx"), "top.tmx");
        assert_eq!(key("/game/rules/../rules.txt"), "../rules.txt", "a .. key");
        assert_eq!(key("/elsewhere/godot"), "../../elsewhere/godot");
        let messy = Path::new("/game/./levels/../levels/better-tiled.json");
        assert_eq!(
            super::key(messy, Path::new("/game/levels/caves/deep.tmx")).unwrap(),
            "caves/deep.tmx",
            "the file's own path is folded too"
        );
    }

    #[test]
    fn keys_resolve_back_to_paths_in_the_files_form() {
        let file = Path::new("/game/levels/better-tiled.json");
        assert_eq!(
            resolve(file, "caves/deep.tmx"),
            PathBuf::from("/game/levels/caves/deep.tmx")
        );
        assert_eq!(
            resolve(file, "../rules/./walls.txt"),
            PathBuf::from("/game/rules/walls.txt")
        );
        assert_eq!(resolve(file, "/abs/godot"), PathBuf::from("/abs/godot"));
        for path in ["/game/levels/caves/deep.tmx", "/game/rules.txt", "/other/x"] {
            let key = key(file, Path::new(path)).unwrap();
            assert_eq!(resolve(file, &key), PathBuf::from(path), "{key}");
        }
        assert_eq!(
            resolve(Path::new("better-tiled.json"), "maps/a.tmx"),
            PathBuf::from("maps/a.tmx"),
            "a relative file gives relative paths"
        );
    }

    #[test]
    fn relative_paths_are_taken_from_the_working_folder() {
        let file = std::env::current_dir().unwrap().join("better-tiled.json");
        assert_eq!(key(&file, Path::new("maps/./a.tmx")).unwrap(), "maps/a.tmx");
        assert_eq!(
            key(Path::new("better-tiled.json"), Path::new("maps/a.tmx")).unwrap(),
            "maps/a.tmx"
        );
    }

    #[cfg(windows)]
    #[test]
    fn keys_use_slashes_on_windows_and_another_drive_has_none() {
        let file = Path::new(r"C:\game\better-tiled.json");
        assert_eq!(
            key(file, Path::new(r"C:\game\levels\caves\deep.tmx")).unwrap(),
            "levels/caves/deep.tmx"
        );
        assert_eq!(
            key(file, Path::new(r"C:\rules\walls.txt")).unwrap(),
            "../rules/walls.txt"
        );
        assert!(matches!(
            key(file, Path::new(r"D:\game\a.tmx")),
            Err(Error::Unrelated { .. })
        ));
        assert_eq!(
            resolve(file, "levels/caves/deep.tmx"),
            PathBuf::from(r"C:\game\levels\caves\deep.tmx")
        );
    }

    fn tileset_named(name: &str) -> Arc<Tileset> {
        Arc::new(Tileset {
            name: name.to_owned(),
            ..Tileset::default()
        })
    }

    fn external(source: &str, path: &str) -> TilesetRef {
        TilesetRef {
            file: Some(FileRef::new(source, path)),
            first_gid: None,
            tileset: tileset_named("ignored"),
        }
    }

    #[test]
    fn tilesets_are_named_by_the_maps_reference_or_their_name() {
        assert_eq!(
            tileset_key(&TilesetRef::embedded(tileset_named("Ground"))),
            "#Ground"
        );
        let key = |source: &str| tileset_key(&external(source, "/resolved/a.tsx"));
        assert_eq!(key("../tiles/a.tsx"), "../tiles/a.tsx");
        assert_eq!(key("./../tiles/./a.tsx"), "../tiles/a.tsx");
        assert_eq!(
            key("..\\tiles\\a.tsx"),
            "../tiles/a.tsx",
            "as Windows writes it"
        );
        assert_eq!(key("tiles//x/../a.tsx"), "tiles/a.tsx");
        assert_eq!(key("../../a.tsx"), "../../a.tsx");
        assert_eq!(key("/abs/../tiles/a.tsx"), "/tiles/a.tsx");
        assert_eq!(key("/../a.tsx"), "/a.tsx");
        assert_eq!(key(":/automap-tiles.tsx"), ":/automap-tiles.tsx");
        assert_eq!(key("C:/art/a.tsx"), "C:/art/a.tsx");
        assert_eq!(key(""), "/resolved/a.tsx", "not written yet");
    }
}
