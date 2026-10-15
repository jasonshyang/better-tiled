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
