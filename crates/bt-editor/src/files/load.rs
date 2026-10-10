//! Loading a map on a worker: the job the editor hands out, and what comes back. The job reads
//! the map, its external tilesets and (when no project is open) the nearest project above it,
//! and hashes each file, so the editor can later tell its own writes from others'.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{CustomTypes, Map, Project, Tileset};
use bt_tmx::{Loader, Warning};
use bt_utils::hash::fnv1a_64;

/// Where a load's text comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadSource {
    /// The file itself.
    File,
    /// A recovery snapshot of it.
    Snapshot(PathBuf),
}

/// Which custom types a load decodes properties with.
#[derive(Clone, Debug)]
pub(crate) enum ProjectChoice {
    /// The open project's.
    Known(Arc<CustomTypes>),
    /// The nearest project above the map's, if any, searching no higher than `home`.
    FindNearest { home: Option<PathBuf> },
}

/// What a file holds.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadKind {
    /// A map.
    Map,
    /// A tileset in a file of its own.
    Tileset,
}

/// Why a file is loaded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadPurpose {
    /// To open it.
    Open,
    /// It changed on disk: its asset takes the new content.
    Reload(crate::store::AssetId),
    /// Add External Tileset: the map of a document refers to it.
    AddToMap(crate::document::DocumentId),
}

/// A load to run on a worker; [`LoadJob::run`] does it.
#[derive(Clone, Debug)]
pub struct LoadJob {
    pub(crate) path: PathBuf,
    pub(crate) source: LoadSource,
    pub(crate) project: ProjectChoice,
    pub(crate) kind: LoadKind,
    pub(crate) purpose: LoadPurpose,
}

/// What a load read.
#[derive(Debug)]
pub(crate) enum LoadedContent {
    /// A map.
    Map(Box<Map>),
    /// A tileset.
    Tileset(Arc<Tileset>),
}

/// A loaded map or tileset and what came with it.
#[derive(Debug)]
pub(crate) struct LoadedFiles {
    pub(crate) content: LoadedContent,
    pub(crate) warnings: Vec<Warning>,
    /// The map file's hash; `None` for a recovered map (not what the file holds).
    pub(crate) content_hash: Option<u64>,
    pub(crate) tilesets: Vec<LoadedTileset>,
    pub(crate) project: Option<LoadedProject>,
    /// The loader, with the templates the map uses, for writing it.
    pub(crate) loader: Loader,
}

/// An external tileset read with a map.
#[derive(Debug)]
pub(crate) struct LoadedTileset {
    pub(crate) path: PathBuf,
    pub(crate) tileset: Arc<Tileset>,
    pub(crate) content_hash: u64,
}

/// A project found above a map.
#[derive(Debug)]
pub(crate) struct LoadedProject {
    pub(crate) path: PathBuf,
    pub(crate) project: Project,
    pub(crate) content_hash: u64,
}

/// What a load gave: the map and its files, or why not.
#[derive(Debug)]
pub struct Loaded {
    pub(crate) path: PathBuf,
    /// The file's canonical path (its identity), or the path asked for when it has none (a
    /// recovered map whose file is gone).
    pub(crate) canonical: PathBuf,
    pub(crate) outcome: Result<LoadedFiles, String>,
    pub(crate) recovered_from: Option<PathBuf>,
    pub(crate) purpose: LoadPurpose,
}

impl Loaded {
    /// The file loaded.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl LoadJob {
    /// The file to load.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the files; run it on a worker.
    pub fn run(&self) -> Loaded {
        let recovered_from = match &self.source {
            LoadSource::File => None,
            LoadSource::Snapshot(snapshot) => Some(snapshot.clone()),
        };
        Loaded {
            path: self.path.clone(),
            canonical: std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone()),
            outcome: self.load(),
            recovered_from,
            purpose: self.purpose,
        }
    }

    fn load(&self) -> Result<LoadedFiles, String> {
        let project = match &self.project {
            ProjectChoice::Known(_) => None,
            ProjectChoice::FindNearest { home } => self
                .path
                .parent()
                .and_then(|folder| find_project(folder, home.as_deref()))
                .map(|path| read_project(&path))
                .transpose()?,
        };
        let custom_types = match &self.project {
            ProjectChoice::Known(types) => Arc::clone(types),
            ProjectChoice::FindNearest { .. } => Arc::new(
                project
                    .as_ref()
                    .map(|found| found.project.custom_types.clone())
                    .unwrap_or_default(),
            ),
        };
        let mut loader = Loader::builder().custom_types(custom_types).build();
        if self.kind == LoadKind::Tileset {
            let (loaded, content_hash) = match &self.source {
                LoadSource::File => (
                    loader
                        .load_tileset(&self.path)
                        .map_err(|error| error.to_string())?,
                    Some(hash_file(&self.path)?),
                ),
                LoadSource::Snapshot(snapshot) => {
                    let text = std::fs::read_to_string(snapshot)
                        .map_err(|error| format!("cannot read {}: {error}", snapshot.display()))?;
                    let loaded = loader
                        .parse_tileset(&text, &self.path)
                        .map_err(|error| error.to_string())?;
                    (loaded, None)
                }
            };
            return Ok(LoadedFiles {
                content: LoadedContent::Tileset(loaded.value),
                warnings: loaded.warnings,
                content_hash,
                tilesets: Vec::new(),
                project,
                loader,
            });
        }
        let loaded = match &self.source {
            LoadSource::File => loader
                .load_map(&self.path)
                .map_err(|error| error.to_string())?,
            LoadSource::Snapshot(snapshot) => {
                let text = std::fs::read_to_string(snapshot)
                    .map_err(|error| format!("cannot read {}: {error}", snapshot.display()))?;
                loader
                    .parse_map(&text, &self.path)
                    .map_err(|error| error.to_string())?
            }
        };
        let content_hash = match &self.source {
            LoadSource::File => Some(hash_file(&self.path)?),
            LoadSource::Snapshot(_) => None,
        };
        let mut tilesets = Vec::new();
        for reference in &loaded.value.tilesets {
            let Some(file) = &reference.file else {
                continue;
            };
            // A missing tileset is a placeholder with a warning; it has no file to hash.
            let Ok(hash) = hash_file(&file.path) else {
                continue;
            };
            tilesets.push(LoadedTileset {
                path: file.path.clone(),
                tileset: Arc::clone(&reference.tileset),
                content_hash: hash,
            });
        }
        Ok(LoadedFiles {
            content: LoadedContent::Map(Box::new(loaded.value)),
            warnings: loaded.warnings,
            content_hash,
            tilesets,
            project,
            loader,
        })
    }
}

/// The FNV-1a hash of a file's bytes.
pub(crate) fn hash_file(path: &Path) -> Result<u64, String> {
    std::fs::read(path)
        .map(|bytes| fnv1a_64(&bytes))
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// The nearest `.tiled-project` above `folder` (the first by name in a folder with several),
/// stopping at the top of a repository and below `home`.
fn find_project(folder: &Path, home: Option<&Path>) -> Option<PathBuf> {
    bt_utils::paths::search_up(folder, home, |dir| {
        let mut projects: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "tiled-project"))
            .collect();
        projects.sort();
        projects.into_iter().next()
    })
}

fn read_project(path: &Path) -> Result<LoadedProject, String> {
    let loaded = bt_tmx::load_project(path).map_err(|error| error.to_string())?;
    Ok(LoadedProject {
        path: path.to_path_buf(),
        project: loaded.value,
        content_hash: hash_file(path)?,
    })
}
