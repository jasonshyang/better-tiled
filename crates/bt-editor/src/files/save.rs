//! Saving: a map's unsaved external tilesets first (a tileset that cannot be written keeps its
//! changes and the map is saved anyway), then the map, then the open project when its custom
//! types changed; Save As moves the map's file properties
//! to the new folder as a transaction first. Writes are atomic unless safe writing is off, and
//! the bytes written are hashed so the watcher can tell our writes from others'.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_tmx::WriteRequest;
use bt_utils::atomic_write::{WriteMode, write_file};
use bt_utils::hash::fnv1a_64;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::{Effect, FileRequest};
use crate::error::EditError;
use crate::notice::Notice;
use crate::store::{AssetId, Root};

impl Editor {
    /// Saves a document (its asset's file, or a file to pick when it has none) and the open
    /// project's file when its custom types changed.
    pub(crate) fn save(&mut self, doc: DocumentId) -> Vec<Effect> {
        let mut effects = self.save_document(doc);
        effects.extend(self.save_project());
        let mut worlds = self
            .path_of(doc)
            .map(|path| self.worlds_for_path(&path))
            .unwrap_or_default();
        if let Some(document) = self.document(doc) {
            for change in document
                .history
                .entries
                .iter()
                .take(document.history.position())
                .flat_map(|entry| entry.changes())
            {
                if self.store.world(change.asset).is_some() && !worlds.contains(&change.asset) {
                    worlds.push(change.asset);
                }
            }
        }
        for world in worlds {
            if self.store.get(world).is_some_and(|asset| asset.is_dirty()) {
                match self.write_auxiliary(world) {
                    Ok(()) => {
                        effects.push(Effect::DeleteSnapshot(self.recovery.snapshot_path(world)))
                    }
                    Err(notice) => effects.push(Effect::Notify(notice)),
                }
            }
        }
        effects
    }

    /// Writes the open project's file when it has one and its custom types changed.
    fn save_project(&mut self) -> Vec<Effect> {
        let Some(project) = self.project_file() else {
            return Vec::new();
        };
        if !self
            .store
            .get(project)
            .is_some_and(|asset| asset.is_dirty())
        {
            return Vec::new();
        }
        match self.write_project(project) {
            Ok(()) => vec![Effect::DeleteSnapshot(self.recovery.snapshot_path(project))],
            Err(notice) => vec![Effect::Notify(notice)],
        }
    }

