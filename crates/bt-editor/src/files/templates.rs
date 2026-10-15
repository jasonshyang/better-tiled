//! Template creation, placement and instance commands. A chosen template is a typed asset;
//! insertion adopts its tileset and the object in one transaction, as the preview showed.

use super::{LoadKind, LoadPurpose, LoadSource, Loaded};
use crate::objects::{ObjectHost, change_tile_shapes};
use crate::{
    AssetId, DocumentId, EditError, Editor, Effect, NewAsset, Notice, Root, SnapToggle, ToolKind,
};
use bt_core::{FileRef, LayerId, Map, MapObject, ObjectId, ObjectOverrides, ObjectTemplate};
use bt_edit::{add_object, instantiate_template};
use glam::DVec2;
use std::{path::Path, sync::Arc};

struct Placement {
    host: ObjectHost,
    layer: LayerId,
    map: Map,
    object: MapObject,
}

impl Editor {
    /// The object editing view, including a tile's collision shapes.
    pub(crate) fn template_editing_map(&self, doc: DocumentId) -> Option<Arc<Map>> {
        self.collision_view(doc)
            .map(Arc::new)
            .or_else(|| self.map_of(doc).cloned())
    }

    fn template_owner(&self, doc: DocumentId) -> Option<AssetId> {
        match self.object_host(doc)? {
            ObjectHost::Map(asset) => Some(asset),
            ObjectHost::Tile { home, .. } => Some(home.asset()),
        }
    }

    fn ensure_template_writer(&mut self, doc: DocumentId) {
        if let Some(asset) = self.template_owner(doc) {
            let types = Arc::new(self.custom_types().clone());
            self.files
                .loaders
                .entry(asset)
                .or_insert_with(|| bt_tmx::Loader::builder().custom_types(types).build());
        }
    }

    /// The template chosen for placement or replacement.
    pub fn chosen_template(&self) -> Option<AssetId> {
        self.tools
            .template
            .filter(|id| self.store.template(*id).is_some())
    }

    pub(crate) fn choose_template_file(&self, doc: DocumentId, path: &Path) -> Vec<Effect> {
        let mut job = self.load_job(path.to_path_buf(), LoadSource::File);
        job.kind = LoadKind::Template;
        job.purpose = LoadPurpose::ChooseTemplate(doc);
        vec![Effect::Load(job)]
    }

    pub(super) fn finish_choosing_template(
        &mut self,
        doc: DocumentId,
        loaded: Loaded,
    ) -> Vec<Effect> {
        if self.document(doc).is_none() {
            return Vec::new();
        }
        let files = match loaded.outcome {
            Ok(files) => files,
            Err(problem) => return vec![Effect::Notify(Notice::error(problem))],
        };
        let Root::Template(template) = files.content else {
            return Vec::new();
        };
        if matches!(self.object_host(doc), Some(ObjectHost::Tile { .. }))
            && template
                .object
                .as_ref()
                .is_some_and(|object| object.tile.is_some())
        {
            return vec![Effect::Notify(Notice::error(
                "Tile-object templates cannot be used in collision groups",
            ))];
        }
        self.adopt_dependencies(files.dependencies);
        let asset = match self.store.by_path(&loaded.canonical) {
            Some(asset) if self.store.template(asset).is_some() => asset,
            Some(_) => {
                return vec![Effect::Notify(Notice::error(
                    "The selected file is already loaded as a different asset kind",
                ))];
            }
            None => self.store.insert(NewAsset {
                root: Root::Template(template),
                path: Some(loaded.canonical),
                content_hash: files.content_hash,
                warnings: files.warnings,
            }),
        };
        self.files.loaders.insert(asset, files.loader);
        self.ensure_template_writer(doc);
        self.sync_template_caches();
        self.tools.template = Some(asset);
        self.tools.set_tool(ToolKind::InsertTemplate);
        self.documents.set_current(doc);
        Vec::new()
    }

