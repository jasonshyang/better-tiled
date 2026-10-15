//! Loaded worlds are assets; their explicit map positions remain part of map history.

use crate::{AssetId, DocumentId, Editor};
use bt_core::WorldMap;
use std::path::Path;

impl Editor {
    /// The first loaded world containing a document's map.
    pub fn world_of(&self, doc: DocumentId) -> Option<AssetId> {
        let path = self.path_of(doc)?;
        let worlds = self.worlds_for_path(&path);
        self.files
            .active_world
            .filter(|world| worlds.contains(world))
            .or_else(|| worlds.into_iter().next())
    }

    pub(crate) fn worlds_for_path(&self, path: &Path) -> Vec<AssetId> {
        self.store
            .assets()
            .filter(|asset| self.store.world(asset.id()).is_some())
            .filter(|asset| {
                self.world_maps(asset.id())
                    .iter()
                    .any(|map| map.file.path == path)
            })
            .map(|asset| asset.id())
            .collect()
    }

    /// Explicit positions plus maps found by filename patterns, one entry per resolved path.
    pub fn world_maps(&self, world: AssetId) -> Vec<WorldMap> {
        let mut maps = self
            .store
            .world(world)
            .map(|world| world.maps.clone())
            .unwrap_or_default();
        if self.store.get(world).is_some_and(|asset| {
            self.files.world_pattern_results.get(&world) == Some(&asset.version())
        }) && let Some(patterns) = self.files.world_patterns.get(&world)
        {
            for map in patterns {
                if !maps.iter().any(|known| known.file.path == map.file.path) {
                    maps.push(map.clone());
                }
            }
        }
        maps
    }
}

impl Editor {
    /// Offset from this map's drawing coordinates to its containing world's coordinates.
    pub fn world_origin(&self, doc: DocumentId) -> Option<glam::DVec2> {
        let world = self.world_of(doc)?;
        let path = self.path_of(doc)?;
        let entry = self
            .world_maps(world)
            .into_iter()
            .find(|map| map.file.path == path)?;
        Some((entry.rect.min() - self.map_of(doc)?.bounding_rect().min()).as_dvec2())
    }

    /// Maps visible in the viewport and permitted by the world's adjacency switch.
    pub fn world_maps_in_view(&self, doc: DocumentId, visible: bt_utils::DRect) -> Vec<WorldMap> {
        let Some(world) = self.world_of(doc) else {
            return Vec::new();
        };
        let Some(origin) = self.world_origin(doc) else {
            return Vec::new();
        };
        let maps = self.world_maps(world);
        let adjacent = self
            .store
            .world(world)
            .filter(|world| world.only_show_adjacent_maps)
            .and_then(|_| self.path_of(doc))
            .and_then(|path| maps.iter().find(|map| map.file.path == path))
            .map(|entry| {
                bt_utils::DRect::from_min_size(
                    entry.rect.min().as_dvec2(),
                    entry.rect.size().as_dvec2(),
                )
                .expanded(glam::DVec2::ONE)
            });
        let visible = visible.translated(origin);
        maps.into_iter()
            .filter(|entry| {
                let rect = bt_utils::DRect::from_min_size(
                    entry.rect.min().as_dvec2(),
                    entry.rect.size().as_dvec2(),
                );
                visible.intersects(rect) && adjacent.is_none_or(|area| area.intersects(rect))
            })
            .collect()
    }

    /// Whether this viewport contains a neighbour whose load has not been requested.
    pub fn needs_world_maps(&self, doc: DocumentId, visible: bt_utils::DRect) -> bool {
        self.world_maps_in_view(doc, visible).iter().any(|entry| {
            self.store.by_path(&entry.file.path).is_none()
                && !self.files.world_requested.contains(&entry.file.path)
        })
    }

    pub(crate) fn load_world_maps(
        &mut self,
        doc: DocumentId,
        visible: bt_utils::DRect,
    ) -> Vec<crate::Effect> {
        let Some(world) = self.world_of(doc) else {
            return Vec::new();
        };
        self.world_maps_in_view(doc, visible)
            .into_iter()
            .filter_map(|entry| {
                if self.store.by_path(&entry.file.path).is_some()
                    || !self.files.world_requested.insert(entry.file.path.clone())
                {
                    return None;
                }
                let mut job = self.load_job(entry.file.path, super::LoadSource::File);
                job.purpose = super::LoadPurpose::WorldMap(world);
                Some(crate::Effect::Load(job))
            })
            .collect()
    }

