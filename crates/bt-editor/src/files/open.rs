//! Opening maps: the load handed out, then the asset, its tilesets and project, and a document.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::document::{DocumentId, Selection, ViewState};
use crate::editor::Editor;
use crate::effect::Effect;
use crate::files::{
    LoadJob, LoadKind, LoadPurpose, LoadSource, Loaded, LoadedContent, LoadedFiles, ProjectChoice,
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

    /// A load of `path`, with the open project's types or a search for the nearest project.
    pub(crate) fn load_job(&self, path: PathBuf, source: LoadSource) -> LoadJob {
        let project = match self.files.project {
            Some(_) => ProjectChoice::Known(Arc::clone(&self.files.custom_types)),
            None => ProjectChoice::FindNearest {
                home: self.files.home.clone(),
            },
        };
        LoadJob {
            path,
            source,
            project,
            kind: LoadKind::Map,
            purpose: LoadPurpose::Open,
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
    pub(crate) fn finish_load(&mut self, loaded: Loaded) -> Vec<Effect> {
        let mut effects = Vec::new();
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
        let LoadedContent::Map(map) = files.content else {
            return vec![Effect::Notify(Notice::error(format!(
                "{} is a tileset; tilesets open with the tileset editor",
                bt_utils::paths::display_name(&path)
            )))];
        };
        if loaded.recovered_from.is_none()
            && let Some(doc) = self.document_of_path(&path)
        {
            self.documents.set_current(doc);
            return effects;
        }
        if let Some(project) = files.project
            && self.files.project.is_none()
        {
            self.files.custom_types = Arc::new(project.project.custom_types.clone());
            let id = self.store.insert(NewAsset {
                root: Root::Project(Arc::new(project.project)),
                path: Some(project.path),
                content_hash: Some(project.content_hash),
                warnings: Vec::new(),
            });
            self.files.project = Some(id);
        }
        let mut map = *map;
        self.adopt_tilesets(files.tilesets);
        self.store.refresh_tilesets(&mut map);
        let warning_count = files.warnings.len();
        // A recovered map replaces nothing: if its file is open, it opens beside it, unsaved.
        let file = if loaded.recovered_from.is_some() && self.store.by_path(&path).is_some() {
            None
        } else {
            Some(path.clone())
        };
        let asset = self.store.insert(NewAsset {
            root: Root::Map(Arc::new(map)),
            path: file,
            content_hash: files.content_hash,
            warnings: files.warnings,
        });
        if loaded.recovered_from.is_some() {
            self.store.forget_disk(asset);
        }
        self.files.loaders.insert(asset, files.loader);
        let doc = self.open_document(asset);
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
        self.session_changed();
        effects
    }

    /// Puts the external tilesets a load read into the store, unless it has their files.
    fn adopt_tilesets(&mut self, tilesets: Vec<crate::files::load::LoadedTileset>) {
        for tileset in tilesets {
            if self.store.by_path(&tileset.path).is_none() {
                self.store.insert(NewAsset {
                    root: Root::Tileset(tileset.tileset),
                    path: Some(tileset.path),
                    content_hash: Some(tileset.content_hash),
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
        match files.content {
            LoadedContent::Map(map) => {
                let mut map = *map;
                self.adopt_tilesets(files.tilesets);
                self.store.refresh_tilesets(&mut map);
                let Some(doc) = self
                    .documents
                    .in_order()
                    .find(|d| d.asset() == asset)
                    .map(|d| d.id())
                else {
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
            }
            LoadedContent::Tileset(tileset) => {
                let version = self.store.next_version(asset);
                self.store.set_root(asset, Root::Tileset(tileset), version);
            }
        }
        self.store.mark_saved(asset, content_hash);
        vec![Effect::Notify(Notice::info(format!(
            "{name} changed on disk and was reloaded"
        )))]
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

    /// The tileset assets no map uses any more and that hold no unsaved changes.
    pub(crate) fn drop_unused_tilesets(&mut self) {
        let used: std::collections::HashSet<_> = self
            .store
            .assets()
            .filter_map(|asset| match asset.root() {
                Root::Map(map) => Some(self.store.tilesets_of(map)),
                _ => None,
            })
            .flatten()
            .collect();
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