    fn prepare_template(
        &self,
        doc: DocumentId,
        at: DVec2,
        snap: SnapToggle,
    ) -> Result<Placement, String> {
        let host = self
            .object_host(doc)
            .ok_or("Choose an object layer or collision tile first")?;
        let mut map = self
            .template_editing_map(doc)
            .ok_or("The object editor is no longer open")?
            .as_ref()
            .clone();
        let layer = match host {
            ObjectHost::Tile { .. } => LayerId(2),
            ObjectHost::Map(_) => self
                .document(doc)
                .and_then(|doc| doc.selection().current_layer)
                .ok_or("Choose an object layer first")?,
        };
        if map
            .layer(layer)
            .and_then(|layer| layer.as_object_layer())
            .is_none()
        {
            return Err("Templates go on object layers".into());
        }
        let state = map
            .effective_layer(layer)
            .ok_or("The object layer no longer exists")?;
        if state.locked {
            return Err("The object layer is locked".into());
        }
        let template = self.chosen_template().ok_or("Choose a template first")?;
        let path = self
            .store
            .get(template)
            .and_then(|asset| asset.path())
            .ok_or("The template has no filename")?;
        let definition = self
            .store
            .template(template)
            .ok_or("The template is no longer loaded")?;
        if matches!(host, ObjectHost::Tile { .. })
            && definition
                .object
                .as_ref()
                .is_some_and(|object| object.tile.is_some())
        {
            return Err("Tile-object templates cannot be placed in collision groups".into());
        }

        let source = self
            .template_owner(doc)
            .and_then(|owner| self.store.get(owner))
            .and_then(|asset| asset.path())
            .map(Path::to_path_buf)
            .and_then(|owner| {
                owner
                    .parent()
                    .map(|folder| bt_utils::paths::reference_path(folder, path))
            })
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let mut object = instantiate_template(&mut map, definition, FileRef::new(source, path))
            .map_err(|error| error.to_string())?;
        let bounds = bt_core::object_outline(
            &map.geometry(),
            &object,
            object.tile.and_then(|cell| map.tileset_of(cell)),
        )
        .bounds();
        let anchor = map.geometry().pixel_to_screen(object.pos) + at
            - state.offset
            - (bounds.min + bounds.max) * 0.5;
        object.pos = self.snapped(doc, map.geometry().screen_to_pixel(anchor), snap);
        Ok(Placement {
            host,
            layer,
            map,
            object,
        })
    }

    /// The complete map with a translucent template ghost; new tilesets are present for the
    /// renderer, while the stored map and its history remain unchanged.
    pub fn template_preview(
        &self,
        doc: DocumentId,
        at: DVec2,
        snap: SnapToggle,
    ) -> Option<Arc<Map>> {
        let mut placement = self.prepare_template(doc, at, snap).ok()?;
        placement.object.opacity *= 0.55;
        add_object(&mut placement.map, placement.layer, placement.object, None).ok()?;
        Some(Arc::new(placement.map))
    }

    pub(crate) fn place_template(
        &mut self,
        doc: DocumentId,
        at: DVec2,
        snap: SnapToggle,
    ) -> Vec<Effect> {
        let placement = match self.prepare_template(doc, at, snap) {
            Ok(placement) => placement,
            Err(problem) => return vec![Effect::Notify(Notice::error(problem))],
        };
        let mut placement = placement;
        let id = match add_object(&mut placement.map, placement.layer, placement.object, None) {
            Ok(id) => id,
            Err(problem) => return vec![Effect::Notify(Notice::error(problem.to_string()))],
        };
        self.transact(doc, "Insert Template", |tx| {
            match placement.host {
                ObjectHost::Map(asset) => *tx.map_mut(asset)? = placement.map,
                ObjectHost::Tile { home, tile } => {
                    let objects = placement
                        .map
                        .layer(placement.layer)
                        .and_then(|layer| layer.as_object_layer())
                        .ok_or(EditError::NoSuchLayer(placement.layer))?
                        .clone();
                    change_tile_shapes(tx, home, tile, |shapes, _| {
                        *shapes = objects;
                        Ok(())
                    })?;
                }
            }
            tx.selection.objects = vec![id];
            Ok::<_, EditError>(())
        })
        .err()
        .map(|error| Effect::Notify(Notice::error(error.to_string())))
        .into_iter()
        .collect()
    }

