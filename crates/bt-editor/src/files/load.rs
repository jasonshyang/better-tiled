//! Loading a map on a worker: the job the editor hands out, and what comes back. The job reads
//! the map, its external tilesets and (when no project is open) the nearest project above it,
//! and hashes each file, so the editor can later tell its own writes from others'.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::layers::{Restored, restore};
use bt_core::{CustomTypes, Project};
use bt_project::AutoMapRecord;
use bt_tmx::{Loader, Warning};
use bt_utils::hash::fnv1a_64;

use crate::automap::read_layers;
use crate::store::Root;

/// Where a load's text comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadSource {
    /// The file itself.
    File,
    /// A recovery snapshot of it.
    Snapshot(PathBuf),
    /// In-memory auxiliary content to parse and expand without writing a temporary file.
    Text(String),
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
    /// A generic JSON file, classified by its top-level type.
    Auto,
    /// A world file.
    World,
    /// An object template.
    Template,
    /// A project file opened explicitly.
    Project,
    /// A map.
    Map,
    /// A tileset in a file of its own.
    Tileset,
}

/// Why a file is loaded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoadPurpose {
    /// Load a replacement for an exact Issues reference.
    Locate {
        doc: crate::DocumentId,
        target: crate::LocateTarget,
        version: u64,
    },
    /// To open it.
    Open,
    /// Cache a visible neighbouring map without opening a document tab.
    WorldMap(crate::store::AssetId),
    /// Choose a template for insertion without opening an editing tab.
    ChooseTemplate(crate::DocumentId),
    /// Probe and adopt a map before adding its rectangle to a world.
    WorldPatterns { world: crate::AssetId, version: u64 },
    /// Probe a map before adding it to a world.
    AddWorldMap {
        doc: crate::DocumentId,
        world: crate::AssetId,
    },
    /// It changed on disk: its asset takes the new content.
    Reload(crate::store::AssetId),
    /// Add External Tileset: the map of a document refers to it.
    AddToMap(crate::document::DocumentId),
}

/// A load to run on a worker; [`LoadJob::run`] does it.
#[derive(Clone, Debug)]
pub struct LoadJob {
    pub(crate) epoch: u64,
    pub(crate) path: PathBuf,
    pub(crate) source: LoadSource,
    pub(crate) project: ProjectChoice,
    pub(crate) kind: LoadKind,
    pub(crate) purpose: LoadPurpose,
    /// The user's home folder, where the search for the map's project file stops.
    pub(crate) home: Option<PathBuf>,
    /// For a recovered map, its AutoMap layers' setup as its snapshot's description keeps it.
    pub(crate) recovered_layers: Option<AutoMapRecord>,
}

/// A loaded map or tileset and what came with it.
#[derive(Debug)]
pub(crate) struct LoadedFiles {
    /// World files beside a map or one folder above, discovered on the worker.
    pub(crate) nearby_worlds: Vec<PathBuf>,
    /// Pattern-expanded map positions, for world loads.
    pub(crate) world_maps: Vec<bt_core::WorldMap>,
    pub(crate) content: Root,
    pub(crate) warnings: Vec<Warning>,
    /// The map file's hash; `None` for a recovered map (not what the file holds).
    pub(crate) content_hash: Option<u64>,
    pub(crate) dependencies: Vec<LoadedAsset>,
    pub(crate) project: Option<LoadedProject>,
    /// The loader, with the templates the map uses, for writing it.
    pub(crate) loader: Loader,
    /// The map's AutoMap layers as its project file keeps them, rebuilt on the map read; `None`
    /// when it keeps none, or why they could not be read.
    pub(crate) layers: Result<Option<Restored>, String>,
}

