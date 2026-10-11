//! What to write as the resolved JSON and where: the map, its file, the custom types its
//! classes resolve with; the JSON made in one walk over the map, and written atomically.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{CustomTypes, Map, Orientation, TilesetRef};
use bt_utils::atomic_write::{WriteMode, write_file};
use bt_utils::paths::absolute_path;

use super::RESOLVED_FORMAT;
use super::document::ResolvedMap;
use super::error::{ResolvedError, ResolvedWarning};
use super::grid::{map_entry, written_area};
use super::layers::layer_entries;
use super::layout::json_text;
use super::palette::{Palette, tileset_entries};
use super::resolving::{Folders, Resolving};

/// A map to write as the resolved JSON: the map, where its file is, and the custom types its
/// classes resolve with.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedRequest<'m> {
    map: &'m Map,
    map_path: &'m Path,
    types: Option<&'m CustomTypes>,
}

/// The resolved JSON of a map, and what making it warned about.
#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    /// The JSON's contents.
    pub(crate) document: ResolvedMap,
    /// What was left out or could not be said.
    pub(crate) warnings: Vec<ResolvedWarning>,
}

impl<'m> ResolvedRequest<'m> {
    /// The resolved JSON of `map`, whose file is at `map_path`: what its `source` names and
    /// what its own `file` properties are relative to.
    pub fn new(map: &'m Map, map_path: &'m Path) -> ResolvedRequest<'m> {
        ResolvedRequest {
            map,
            map_path,
            types: None,
        }
    }

    /// The custom types (the project's) the map's classes resolve with. Without them an
    /// element's properties are its own and its tile's.
    pub fn custom_types(mut self, types: &'m CustomTypes) -> ResolvedRequest<'m> {
        self.types = Some(types);
        self
    }

    /// Writes the JSON to `out`, atomically, with its paths relative to `out`'s folder, and
    /// says what it warned about. An oblique map, a read-only file and a folder that can't be
    /// written to are refused, and then nothing is written.
    pub fn write(&self, out: &Path) -> Result<Vec<ResolvedWarning>, ResolvedError> {
        let out = absolute_path(out)?;
        let (Some(folder), Some(name)) = (out.parent(), out.file_name()) else {
            return Err(ResolvedError::Folder {
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a file"),
                path: out,
            });
        };
        // Image paths are canonical when the map was read through its canonical path, as bt
        // reads maps, so the folder they are made relative to is too.
        let folder = std::fs::canonicalize(folder).map_err(|source| ResolvedError::Folder {
            path: out.clone(),
            source,
        })?;
        let out = folder.join(name);
        let resolved = self.resolve_for(&out)?;
        let text = json_text(&resolved.document)?;
        write_file(&out, text.as_bytes(), WriteMode::Atomic)?;
        Ok(resolved.warnings)
    }

    /// The JSON as it would be written to `out`.
    pub(crate) fn resolve_for(&self, out: &Path) -> Result<Resolved, ResolvedError> {
        if self.map.orientation == Orientation::Oblique {
            return Err(ResolvedError::Oblique {
                map: self.map_path.to_path_buf(),
            });
        }
        let out = absolute_path(out)?;
        let map_path = absolute_path(self.map_path)?;
        let folders = Folders {
            json: folder_of(&out),
            map: folder_of(&map_path),
        };
        let no_types = CustomTypes::new();
        let types = self.types.unwrap_or(&no_types);
        let map = rebased(self.map, &folders);
        let mut resolving = Resolving::new(&map, types, folders);
        let area = written_area(&map);
        let mut palette = Palette::default();
        let layers = layer_entries(&mut resolving, &mut palette, area);
        let tilesets = tileset_entries(&mut resolving);
        let name = map_path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let source = resolving.relative(&map_path);
        let map_entry = map_entry(&resolving, area, name, source);
        Ok(Resolved {
            document: ResolvedMap {
                format: RESOLVED_FORMAT,
                map: map_entry,
                tilesets,
                tiles: palette.into_tiles(),
                layers,
            },
            warnings: resolving.into_warnings(),
        })
    }
}

/// The folder a file is in (`file` is absolute, so it has one unless it is the root).
fn folder_of(file: &Path) -> PathBuf {
    file.parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

/// `map` with its `file` properties written relative to the JSON file's folder, as embedding
/// its tilesets in a file there would leave them: the map's own and its embedded tilesets'
/// were relative to the map file's folder, a tileset file's to that file's folder.
fn rebased(map: &Map, folders: &Folders) -> Map {
    let json_dir = folders.json.as_path();
    let mut map = map.clone();
    bt_tmx::rebase_file_references(&mut map, &folders.map, json_dir);
    for used in &mut map.tilesets {
        let Some(tileset_dir) = used.file.as_ref().and_then(|file| file.path.parent()) else {
            continue;
        };
        if !tileset_dir.is_absolute() {
            // Tiled's built-in rule tileset (`:/automap-tiles.tsx`) is in no folder.
            continue;
        }
        let mut holder = Map {
            tilesets: vec![TilesetRef::embedded(Arc::clone(&used.tileset))],
            ..Map::default()
        };
        bt_tmx::rebase_file_references(&mut holder, tileset_dir, json_dir);
        if let Some(embedded) = holder.tilesets.pop() {
            used.tileset = embedded.tileset;
        }
    }
    map
}