    pub(crate) fn save_object_template(
        &mut self,
        doc: DocumentId,
        object: ObjectId,
        path: &Path,
    ) -> Vec<Effect> {
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => return vec![Effect::Notify(Notice::error(error.to_string()))],
        };
        if self.store.by_path(&path).is_some() {
            return vec![Effect::Notify(Notice::error(
                "That template is already loaded; edit its base instead",
            ))];
        }
        let Some(host) = self.object_host(doc) else {
            return Vec::new();
        };
        let Some(owner) = self.template_owner(doc) else {
            return Vec::new();
        };
        let Some(map) = self.template_editing_map(doc) else {
            return Vec::new();
        };
        let Some(original) = map.object(object) else {
            return Vec::new();
        };
        if original.template.is_some() {
            return vec![Effect::Notify(Notice::error(
                "Detach the instance before saving a new template",
            ))];
        }
        if original
            .tile
            .and_then(|cell| cell.slot())
            .and_then(|slot| map.tilesets.get(slot.index()))
            .is_some_and(|tileset| tileset.file.is_none())
        {
            return vec![Effect::Notify(Notice::error(
                "Export the object's embedded tileset to an external tileset before creating a template",
            ))];
        }
        if map
            .object_layer_of(object)
            .and_then(|layer| map.effective_layer(layer))
            .is_some_and(|layer| layer.locked)
        {
            return vec![Effect::Notify(Notice::error(
                "The object's layer is locked",
            ))];
        }
        let mut template = ObjectTemplate::from_map_object(&map, original);
        if let Some(from) = self
            .store
            .get(owner)
            .and_then(|asset| asset.path())
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            && let Some(to) = path.parent()
            && let Some(base) = &mut template.object
        {
            bt_tmx::rebase_properties(&mut base.properties, &from, to);
        }
        let id = self.store.insert(NewAsset {
            root: Root::Template(Arc::new(template.clone())),
            path: Some(path.clone()),
            content_hash: None,
            warnings: Vec::new(),
        });
        let effects = self.save_dependencies(id);
        if effects.iter().any(|effect| matches!(effect, Effect::Notify(notice) if notice.severity == crate::Severity::Error)) {
            self.store.remove(id);
            return effects;
        }
        if let Err(notice) = self.write_auxiliary(id) {
            self.store.remove(id);
            return vec![Effect::Notify(notice)];
        }
        self.ensure_template_writer(doc);
        self.sync_template_caches();
        let source = self
            .store
            .get(owner)
            .and_then(|asset| asset.path())
            .and_then(Path::parent)
            .map(|folder| bt_utils::paths::reference_path(folder, &path))
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let link = |object: &mut MapObject| {
            object.template = Some(FileRef::new(source, &path));
            object.overrides = ObjectOverrides::empty();
            if let Some(base) = &template.object {
                object.properties = base.properties.clone();
            }
        };
        let result = self.transact(doc, "Save As Template", |tx| {
            match host {
                ObjectHost::Map(asset) => link(bt_edit::object_mut(tx.map_mut(asset)?, object)?),
                ObjectHost::Tile { home, tile } => {
                    change_tile_shapes(tx, home, tile, |shapes, _| {
                        link(bt_edit::object_mut_in(shapes, object)?);
                        Ok(())
                    })?
                }
            }
            Ok::<_, EditError>(())
        });
        self.tools.template = Some(id);
        let mut effects = effects;
        effects.extend(
            result
                .err()
                .map(|error| Effect::Notify(Notice::error(error.to_string()))),
        );
        effects
    }

    pub(crate) fn edit_instance_template(&mut self, doc: DocumentId) -> Vec<Effect> {
        let path = self.document(doc).and_then(|document| {
            document.selection().objects.iter().find_map(|id| {
                self.template_editing_map(doc)?
                    .object(*id)?
                    .template
                    .as_ref()
                    .map(|file| file.path.clone())
            })
        });
        path.map(|path| self.request_open(&path))
            .unwrap_or_default()
    }

    pub(crate) fn edit_template_instances(
        &mut self,
        doc: DocumentId,
        action: crate::ActionId,
    ) -> Vec<Effect> {
        let detach = action == crate::ActionId::DetachTemplate;
        let replace = action == crate::ActionId::ReplaceWithTemplate;
        let templates = self
            .store
            .assets()
            .filter_map(|asset| {
                Some((
                    asset.path()?.to_path_buf(),
                    Arc::clone(self.store.template(asset.id())?),
                ))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let chosen = self
            .chosen_template()
            .and_then(|id| self.store.get(id))
            .and_then(|asset| asset.path())
            .map(|path| path.to_path_buf());
        let folder = self
            .template_owner(doc)
            .and_then(|owner| self.store.get(owner))
            .and_then(|asset| asset.path())
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let apply = |map: &mut Map, ids: &[ObjectId]| -> Result<(), bt_edit::EditError> {
            for id in ids {
                let previous = map
                    .object(*id)
                    .cloned()
                    .ok_or(bt_edit::EditError::NoSuchObject(*id))?;
                let file = if replace {
                    chosen
                        .as_ref()
                        .map(|path| FileRef::new(path.to_string_lossy(), path))
                } else {
                    previous.template.clone()
                };
                let Some(file) = file else { continue };
                let template = templates.get(&file.path);
                let mut object = if detach {
                    previous.clone()
                } else {
                    let template = template.ok_or_else(|| {
                        bt_edit::EditError::TemplateUnavailable(file.path.clone())
                    })?;
                    instantiate_template(map, template, file.clone())?
                };
                if detach {
                    if let (Some(from), Some(to), Some(base)) = (
                        file.path.parent(),
                        folder.as_deref(),
                        template.and_then(|template| template.object.as_ref()),
                    ) {
                        let mut inherited = object
                            .properties
                            .iter()
                            .filter(|(name, value)| base.properties.get(*name) == Some(*value))
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect::<bt_core::Properties>();
                        bt_tmx::rebase_properties(&mut inherited, from, to);
                        object.properties.extend(inherited);
                    }
                    object.template = None;
                    object.overrides = ObjectOverrides::empty();
                }
                object.id = previous.id;
                object.pos = previous.pos;
                *bt_edit::object_mut(map, *id)? = object;
            }
            Ok(())
        };
        self.on_selected_objects(
            doc,
            if detach {
                "Detach Template Instances"
            } else if replace {
                "Replace With Template"
            } else {
                "Reset Template Instances"
            },
            apply,
            |objects, _, ids| {
                let mut map = Map {
                    layers: vec![bt_core::Layer::new(
                        LayerId(1),
                        "Collision",
                        bt_core::LayerKind::Object(objects.clone()),
                    )],
                    ..Map::default()
                };
                apply(&mut map, ids)?;
                if map
                    .layer(LayerId(1))
                    .and_then(|layer| layer.as_object_layer())
                    .is_some_and(|layer| layer.objects.iter().any(|object| object.tile.is_some()))
                {
                    return Err(bt_edit::EditError::TileTemplateInCollision);
                }
                *objects = map
                    .layer(LayerId(1))
                    .and_then(|layer| layer.as_object_layer())
                    .cloned()
                    .ok_or(bt_edit::EditError::NotAnObjectLayer(LayerId(1)))?;
                Ok(())
            },
        )
    }
}
