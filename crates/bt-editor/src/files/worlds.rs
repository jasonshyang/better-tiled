//! Loaded worlds are assets; their explicit map positions remain part of map history.

use crate::{AssetId, DocumentId, Editor};
use bt_core::WorldMap;
use std::path::Path;

impl Editor {
    /// The first loaded world containing a document's map.
    pub fn world_of(&self, doc: DocumentId) -> Option<AssetId> {
        let path = self.path_of(doc)?;
        self.worlds_for_path(&path).into_iter().next()
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
        if let Some(patterns) = self.files.world_patterns.get(&world) {
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