    /// Saves a document: its asset's file, or a file to pick when it has none.
    fn save_document(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let Some(path) = self
            .store
            .get(asset)
            .and_then(|a| a.path().map(Path::to_path_buf))
        else {
            return vec![self.save_as_request(doc)];
        };
        if self.store.get(asset).is_some_and(|asset| {
            matches!(
                asset.kind(),
                crate::AssetKind::Template | crate::AssetKind::World | crate::AssetKind::Project
            )
        }) {
            let mut effects = self.save_dependencies(asset);
            effects.extend(match self.write_auxiliary(asset) {
                Ok(()) => vec![
                    Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)),
                    Effect::Notify(Notice::info(format!(
                        "Saved {}",
                        bt_utils::paths::display_name(&path)
                    ))),
                ],
                Err(notice) => vec![Effect::Notify(notice)],
            });
            return effects;
        }
        if self.store.tileset(asset).is_some() {
            return match self.write_tileset(asset) {
                Ok(()) => vec![
                    Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)),
                    Effect::Notify(Notice::info(format!(
                        "Saved {}",
                        bt_utils::paths::display_name(&path)
                    ))),
                ],
                Err(notice) => vec![Effect::Notify(notice)],
            };
        }
        self.save_map_to(asset, &path)
    }

    /// The file request of Save As for a document.
    pub(crate) fn save_as_request(&self, doc: DocumentId) -> Effect {
        if let Some(project) = self
            .document(doc)
            .map(|doc| doc.asset())
            .filter(|id| self.store.project(*id).is_some())
        {
            return Effect::PickFile(FileRequest::ProjectAs(project));
        }
        let suggested = self
            .path_of(doc)
            .unwrap_or_else(|| PathBuf::from("untitled.tmx"));
        Effect::PickFile(FileRequest::SaveAs { doc, suggested })
    }

    /// Saves a document to a new file: its file properties are rebased to the new folder as a
    /// transaction, the asset moves there, and it is written.
    pub(crate) fn save_as(&mut self, doc: DocumentId, path: &Path) -> Vec<Effect> {
        if self.template_of(doc).is_some() {
            return vec![Effect::Notify(Notice::error(
                "Use Save As Template on an instance to make a new template",
            ))];
        }
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => {
                return vec![Effect::Notify(Notice::error(format!(
                    "Cannot save to {}: {error}",
                    path.display()
                )))];
            }
        };
        let old_folder = self
            .store
            .get(asset)
            .and_then(|a| a.path())
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        if let Some(from) = old_folder
            && let Some(to) = path.parent()
            && from != to
        {
            let rebased = self.transact(doc, "Save As", |tx| {
                bt_tmx::rebase_file_references(tx.map_mut(asset)?, &from, to);
                Ok::<_, EditError>(())
            });
            if let Err(error) = rebased {
                return vec![Effect::Notify(Notice::error(format!(
                    "Cannot save to {}: {error}",
                    path.display()
                )))];
            }
        }
        let mut effects = self.move_layers_record(asset, &path);
        self.store.retarget(asset, path.clone());
        effects.extend(self.save_map_to(asset, &path));
        self.remember_recent(&path);
        self.session_changed();
        effects
    }

    /// Writes a map's unsaved external tilesets, then the map.
    fn save_map_to(&mut self, asset: AssetId, path: &Path) -> Vec<Effect> {
        let mut effects = Vec::new();
        effects.extend(self.save_dependencies(asset));
        match self.write_map(asset, path) {
            Ok(warnings) => {
                self.recovery.forget(asset);
                if let Some(document) = self.documents.in_order().find(|d| d.asset() == asset) {
                    effects.push(Effect::Godot {
                        doc: document.id(),
                        operation: crate::GodotOperation::AfterSave,
                    });
                }
                effects.push(Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)));
                effects.extend(warnings.into_iter().map(Effect::Notify));
                effects.push(Effect::Notify(Notice::info(format!(
                    "Saved {}",
                    bt_utils::paths::display_name(path)
                ))));
            }
            Err(notice) => effects.push(Effect::Notify(notice)),
        }
        effects
    }

    /// Saves referenced definitions before their owner, reporting failures individually.
    pub(super) fn save_dependencies(&mut self, asset: AssetId) -> Vec<Effect> {
        let mut effects = Vec::new();
        for dependency in self.store.dependencies_of(asset) {
            let Some(source) = self.store.get(dependency).filter(|asset| asset.is_dirty()) else {
                continue;
            };
            let result = match source.kind() {
                crate::AssetKind::Tileset => self.write_tileset(dependency),
                crate::AssetKind::Template => self.write_auxiliary(dependency),
                _ => continue,
            };
            match result {
                Ok(()) => effects.push(Effect::DeleteSnapshot(
                    self.recovery.snapshot_path(dependency),
                )),
                Err(notice) => effects.push(Effect::Notify(notice)),
            }
        }
        effects
    }

    /// Writes every unsaved asset that has a file; the notices of those that failed.
    pub(crate) fn save_all(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut unsaved = self.store.unsaved();
        unsaved.sort_by_key(|id| match self.store.get(*id).map(|asset| asset.kind()) {
            Some(crate::AssetKind::Tileset) => 0,
            Some(crate::AssetKind::Template) => 1,
            Some(crate::AssetKind::Map) => 2,
            _ => 3,
        });
        for id in unsaved {
            let Some(asset) = self.store.get(id) else {
                continue;
            };
            let Some(path) = asset.path().map(Path::to_path_buf) else {
                continue;
            };
            let written = match asset.root() {
                Root::Map(_) => self.write_map(id, &path),
                Root::Tileset(_) => self.write_tileset(id).map(|()| Vec::new()),
                Root::Project(_) => self.write_project(id).map(|()| Vec::new()),
                Root::World(_) | Root::Template(_) => self.write_auxiliary(id).map(|()| Vec::new()),
            };
            match written {
                Ok(warnings) => {
                    self.recovery.forget(id);
                    if self.store.map(id).is_some()
                        && let Some(document) = self.documents.in_order().find(|d| d.asset() == id)
                    {
                        effects.push(Effect::Godot {
                            doc: document.id(),
                            operation: crate::GodotOperation::AfterSave,
                        });
                    }
                    effects.push(Effect::DeleteSnapshot(self.recovery.snapshot_path(id)));
                    effects.extend(warnings.into_iter().map(Effect::Notify));
                }
                Err(notice) => effects.push(Effect::Notify(notice)),
            }
        }
        effects
    }

    /// Writes a map asset to `path` and marks it saved: the rules' result while its AutoMap
    /// layers are on, their record going to its project file (with them off, a record left from
    /// before goes). The warnings of what could not be done along the way.
    fn write_map(&mut self, asset: AssetId, path: &Path) -> Result<Vec<Notice>, Notice> {
        let save = self.layers_to_save(asset);
        let map = match &save {
            Some(save) => Arc::clone(save.layers.result()),
            None => self
                .store
                .map(asset)
                .cloned()
                .ok_or_else(|| Notice::error(format!("{asset} is not a map")))?,
        };
        let text = {
            let mut request = WriteRequest::for_map(&map, path)
                .compatibility_version(self.compatibility_version());
            if let Some(loader) = self.files.loaders.get(&asset) {
                request = request.loader(loader);
            }
            request
                .render()
                .map_err(|error| cannot_save(path, &error.to_string()))?
                .text
        };
        self.write_bytes(path, text.as_bytes())?;
        self.store.mark_saved(asset, fnv1a_64(text.as_bytes()));
        let name = bt_utils::paths::display_name(path);
        let mut warnings = Vec::new();
        if save.as_ref().is_some_and(|save| !save.with_result) {
            warnings.push(Notice::warning(format!(
                "{name} was saved as painted: its AutoMap rules have not run on it as it is"
            )));
        }
        if let Err(problem) = self.write_layers_record(asset, path, save.as_ref()) {
            warnings.push(Notice::warning(format!(
                "What was painted under the AutoMap layers of {name} was not saved: {problem}"
            )));
        }
        Ok(warnings)
    }

    /// Writes a tileset asset to its file and marks it saved; its snapshot is no longer kept.
    fn write_tileset(&mut self, asset: AssetId) -> Result<(), Notice> {
        let tileset = self
            .store
            .tileset(asset)
            .cloned()
            .ok_or_else(|| Notice::error(format!("{asset} is not a tileset")))?;
        let path = self
            .store
            .get(asset)
            .and_then(|a| a.path().map(Path::to_path_buf))
            .ok_or_else(|| Notice::error(format!("{asset} has no file")))?;
        let text = WriteRequest::for_tileset(&tileset, &path)
            .compatibility_version(self.compatibility_version())
            .render()
            .map_err(|error| cannot_save(&path, &error.to_string()))?
            .text;
        self.write_bytes(&path, text.as_bytes())?;
        self.store.mark_saved(asset, fnv1a_64(text.as_bytes()));
        self.recovery.forget(asset);
        Ok(())
    }

    /// Writes a project asset to its file and marks it saved.
    fn write_project(&mut self, asset: AssetId) -> Result<(), Notice> {
        let project = self
            .store
            .project(asset)
            .cloned()
            .ok_or_else(|| Notice::error(format!("{asset} is not a project")))?;
        let path = self
            .store
            .get(asset)
            .and_then(|a| a.path().map(Path::to_path_buf))
            .ok_or_else(|| Notice::error(format!("{asset} has no file")))?;
        let text = WriteRequest::for_project(&project, &path)
            .render()
            .map_err(|error| cannot_save(&path, &error.to_string()))?
            .text;
        self.write_bytes(&path, text.as_bytes())?;
        self.store.mark_saved(asset, fnv1a_64(text.as_bytes()));
        self.recovery.forget(asset);
        Ok(())
    }

    pub(super) fn write_auxiliary(&mut self, asset: AssetId) -> Result<(), Notice> {
        let source = self
            .store
            .get(asset)
            .ok_or_else(|| Notice::error(format!("{asset} is no longer open")))?;
        let path = source
            .path()
            .ok_or_else(|| Notice::error(format!("{asset} has no file")))?
            .to_path_buf();
        let text = source
            .root()
            .write_request(&path)
            .compatibility_version(self.compatibility_version())
            .render()
            .map_err(|error| cannot_save(&path, &error.to_string()))?
            .text;
        self.write_bytes(&path, text.as_bytes())?;
        self.store.mark_saved(asset, fnv1a_64(text.as_bytes()));
        self.recovery.forget(asset);
        Ok(())
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), Notice> {
        let mode = if self.settings.general.safe_writing {
            WriteMode::Atomic
        } else {
            WriteMode::InPlace
        };
        write_file(path, bytes, mode).map_err(|error| match error.refused() {
            Some(reason) => cannot_save(
                path,
                &format!("{reason}; make it writable, or Save As elsewhere"),
            ),
            None => cannot_save(path, &error.to_string()),
        })
    }
}

fn cannot_save(path: &Path, why: &str) -> Notice {
    Notice::error(format!(
        "Cannot save {}",
        bt_utils::paths::display_name(path)
    ))
    .with_detail(why.to_owned())
}
