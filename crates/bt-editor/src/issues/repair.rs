//! Locate File repairs a model reference as one undoable transaction. Both picker and worker
//! completions check the captured owner version, including undo and project switches.
use super::LocateTarget;
use crate::files::{LoadKind, LoadPurpose, LoadSource, Loaded};
use crate::{AssetId, DocumentId, EditError, Editor, Effect, FileRequest, NewAsset, Notice, Root};
use bt_core::{
    FileRef, ImageRef, ImageSource, MapObject, ObjectTemplate, TilesetImage, TilesetRef,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn error(message: impl Into<String>) -> Vec<Effect> {
    vec![Effect::Notify(Notice::error(message))]
}
impl Editor {
    fn repair_current(&self, doc: DocumentId, target: LocateTarget, version: u64) -> bool {
        self.document(doc).is_some()
            && self
                .store
                .get(target.asset())
                .is_some_and(|asset| asset.version() == version)
    }
    pub(crate) fn locate_issue(&self, doc: DocumentId, target: LocateTarget) -> Vec<Effect> {
        let Some(asset) = self
            .store
            .get(target.asset())
            .filter(|_| self.document(doc).is_some())
        else {
            return Vec::new();
        };
        vec![Effect::PickFile(FileRequest::LocateFile {
            doc,
            target,
            version: asset.version(),
        })]
    }
    pub(crate) fn locate_file_picked(
        &mut self,
        doc: DocumentId,
        target: LocateTarget,
        version: u64,
        path: &Path,
    ) -> Vec<Effect> {
        if !self.repair_current(doc, target, version) {
            return error(
                "The reference changed while Locate File was open; refresh Issues and try again",
            );
        }
        let kind = match target {
            LocateTarget::Tileset { .. } => LoadKind::Tileset,
            LocateTarget::Template { .. } => LoadKind::Template,
            LocateTarget::WorldMap { .. } => LoadKind::Map,
            LocateTarget::TilesetImage { .. } | LocateTarget::LayerImage { .. } => {
                return vec![Effect::ReadIssueImage {
                    request: FileRequest::LocateFile {
                        doc,
                        target,
                        version,
                    },
                    path: path.to_path_buf(),
                }];
            }
        };
        let mut job = self.load_job(path.to_path_buf(), LoadSource::File);
        job.kind = kind;
        job.purpose = LoadPurpose::Locate {
            doc,
            target,
            version,
        };
        vec![Effect::Load(job)]
    }
    fn reference_to(&self, owner: AssetId, path: &Path) -> FileRef {
        let source = self
            .store
            .get(owner)
            .and_then(|asset| asset.path())
            .and_then(Path::parent)
            .map(|folder| bt_utils::paths::reference_path(folder, path))
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        FileRef::new(source, path)
    }
    fn repaired_file(&mut self, old: Option<PathBuf>, path: &Path) -> Vec<Effect> {
        if let Some(old) = &old {
            self.store.clear_file_warnings(old);
        }
        self.store.clear_file_warnings(path);
        let mut effects = vec![Effect::ReloadImages(
            old.into_iter().chain([path.to_path_buf()]).collect(),
        )];
        effects.extend(self.refresh_issues());
        effects
    }
    pub(crate) fn issue_image_read(
        &mut self,
        request: FileRequest,
        result: Result<ImageRef, String>,
    ) -> Vec<Effect> {
        let FileRequest::LocateFile {
            doc,
            target,
            version,
        } = request
        else {
            return Vec::new();
        };
        if !self.repair_current(doc, target, version) {
            return error(
                "The reference changed while its replacement image was read; refresh Issues and try again",
            );
        }
        let mut image = match result {
            Ok(image) => image,
            Err(message) => return error(message),
        };
        let Some(path) = image.path().map(Path::to_path_buf) else {
            return error("Locate File requires a file image");
        };
        image.source = ImageSource::File(self.reference_to(target.asset(), &path));
        let mut old = None;
        let result = self.transact(doc, "Locate Image", |tx| {
            match target {
                LocateTarget::LayerImage { map, layer } => {
                    let layer = tx
                        .map_mut(map)?
                        .layer_mut(layer)
                        .ok_or(EditError::NoSuchLayer(layer))?;
                    let bt_core::LayerKind::Image(layer) = &mut layer.kind else {
                        return Err(EditError::NoSuchLayer(layer.id));
                    };
                    if let Some(previous) = &layer.image {
                        old = previous.path().map(Path::to_path_buf);
                        image.transparent = previous.transparent;
                    }
                    layer.image = Some(image);
                }
                LocateTarget::TilesetImage { home, tile } => {
                    let tileset = tx.tileset_at_mut(home)?;
                    if let Some(tile) = tile {
                        let data = tileset
                            .tiles
                            .get_mut(&tile)
                            .ok_or(EditError::NoSuchTile(tile))?;
                        if let Some(previous) = &data.image {
                            old = previous.path().map(Path::to_path_buf);
                            image.transparent = previous.transparent;
                        }
                        data.image = Some(image);
                        tileset.tile_width = tileset
                            .tiles
                            .values()
                            .filter_map(|tile| tile.image.as_ref())
                            .map(|image| image.declared_size.x)
                            .max()
                            .unwrap_or(0);
                        tileset.tile_height = tileset
                            .tiles
                            .values()
                            .filter_map(|tile| tile.image.as_ref())
                            .map(|image| image.declared_size.y)
                            .max()
                            .unwrap_or(0);
                    } else {
                        let columns = tileset.columns_for_width(image.declared_size.x);
                        let rows = tileset.rows_for_height(image.declared_size.y);
                        let TilesetImage::Atlas {
                            image: previous, ..
                        } = &tileset.image
                        else {
                            return Err(EditError::NoSuchAsset(home.asset()));
                        };
                        old = previous.path().map(Path::to_path_buf);
                        image.transparent = previous.transparent;
                        tileset.image = TilesetImage::Atlas {
                            image,
                            columns,
                            tile_count: columns.saturating_mul(rows),
                        };
                    }
                }
                _ => return Err(EditError::NoSuchAsset(target.asset())),
            }
            Ok::<_, EditError>(())
        });
        if let Err(problem) = result {
            return error(problem.to_string());
        }
        self.repaired_file(old, &path)
    }
    pub(crate) fn finish_locating_file(
        &mut self,
        doc: DocumentId,
        target: LocateTarget,
        version: u64,
        loaded: Loaded,
    ) -> Vec<Effect> {
        if !self.repair_current(doc, target, version) {
            return error(
                "The reference changed while its replacement was read; refresh Issues and try again",
            );
        }
        let files = match loaded.outcome {
            Ok(files) => files,
            Err(message) => return error(message),
        };
        let path = loaded.canonical;
        let reference = self.reference_to(target.asset(), &path);
        // An existing edited asset remains authoritative. Reading another reference must never
        // overwrite unsaved work in its editor.
        let existing = self.store.by_path(&path);
        let content = existing
            .and_then(|id| self.store.get(id))
            .map(|asset| asset.root().clone())
            .unwrap_or(files.content);
        self.adopt_dependencies(files.dependencies);
        let mut old = None;
        let result = match (target, &content) {
            (LocateTarget::Tileset { owner, slot }, Root::Tileset(tileset)) => {
                let tileset = tileset.clone();
                self.transact(doc, "Locate Tileset", |tx| {
                    let map = tx.map_mut(owner)?;
                    let previous = map
                        .tilesets
                        .get(slot.index())
                        .ok_or(bt_edit::EditError::NoSuchTileset(slot))?;
                    old = previous.file.as_ref().map(|file| file.path.clone());
                    let first_gid = previous.first_gid;
                    bt_edit::replace_tileset(
                        map,
                        slot,
                        TilesetRef {
                            tileset,
                            file: Some(reference),
                            first_gid,
                        },
                    )?;
                    Ok::<_, EditError>(())
                })
            }
            (LocateTarget::Template { map, object }, Root::Template(template)) => {
                let previous = self
                    .store
                    .editing_map(map)
                    .and_then(|map| map.object(object))
                    .and_then(|object| object.template.as_ref())
                    .map(|file| file.path.clone());
                let before = previous
                    .as_deref()
                    .and_then(|path| self.store.by_path(path))
                    .and_then(|id| self.store.template(id))
                    .cloned()
                    .unwrap_or_else(|| {
                        Arc::new(ObjectTemplate {
                            object: Some(MapObject::default()),
                            ..ObjectTemplate::default()
                        })
                    });
                self.transact(doc, "Locate Template", |tx| {
                    let map = tx.map_mut(map)?;
                    let base = bt_edit::instantiate_template(map, template, reference.clone())?;
                    let instance = bt_edit::object_mut(map, object)?;
                    old = instance.template.as_ref().map(|file| file.path.clone());
                    let before = before
                        .object
                        .as_ref()
                        .ok_or(bt_edit::EditError::TemplateBaseRequired)?;
                    bt_edit::refresh_template_object(instance, before, &base);
                    instance.template = Some(reference);
                    Ok::<_, EditError>(())
                })
            }
            (LocateTarget::WorldMap { world, index }, Root::Map(map)) => {
                let bounds = map.bounding_rect();
                self.transact(doc, "Locate World Map", |tx| {
                    let map = tx
                        .world_mut(world)?
                        .maps
                        .get_mut(index)
                        .ok_or(EditError::NoSuchAsset(world))?;
                    old = Some(map.file.path.clone());
                    map.file = reference;
                    map.rect.w = bounds.w;
                    map.rect.h = bounds.h;
                    Ok::<_, EditError>(())
                })
            }
            _ => return error("The replacement has the wrong asset kind"),
        };
        if let Err(problem) = result {
            return error(problem.to_string());
        }
        let asset = existing.unwrap_or_else(|| {
            self.store.insert(NewAsset {
                root: content,
                path: Some(path.clone()),
                content_hash: files.content_hash,
                warnings: files.warnings,
            })
        });
        self.files.loaders.insert(asset, files.loader);
        if matches!(target, LocateTarget::Template { .. }) {
            let types = Arc::new(self.custom_types().clone());
            self.files
                .loaders
                .entry(target.asset())
                .or_insert_with(|| bt_tmx::Loader::builder().custom_types(types).build());
            self.sync_template_caches();
        }
        self.repaired_file(old, &path)
    }
}