    pub(super) fn cache_world_map(
        &mut self,
        world: AssetId,
        loaded: super::Loaded,
    ) -> Vec<crate::Effect> {
        if self.store.world(world).is_none() {
            return Vec::new();
        }
        let files = match loaded.outcome {
            Ok(files) => files,
            Err(problem) => {
                return vec![crate::Effect::Notify(crate::Notice::error(format!(
                    "Cannot load world map {}: {problem}",
                    loaded.path.display()
                )))];
            }
        };
        if self.store.by_path(&loaded.canonical).is_some() {
            return Vec::new();
        }
        let crate::Root::Map(map) = files.content else {
            return Vec::new();
        };
        self.adopt_dependencies(files.dependencies);
        let mut map = (*map).clone();
        self.store.refresh_tilesets(&mut map);
        let asset = self.store.insert(crate::NewAsset {
            root: crate::Root::Map(std::sync::Arc::new(map)),
            path: Some(loaded.canonical),
            content_hash: files.content_hash,
            warnings: files.warnings,
        });
        self.files.loaders.insert(asset, files.loader);
        self.sync_template_caches();
        Vec::new()
    }

    pub(crate) fn move_world_map(
        &mut self,
        doc: DocumentId,
        world: AssetId,
        path: &Path,
        position: glam::IVec2,
    ) -> Vec<crate::Effect> {
        let Some(source) = self.store.world(world) else {
            return Vec::new();
        };
        let Some(index) = source.maps.iter().position(|entry| entry.file.path == path) else {
            return vec![crate::Effect::Notify(crate::Notice::error(
                "Maps placed by filename patterns cannot move individually; edit their world pattern",
            ))];
        };
        if self.world_of(doc) != Some(world) {
            return Vec::new();
        }
        self.transact(doc, "Move Map in World", |tx| {
            if let Some(entry) = tx.world_mut(world)?.maps.get_mut(index) {
                entry.rect.x = position.x;
                entry.rect.y = position.y;
            }
            Ok::<_, crate::EditError>(())
        })
        .err()
        .map(|error| crate::Effect::Notify(crate::Notice::error(error.to_string())))
        .into_iter()
        .collect()
    }
}

impl Editor {
    pub(crate) fn create_world(&mut self, doc: DocumentId, path: &Path) -> Vec<crate::Effect> {
        let Some(map_path) = self.path_of(doc) else {
            return vec![crate::Effect::Notify(crate::Notice::error(
                "Save the map before creating its world",
            ))];
        };
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => {
                return vec![crate::Effect::Notify(crate::Notice::error(
                    error.to_string(),
                ))];
            }
        };
        if self.store.by_path(&path).is_some() {
            return vec![crate::Effect::Notify(crate::Notice::error(
                "That file is already loaded",
            ))];
        }
        let Some(folder) = path.parent() else {
            return Vec::new();
        };
        let value = bt_core::World {
            maps: vec![WorldMap {
                file: bt_core::FileRef::new(
                    bt_utils::paths::reference_path(folder, &map_path),
                    map_path,
                ),
                rect: bt_utils::IRect::from_min_size(glam::IVec2::ZERO, map.bounding_rect().size()),
            }],
            ..bt_core::World::default()
        };
        let world = self.store.insert(crate::NewAsset {
            root: crate::Root::World(std::sync::Arc::new(value)),
            path: Some(path.clone()),
            content_hash: None,
            warnings: Vec::new(),
        });
        self.files.active_world = Some(world);
        self.session_changed();
        self.remember_recent(&path);
        self.write_auxiliary(world)
            .err()
            .map(crate::Effect::Notify)
            .into_iter()
            .collect()
    }

    pub(crate) fn set_world(
        &mut self,
        doc: DocumentId,
        world: AssetId,
        value: bt_core::World,
    ) -> Vec<crate::Effect> {
        if let Some(problem) = world_problem(&value) {
            return vec![crate::Effect::Notify(crate::Notice::error(problem))];
        }
        if self.store.world(world).is_none() || self.document(doc).is_none() {
            return Vec::new();
        }
        let result = self.transact(doc, "Edit World Properties", |tx| {
            *tx.world_mut(world)? = value;
            Ok::<_, crate::EditError>(())
        });
        if result.is_ok() {
            self.files.world_requested.clear();
        }
        result
            .err()
            .map(|error| crate::Effect::Notify(crate::Notice::error(error.to_string())))
            .into_iter()
            .collect()
    }

    pub(crate) fn add_world_map(
        &self,
        doc: DocumentId,
        world: AssetId,
        path: &Path,
    ) -> Vec<crate::Effect> {
        let mut job = self.load_job(path.to_path_buf(), super::LoadSource::File);
        job.purpose = super::LoadPurpose::AddWorldMap { doc, world };
        vec![crate::Effect::Load(job)]
    }

    pub(super) fn finish_adding_world_map(
        &mut self,
        doc: DocumentId,
        world: AssetId,
        loaded: super::Loaded,
    ) -> Vec<crate::Effect> {
        let size = match &loaded.outcome {
            Ok(files) => match &files.content {
                crate::Root::Map(map) => map.bounding_rect().size(),
                _ => return Vec::new(),
            },
            Err(problem) => {
                return vec![crate::Effect::Notify(crate::Notice::error(problem.clone()))];
            }
        };
        if self.document(doc).is_none() {
            return Vec::new();
        }
        let Some(source) = self.store.world(world) else {
            return Vec::new();
        };
        let Some(folder) = self
            .store
            .get(world)
            .and_then(|asset| asset.path())
            .and_then(Path::parent)
        else {
            return Vec::new();
        };
        let path = loaded.canonical.clone();
        if self
            .world_maps(world)
            .iter()
            .any(|entry| entry.file.path == path)
        {
            return vec![crate::Effect::Notify(crate::Notice::info(
                "That map is already in this world",
            ))];
        }
        let next_x = self
            .world_maps(world)
            .iter()
            .map(|entry| entry.rect.x.saturating_add(entry.rect.w))
            .max()
            .unwrap_or(0);
        let _ = source;
        let entry = WorldMap {
            file: bt_core::FileRef::new(bt_utils::paths::reference_path(folder, &path), &path),
            rect: bt_utils::IRect::from_min_size(glam::IVec2::new(next_x, 0), size),
        };
        let mut effects = self.cache_world_map(world, loaded);
        if let Err(error) = self.transact(doc, "Add Map to World", |tx| {
            tx.world_mut(world)?.maps.push(entry);
            Ok::<_, crate::EditError>(())
        }) {
            effects.push(crate::Effect::Notify(crate::Notice::error(
                error.to_string(),
            )));
        }
        effects
    }

    pub(crate) fn unload_world(&mut self, world: AssetId) -> Vec<crate::Effect> {
        if let Some(asset) = self.store.get(world).filter(|asset| asset.is_dirty()) {
            return vec![self.ask(
                crate::Question::Unsaved {
                    names: vec![asset.display_name()],
                    purpose: crate::Purpose::Close,
                },
                super::Pending::UnloadWorld(world),
            )];
        }
        self.unload_world_now(world)
    }

    pub(super) fn unload_world_now(&mut self, world: AssetId) -> Vec<crate::Effect> {
        for document in self.documents.iter_mut() {
            document.history.forget_asset(world);
        }
        self.store.remove(world);
        self.files.world_patterns.remove(&world);
        self.files.world_pattern_requests.remove(&world);
        self.files.world_pattern_results.remove(&world);
        if self.files.active_world == Some(world) {
            self.files.active_world = None;
        }
        self.files.world_requested.clear();
        self.recovery.forget(world);
        self.session_changed();
        vec![crate::Effect::DeleteSnapshot(
            self.recovery.snapshot_path(world),
        )]
    }
}

