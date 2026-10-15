//! Opening files: the load handed out, then the asset (a map with its tilesets and project, or a
//! tileset file) and a document showing it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::document::{DocumentId, Selection, ViewState};
use crate::editor::Editor;
use crate::effect::Effect;
use crate::files::{
    LoadJob, LoadKind, LoadPurpose, LoadSource, Loaded, LoadedFiles, ProjectChoice,
};
use crate::notice::Notice;
use crate::store::{AssetKind, NewAsset, Root};

impl Editor {
    /// Opens a file: its tab when it is open, else a load for a worker.
    pub(crate) fn open(&mut self, path: &Path) -> Vec<Effect> {
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => {
                return vec![Effect::Notify(Notice::error(format!(
                    "Cannot open {}: {error}",
                    path.display()
                )))];
            }
        };
        if let Some(doc) = self.document_of_path(&path) {
            self.documents.set_current(doc);
            self.session_changed();
            return Vec::new();
        }
        vec![Effect::Load(self.load_job(path, LoadSource::File))]
    }

    /// A load of `path`, with the open project's types or a search for the nearest project: a
    /// tileset for a `.tsx` or `.tsj` file, else a map.
    pub(crate) fn load_job(&self, path: PathBuf, source: LoadSource) -> LoadJob {
        let project = match self.project_file() {
            Some(_) => ProjectChoice::Known(Arc::new(self.custom_types().clone())),
            None => ProjectChoice::FindNearest {
                home: self.files.home.clone(),
            },
        };
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = match extension.as_str() {
            "tsx" | "tsj" => LoadKind::Tileset,
            "tx" | "tj" => LoadKind::Template,
            "world" => LoadKind::World,
            "tiled-project" => LoadKind::Project,
            _ => LoadKind::Map,
        };
        LoadJob {
            path,
            source,
            project,
            kind,
            purpose: LoadPurpose::Open,
            home: self.files.home.clone(),
            recovered_layers: None,
        }
    }

    /// The document showing the asset of a file.
    fn document_of_path(&self, path: &Path) -> Option<DocumentId> {
        let asset = self.store.by_path(path)?;
        self.documents
            .in_order()
            .find(|doc| doc.asset() == asset)
            .map(|doc| doc.id())
    }

    /// Takes in a finished load: the project, the tilesets (the store's when it has the file),
    /// the map, and a document showing it.
    pub(crate) fn finish_load(&mut self, mut loaded: Loaded) -> Vec<Effect> {
        if let Ok(files) = &mut loaded.outcome
            && let Root::Map(map) = &mut files.content
        {
            for dependency in &files.dependencies {
                if let Root::Template(before) = &dependency.root
                    && let Some(after) = self
                        .store
                        .by_path(&dependency.path)
                        .and_then(|id| self.store.template(id))
                    && let Err(error) = bt_edit::refresh_template_instances(
                        Arc::make_mut(map),
                        &dependency.path,
                        before,
                        after,
                    )
                {
                    loaded.outcome = Err(error.to_string());
                    break;
                }
            }
        }
        let mut effects = Vec::new();
        if let LoadPurpose::WorldPatterns { world, version } = loaded.purpose {
            return self.finish_world_patterns(world, version, loaded);
        }
        if let LoadPurpose::AddWorldMap { doc, world } = loaded.purpose {
            return self.finish_adding_world_map(doc, world, loaded);
        }
        if let LoadPurpose::WorldMap(world) = loaded.purpose {
            return self.cache_world_map(world, loaded);
        }
        if let LoadPurpose::Reload(asset) = loaded.purpose {
            let mut effects = match loaded.outcome {
                Ok(files) => self.finish_reload(asset, files),
                Err(message) => vec![Effect::Notify(
                    Notice::error(format!(
                        "Cannot reload {}",
                        bt_utils::paths::display_name(&loaded.path)
                    ))
                    .with_detail(message),
                )],
            };
            effects.extend(self.settle_watch(asset));
            return effects;
        }
        if let LoadPurpose::AddToMap(doc) = loaded.purpose {
            return self.finish_adding_tileset(doc, loaded);
        }
        let files = match loaded.outcome {
            Ok(files) => files,
            Err(message) => {
                return vec![Effect::Notify(
                    Notice::error(format!(
                        "Cannot open {}",
                        bt_utils::paths::display_name(&loaded.path)
                    ))
                    .with_detail(message),
                )];
            }
        };
        let path = loaded.canonical.clone();
        let map = match files.content {
            root @ (Root::World(_) | Root::Template(_) | Root::Project(_)) => {
                return self.open_auxiliary_file(
                    &path,
                    loaded.recovered_from,
                    LoadedFiles {
                        content: root,
                        ..files
                    },
                );
            }
            Root::Map(map) => map,
            Root::Tileset(tileset) => {
                let read = ReadTileset {
                    tileset,
                    content_hash: files.content_hash,
                    warnings: files.warnings,
                    recovered_from: loaded.recovered_from,
                };
                return self.open_tileset_document(&path, read);
            }
        };
        if loaded.recovered_from.is_none()
            && let Some(doc) = self.document_of_path(&path)
        {
            self.documents.set_current(doc);
            return effects;
        }
        if let Some(project) = files.project
            && self.project_file().is_none()
        {
            effects.extend(self.drop_untitled_project(&project.path));
            let id = self.store.insert(NewAsset {
                root: Root::Project(Arc::new(project.project)),
                path: Some(project.path),
                content_hash: Some(project.content_hash),
                warnings: Vec::new(),
            });
            self.files.project = Some(id);
        }
        let mut map = (*map).clone();
        self.adopt_dependencies(files.dependencies);
        self.store.refresh_tilesets(&mut map);
        let mut layers = match files.layers {
            Ok(layers) => layers,
            Err(problem) => {
                effects.push(Effect::Notify(Notice::warning(format!(
                    "The AutoMap layers of {} were not read: {problem}",
                    bt_utils::paths::display_name(&path)
                ))));
                None
            }
        };
        if let Some(restored) = &mut layers {
            self.store.refresh_tilesets(&mut restored.painted);
        }
        // With AutoMap layers, the file holds the rules' result: the map edited is what was
        // painted, and the file's map is kept to check the rules still make it.
        let saved = Arc::new(map);
        let root = match &layers {
            Some(restored) => Arc::new(restored.painted.clone()),
            None => Arc::clone(&saved),
        };
        let warning_count = files.warnings.len();
        // A recovered map replaces nothing: if its file is open, it opens beside it, unsaved.
        let file = if loaded.recovered_from.is_some() && self.store.by_path(&path).is_some() {
            None
        } else {
            Some(path.clone())
        };
        let asset = if loaded.recovered_from.is_none()
            && let Some(id) = self
                .store
                .by_path(&path)
                .filter(|id| self.store.map(*id).is_some())
        {
            if self.store.get(id).is_some_and(|asset| !asset.is_dirty()) {
                let version = self.store.get(id).map_or(1, |asset| asset.version());
                self.store.set_root(id, Root::Map(root), version);
            }
            id
        } else {
            self.store.insert(NewAsset {
                root: Root::Map(root),
                path: file,
                content_hash: files.content_hash,
                warnings: files.warnings,
            })
        };
        if loaded.recovered_from.is_some() {
            self.store.forget_disk(asset);
        }
        self.files.loaders.insert(asset, files.loader);
        self.sync_template_caches();
        let doc = self.open_document(asset);
        if let Some(restored) = layers {
            let saved = loaded.recovered_from.is_none().then_some(saved);
            effects.extend(self.restore_layers(doc, restored, saved));
        }
        self.restore_view_state(doc, &path);
        self.remember_recent(&path);
        if let Some(snapshot) = loaded.recovered_from {
            effects.push(Effect::DeleteSnapshot(snapshot));
        }
        if warning_count > 0 {
            effects.push(Effect::Notify(Notice::warning(format!(
                "{} opened with {}",
                bt_utils::paths::display_name(&path),
                bt_utils::text::plural(warning_count, "warning", "warnings")
            ))));
        }
        for world in files.nearby_worlds {
            if self.store.by_path(&world).is_none()
                && self.files.world_requested.insert(world.clone())
            {
                effects.push(Effect::Load(self.load_job(world, LoadSource::File)));
            }
        }
        self.session_changed();
        effects
    }

    /// Shows a tileset file in a tab of its own: the store's asset when a map uses it already
    /// (its unsaved changes kept), else the tileset read. A recovered tileset becomes the
    /// asset's content, unsaved, whatever the store held.
    fn open_tileset_document(&mut self, path: &Path, read: ReadTileset) -> Vec<Effect> {
        let mut effects = Vec::new();
        let asset = match (self.store.by_path(path), &read.recovered_from) {
            (Some(asset), None) => asset,
            (Some(asset), Some(_)) => {
                let version = self.store.next_version(asset);
                self.store
                    .set_root(asset, Root::Tileset(read.tileset), version);
                asset
            }
            (None, _) => self.store.insert(NewAsset {
                root: Root::Tileset(read.tileset),
                path: Some(path.to_path_buf()),
                content_hash: read.content_hash,
                warnings: read.warnings,
            }),
        };
        if let Some(snapshot) = read.recovered_from {
            self.store.forget_disk(asset);
            effects.push(Effect::DeleteSnapshot(snapshot));
        }
        match self.document_of_path(path) {
            Some(doc) => self.documents.set_current(doc),
            None => {
                self.open_document(asset);
            }
        }
        self.remember_recent(path);
        self.session_changed();
        effects
    }

    /// Puts the external tilesets a load read into the store, unless it has their files.
    pub(super) fn adopt_dependencies(
        &mut self,
        dependencies: Vec<crate::files::load::LoadedAsset>,
    ) {
        for dependency in dependencies {
            if self.store.by_path(&dependency.path).is_none() {
                self.store.insert(NewAsset {
                    root: dependency.root,
                    path: Some(dependency.path),
                    content_hash: Some(dependency.content_hash),
                    warnings: Vec::new(),
                });
            }
        }
    }

    /// Takes in what a file holds now that it changed on disk: a map's new content is a
    /// transaction of the document showing it (so the reload can be undone), a tileset's
    /// replaces the store's root everywhere. Either is then what is on disk.
    fn finish_reload(&mut self, asset: crate::store::AssetId, files: LoadedFiles) -> Vec<Effect> {
        let Some(content_hash) = files.content_hash else {
            return Vec::new();
        };
        let name = self
            .store
            .get(asset)
            .map(crate::store::Asset::display_name)
            .unwrap_or_default();
        self.adopt_dependencies(files.dependencies);
        match files.content {
            Root::World(world) => {
                let version = self.store.next_version(asset);
                self.files.world_patterns.insert(
                    asset,
                    files
                        .world_maps
                        .into_iter()
                        .skip(world.maps.len())
                        .collect(),
                );
                self.store.set_root(asset, Root::World(world), version);
            }
            Root::Template(template) => {
                let path = self
                    .store
                    .get(asset)
                    .and_then(|asset| asset.path())
                    .map(Path::to_path_buf);
                let owner = self
                    .documents
                    .in_order()
                    .find(|doc| doc.asset() == asset)
                    .map(|doc| doc.id())
                    .or_else(|| {
                        self.documents
                            .in_order()
                            .find(|doc| {
                                let Some(map) = self.store.map(doc.asset()) else {
                                    return false;
                                };
                                let mut found = false;
                                map.for_each_layer(|layer, _| {
                                    if let Some(objects) = layer.as_object_layer() {
                                        found |= objects.objects.iter().any(|object| {
                                            object.template.as_ref().is_some_and(|file| {
                                                Some(&file.path) == path.as_ref()
                                            })
                                        });
                                    }
                                });
                                found
                            })
                            .map(|doc| doc.id())
                    });
                if let Some(doc) = owner {
                    if let Err(error) = self.transact(doc, "Reload Template", |tx| {
                        *tx.template_mut(asset)? = (*template).clone();
                        Ok::<_, crate::EditError>(())
                    }) {
                        return vec![Effect::Notify(Notice::error(error.to_string()))];
                    }
                } else {
                    let working =
                        std::collections::BTreeMap::from([(asset, Root::Template(template))]);
                    match self.store.finish_working_roots(working) {
                        Ok(roots) => {
                            self.apply_roots(roots);
                        }
                        Err(error) => {
                            return vec![Effect::Notify(Notice::error(error.to_string()))];
                        }
                    }
                }
            }
            Root::Project(project) => {
                let version = self.store.next_version(asset);
                self.store.set_root(asset, Root::Project(project), version);
            }
            Root::Map(map) => {
                let mut map = (*map).clone();
                self.store.refresh_tilesets(&mut map);
                let Some(doc) = self
                    .documents
                    .in_order()
                    .find(|d| d.asset() == asset)
                    .map(|d| d.id())
                else {
                    let version = self.store.next_version(asset);
                    self.store
                        .set_root(asset, Root::Map(Arc::new(map)), version);
                    self.store.mark_saved(asset, content_hash);
                    return Vec::new();
                };
                let fallback = super::super::editor::topmost_leaf(&map);
                let result = self.transact(doc, "Reload", |tx| {
                    let current = tx.selection.current_layer;
                    if current.is_none_or(|id| map.layer(id).is_none()) {
                        tx.selection = fallback.map(Selection::of_layer).unwrap_or_default();
                    }
                    *tx.map_mut(asset)? = map;
                    Ok::<_, crate::error::EditError>(())
                });
                if let Err(error) = result {
                    return vec![Effect::Notify(Notice::error(format!(
                        "Cannot reload {name}: {error}"
                    )))];
                }
                self.files.loaders.insert(asset, files.loader);
                self.sync_template_caches();
            }
            Root::Tileset(tileset) => {
                let version = self.store.next_version(asset);
                self.store.set_root(asset, Root::Tileset(tileset), version);
            }
        }
        self.store.mark_saved(asset, content_hash);
        vec![Effect::Notify(Notice::info(format!(
            "{name} changed on disk and was reloaded"
        )))]
    }

    fn open_auxiliary_file(
        &mut self,
        path: &Path,
        recovered: Option<PathBuf>,
        files: LoadedFiles,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Root::Project(_) = &files.content {
            if self.project_file().is_some_and(|id| {
                self.store
                    .get(id)
                    .is_some_and(|asset| asset.path() != Some(path) && asset.is_dirty())
            }) {
                return vec![Effect::Notify(Notice::error(
                    "Save the current project before switching projects",
                ))];
            }
        } else if let Some(project) = files.project
            && self.project_file().is_none()
        {
            effects.extend(self.drop_untitled_project(&project.path));
            let id = self.store.insert(NewAsset {
                root: Root::Project(Arc::new(project.project)),
                path: Some(project.path),
                content_hash: Some(project.content_hash),
                warnings: Vec::new(),
            });
            self.files.project = Some(id);
        }
        self.adopt_dependencies(files.dependencies);
        let kind = files.content.kind();
        let pattern_maps = match &files.content {
            Root::World(world) => files
                .world_maps
                .into_iter()
                .skip(world.maps.len())
                .collect(),
            _ => Vec::new(),
        };
        let asset = match self.store.by_path(path) {
            Some(id) if self.store.get(id).is_some_and(|asset| asset.kind() == kind) => {
                if recovered.is_some() {
                    let version = self.store.next_version(id);
                    self.store.set_root(id, files.content, version);
                }
                id
            }
            Some(_) => {
                return vec![Effect::Notify(Notice::error(
                    "That path is already open as a different kind of asset",
                ))];
            }
            None => self.store.insert(NewAsset {
                root: files.content,
                path: Some(path.to_path_buf()),
                content_hash: files.content_hash,
                warnings: files.warnings,
            }),
        };
        if let Some(snapshot) = recovered {
            self.store.forget_disk(asset);
            effects.push(Effect::DeleteSnapshot(snapshot));
        }
        match kind {
            AssetKind::World => {
                self.files.active_world = Some(asset);
                self.files.world_patterns.insert(asset, pattern_maps);
                if let Some(value) = self.store.get(asset) {
                    self.files
                        .world_pattern_results
                        .insert(asset, value.version());
                    self.files
                        .world_pattern_requests
                        .insert(asset, value.version());
                }
                let first = self
                    .store
                    .world(asset)
                    .and_then(|world| world.maps.first())
                    .map(|map| map.file.path.clone())
                    .or_else(|| {
                        self.files
                            .world_patterns
                            .get(&asset)?
                            .first()
                            .map(|map| map.file.path.clone())
                    });
                let shows_member = self
                    .current()
                    .and_then(|doc| self.path_of(doc))
                    .is_some_and(|path| {
                        self.world_maps(asset)
                            .iter()
                            .any(|map| map.file.path == path)
                    });
                if !shows_member && let Some(first) = first {
                    effects.extend(self.open(&first));
                }
                self.session_changed();
            }
            AssetKind::Template => {
                self.files.loaders.insert(asset, files.loader);
                let doc = match self.document_of_path(path) {
                    Some(doc) => {
                        self.documents.set_current(doc);
                        doc
                    }
                    None => self.open_document(asset),
                };
                self.tools.tool = crate::ToolKind::SelectObjects;
                self.sync_template_caches();
                if let Some(document) = self.documents.get_mut(doc) {
                    document.selection.objects = vec![bt_core::ObjectId(1)];
                    document.inspect_selected_objects();
                }
            }
            AssetKind::Project => {
                self.files.project = Some(asset);
            }
            _ => {
                return vec![Effect::Notify(Notice::error(
                    "The auxiliary file has an unsupported asset kind",
                ))];
            }
        }
        self.remember_recent(path);
        effects
    }

    /// Puts back a file's remembered camera and current layer.
    fn restore_view_state(&mut self, doc: DocumentId, path: &Path) {
        let Some(state) = self.session.view_state(path).cloned() else {
            return;
        };
        let Some(map) = self.map_of(doc).cloned() else {
            return;
        };
        let Some(document) = self.documents.get_mut(doc) else {
            return;
        };
        document.camera = Some(ViewState {
            center: glam::DVec2::new(state.center_x, state.center_y),
            zoom: state.zoom,
        });
        if let Some(layer) = state.current_layer.map(bt_core::LayerId)
            && map.layer(layer).is_some()
        {
            document.selection = Selection::of_layer(layer);
        }
    }

    /// The tileset assets no map uses any more, no tab shows and that hold no unsaved changes.
    pub(crate) fn drop_unused_tilesets(&mut self) {
        let mut used: std::collections::HashSet<_> = self
            .store
            .assets()
            .flat_map(|asset| self.store.dependencies_of(asset.id()))
            .collect();
        used.extend(self.documents.in_order().map(|d| d.asset()));
        let unused: Vec<_> = self
            .store
            .assets()
            .filter(|asset| {
                asset.kind() == AssetKind::Tileset
                    && !asset.is_dirty()
                    && !used.contains(&asset.id())
            })
            .map(|asset| asset.id())
            .collect();
        for id in unused {
            self.store.remove(id);
        }
    }
}

/// A tileset file a load read, and the snapshot it came from when it was recovered.
struct ReadTileset {
    tileset: Arc<bt_core::Tileset>,
    content_hash: Option<u64>,
    warnings: Vec<bt_tmx::Warning>,
    recovered_from: Option<std::path::PathBuf>,
}