/// An external tileset read with a map.
#[derive(Debug)]
pub(crate) struct LoadedAsset {
    pub(crate) path: PathBuf,
    pub(crate) root: Root,
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
    pub(crate) epoch: u64,
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
            LoadSource::File | LoadSource::Text(_) => None,
            LoadSource::Snapshot(snapshot) => Some(snapshot.clone()),
        };
        let canonical = std::fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        Loaded {
            epoch: self.epoch,
            path: self.path.clone(),
            outcome: self.load(&canonical),
            canonical,
            recovered_from,
            purpose: self.purpose,
        }
    }

    fn load(&self, canonical: &Path) -> Result<LoadedFiles, String> {
        let kind = if self.kind == LoadKind::Auto {
            let source = match &self.source {
                LoadSource::Snapshot(path) => path,
                _ => &self.path,
            };
            let text = std::fs::read_to_string(source).map_err(|error| error.to_string())?;
            let value: serde_json::Value =
                serde_json::from_str(&text).map_err(|error| error.to_string())?;
            match value.get("type").and_then(|value| value.as_str()) {
                Some("tileset") => LoadKind::Tileset,
                Some("template") => LoadKind::Template,
                Some("world") => LoadKind::World,
                Some("project") => LoadKind::Project,
                _ => LoadKind::Map,
            }
        } else {
            self.kind
        };
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
        if matches!(
            kind,
            LoadKind::World | LoadKind::Template | LoadKind::Project
        ) {
            let text = match &self.source {
                LoadSource::Text(text) => text.clone(),
                LoadSource::File | LoadSource::Snapshot(_) => {
                    let source = match &self.source {
                        LoadSource::Snapshot(path) => path,
                        _ => &self.path,
                    };
                    std::fs::read_to_string(source)
                        .map_err(|error| format!("Cannot read {}: {error}", source.display()))?
                }
            };
            let loaded = match kind {
                LoadKind::World => {
                    let loaded = loader
                        .parse_world(&text, &self.path)
                        .map_err(|error| error.to_string())?;
                    bt_tmx::Loaded {
                        value: Root::World(Arc::new(loaded.value)),
                        warnings: loaded.warnings,
                    }
                }
                LoadKind::Template => {
                    let loaded = loader
                        .parse_template(&text, &self.path)
                        .map_err(|error| error.to_string())?;
                    if loaded.value.object.is_none() {
                        return Err("The template has no base object".into());
                    }
                    bt_tmx::Loaded {
                        value: Root::Template(loaded.value),
                        warnings: loaded.warnings,
                    }
                }
                LoadKind::Project => {
                    let loaded = bt_tmx::parse_project(&text, &self.path)
                        .map_err(|error| error.to_string())?;
                    bt_tmx::Loaded {
                        value: Root::Project(Arc::new(loaded.value)),
                        warnings: loaded.warnings,
                    }
                }
                _ => return Err("The auxiliary load has the wrong file kind".into()),
            };
            let world_maps = match &loaded.value {
                Root::World(world) => bt_tmx::resolve_world_maps(world, &self.path),
                _ => Vec::new(),
            };
            let dependencies = match &loaded.value {
                Root::Template(template) => loaded_tilesets(&template.tilesets),
                _ => Vec::new(),
            };
            let content_hash =
                matches!(self.source, LoadSource::File).then(|| fnv1a_64(text.as_bytes()));
            return Ok(LoadedFiles {
                nearby_worlds: Vec::new(),
                content: loaded.value,
                warnings: loaded.warnings,
                content_hash,
                dependencies,
                world_maps,
                project,
                loader,
                layers: Ok(None),
            });
        }
        if kind == LoadKind::Tileset {
            let (loaded, content_hash) = match &self.source {
                LoadSource::File => (
                    loader
                        .load_tileset(&self.path)
                        .map_err(|error| error.to_string())?,
                    Some(hash_file(&self.path)?),
                ),
                LoadSource::Text(_) => {
                    return Err("In-memory loads require an auxiliary file kind".into());
                }
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
                nearby_worlds: Vec::new(),
                world_maps: Vec::new(),
                content: Root::Tileset(loaded.value),
                warnings: loaded.warnings,
                content_hash,
                dependencies: Vec::new(),
                project,
                loader,
                layers: Ok(None),
            });
        }
        let loaded = match &self.source {
            LoadSource::File => loader
                .load_map(&self.path)
                .map_err(|error| error.to_string())?,
            LoadSource::Text(_) => {
                return Err("In-memory loads require an auxiliary file kind".into());
            }
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
            LoadSource::Snapshot(_) | LoadSource::Text(_) => None,
        };
        let mut dependencies = loaded_tilesets(&loaded.value.tilesets);
        let mut templates = std::collections::BTreeSet::new();
        loaded.value.for_each_layer(|layer, _| {
            if let Some(objects) = layer.as_object_layer() {
                templates.extend(
                    objects.objects.iter().filter_map(|object| {
                        object.template.as_ref().map(|file| file.path.clone())
                    }),
                );
            }
        });
        for path in templates {
            let Some(template) = loader
                .cached_template(&path)
                .filter(|template| template.object.is_some())
            else {
                continue;
            };
            if let Ok(content_hash) = hash_file(&path) {
                dependencies.extend(loaded_tilesets(&template.tilesets));
                dependencies.push(LoadedAsset {
                    path,
                    root: Root::Template(Arc::clone(template)),
                    content_hash,
                });
            }
        }
        // A recovered map is what was painted already; AutoMap layers are read for the file.
        let layers = match &self.source {
            LoadSource::File => read_layers(canonical, self.home.as_deref(), &loaded.value),
            LoadSource::Text(_) => Ok(None),
            LoadSource::Snapshot(_) => Ok(self
                .recovered_layers
                .as_ref()
                .map(|record| restore(record, PathBuf::from(&record.rules), &loaded.value))),
        };
        Ok(LoadedFiles {
            nearby_worlds: if self.purpose == LoadPurpose::Open {
                nearby_worlds(canonical)
            } else {
                Vec::new()
            },
            world_maps: Vec::new(),
            content: Root::Map(Arc::new(loaded.value)),
            warnings: loaded.warnings,
            content_hash,
            dependencies,
            project,
            loader,
            layers,
        })
    }
}

fn loaded_tilesets(references: &[bt_core::TilesetRef]) -> Vec<LoadedAsset> {
    references
        .iter()
        .filter_map(|reference| {
            let file = reference.file.as_ref()?;
            // Missing tilesets are already reported by the loader; no file exists to hash.
            let hash = hash_file(&file.path).ok()?;
            Some(LoadedAsset {
                path: file.path.clone(),
                root: Root::Tileset(Arc::clone(&reference.tileset)),
                content_hash: hash,
            })
        })
        .collect()
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

/// Tiled searches beside the map and in its parent's folder, in stable filename order.
fn nearby_worlds(path: &Path) -> Vec<PathBuf> {
    let mut worlds = Vec::new();
    let Some(folder) = path.parent() else {
        return worlds;
    };
    for folder in std::iter::once(folder).chain(folder.parent()) {
        if let Ok(entries) = std::fs::read_dir(folder) {
            worlds.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|extension| extension == "world")
                    }),
            );
        }
    }
    worlds.sort();
    worlds
}