/// Why a World Properties draft cannot apply.
pub fn world_problem(world: &bt_core::World) -> Option<String> {
    if world.grid_size.x < 0
        || world.grid_size.y < 0
        || (world.grid_size.x == 0) != (world.grid_size.y == 0)
    {
        return Some("World grid dimensions must both be zero or both positive".into());
    }
    if world
        .maps
        .iter()
        .any(|map| map.file.source.trim().is_empty() || map.rect.w <= 0 || map.rect.h <= 0)
    {
        return Some("Every map needs a filename and a positive width and height".into());
    }
    for pattern in &world.patterns {
        if let Some(problem) = bt_tmx::world_pattern_problem(pattern) {
            return Some(format!("Pattern '{}': {problem}", pattern.regexp));
        }
    }
    None
}

impl Editor {
    pub(crate) fn sync_world_patterns(&mut self) -> Vec<crate::Effect> {
        let mut effects = Vec::new();
        for asset in self.store.assets() {
            let crate::Root::World(world) = asset.root() else {
                continue;
            };
            if self.files.world_pattern_requests.get(&asset.id()) == Some(&asset.version()) {
                continue;
            }
            self.files
                .world_pattern_requests
                .insert(asset.id(), asset.version());
            if world.patterns.is_empty() {
                self.files.world_patterns.remove(&asset.id());
                continue;
            }
            let Some(path) = asset.path() else { continue };
            match bt_tmx::WriteRequest::for_world(world, path).render() {
                Ok(written) => {
                    let mut job =
                        self.load_job(path.to_path_buf(), super::LoadSource::Text(written.text));
                    job.purpose = super::LoadPurpose::WorldPatterns {
                        world: asset.id(),
                        version: asset.version(),
                    };
                    effects.push(crate::Effect::Load(job));
                }
                Err(error) => effects.push(crate::Effect::Notify(crate::Notice::error(
                    error.to_string(),
                ))),
            }
        }
        effects
    }

    pub(super) fn finish_world_patterns(
        &mut self,
        world: AssetId,
        version: u64,
        loaded: super::Loaded,
    ) -> Vec<crate::Effect> {
        let Some(asset) = self
            .store
            .get(world)
            .filter(|asset| asset.version() == version)
        else {
            return Vec::new();
        };
        let Some(value) = self.store.world(world) else {
            return Vec::new();
        };
        let _ = asset;
        match loaded.outcome {
            Ok(files) => {
                self.files.world_patterns.insert(
                    world,
                    files
                        .world_maps
                        .into_iter()
                        .skip(value.maps.len())
                        .collect(),
                );
                self.files.world_pattern_results.insert(world, version);
                Vec::new()
            }
            Err(problem) => vec![crate::Effect::Notify(crate::Notice::error(format!(
                "Cannot expand world patterns: {problem}"
            )))],
        }
    }
}
