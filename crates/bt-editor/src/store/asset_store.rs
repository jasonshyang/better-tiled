//! The asset store: every loaded map, tileset and project, found by id or by file.
//!
//! The store is the authority on tilesets: a map's external `TilesetRef`s hold the store's root
//! for that file, put back whenever a map root is restored or loaded, without changing the map's
//! version (the map did not change, its tileset did).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Map, Project, Tileset};
use bt_tmx::Warning;

use super::asset::{Asset, AssetId, AssetKind, OnDisk, Root};

/// Every loaded asset.
#[derive(Clone, Debug, Default)]
pub struct AssetStore {
    assets: BTreeMap<AssetId, Asset>,
    by_path: HashMap<PathBuf, AssetId>,
    next_id: u64,
}

/// A new asset for [`AssetStore::insert`]: what it holds, its file, and what is on disk.
#[derive(Clone, Debug)]
pub struct NewAsset {
    /// What it holds.
    pub root: Root,
    /// Its file, if it has one.
    pub path: Option<PathBuf>,
    /// The hash of the file it was read from, when it was read from one.
    pub content_hash: Option<u64>,
    /// The warnings of its load.
    pub warnings: Vec<Warning>,
}

impl AssetStore {
    /// An empty store.
    pub fn new() -> AssetStore {
        AssetStore::default()
    }

    /// Adds an asset; one read from a file starts clean at version 1, a new one dirty.
    pub fn insert(&mut self, new: NewAsset) -> AssetId {
        self.next_id += 1;
        let id = AssetId(self.next_id);
        let on_disk = new.content_hash.map(|content_hash| OnDisk {
            version: 1,
            content_hash,
        });
        if let Some(path) = &new.path {
            self.by_path.insert(path.clone(), id);
        }
        self.assets.insert(
            id,
            Asset {
                id,
                path: new.path,
                root: new.root,
                version: 1,
                high_water: 1,
                on_disk,
                missing: false,
                warnings: new.warnings,
            },
        );
        id
    }

    /// An asset.
    pub fn get(&self, id: AssetId) -> Option<&Asset> {
        self.assets.get(&id)
    }

    /// Every asset, by id.
    pub fn assets(&self) -> impl Iterator<Item = &Asset> {
        self.assets.values()
    }

    /// The asset of a file.
    pub fn by_path(&self, path: &Path) -> Option<AssetId> {
        self.by_path.get(path).copied()
    }

    /// A map asset's map.
    pub fn map(&self, id: AssetId) -> Option<&Arc<Map>> {
        match &self.get(id)?.root {
            Root::Map(map) => Some(map),
            _ => None,
        }
    }

    /// A tileset asset's tileset.
    pub fn tileset(&self, id: AssetId) -> Option<&Arc<Tileset>> {
        match &self.get(id)?.root {
            Root::Tileset(tileset) => Some(tileset),
            _ => None,
        }
    }

    /// A project asset's project.
    pub fn project(&self, id: AssetId) -> Option<&Arc<Project>> {
        match &self.get(id)?.root {
            Root::Project(project) => Some(project),
            _ => None,
        }
    }

    /// The assets with changes their files lack, whatever their kind: what Save All saves and
    /// what quitting asks about. An untitled project (custom types made with no project open)
    /// lives for the run, as Tiled's does, and is never unsaved.
    pub fn unsaved(&self) -> Vec<AssetId> {
        self.assets
            .values()
            .filter(|asset| asset.is_dirty())
            .filter(|asset| asset.kind() != AssetKind::Project || asset.path.is_some())
            .map(|asset| asset.id)
            .collect()
    }

    /// The tileset assets a map uses, by its external tilesets' files.
    pub fn tilesets_of(&self, map: &Map) -> Vec<AssetId> {
        map.tilesets
            .iter()
            .filter_map(|t| t.file.as_ref())
            .filter_map(|file| self.by_path(&file.path))
            .filter(|id| {
                self.get(*id)
                    .is_some_and(|a| a.kind() == AssetKind::Tileset)
            })
            .collect()
    }

    /// Points a map's external tilesets at the store's roots for their files; whether any
    /// changed.
    pub fn refresh_tilesets(&self, map: &mut Map) -> bool {
        let mut changed = false;
        for tileset in &mut map.tilesets {
            let Some(file) = &tileset.file else { continue };
            let Some(current) = self.by_path(&file.path).and_then(|id| self.tileset(id)) else {
                continue;
            };
            if !Arc::ptr_eq(current, &tileset.tileset) {
                tileset.tileset = Arc::clone(current);
                changed = true;
            }
        }
        changed
    }

    /// Replaces an asset's root and version (a transaction, undo or redo).
    pub(crate) fn set_root(&mut self, id: AssetId, root: Root, version: u64) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.root = root;
            asset.version = version;
        }
        self.refresh_maps_using(id);
    }

    /// Replaces an asset's root and version without refreshing what uses it (a map whose own
    /// tilesets were just refreshed).
    fn set_root_only(&mut self, id: AssetId, root: Root) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.root = root;
        }
    }

    /// The next version for an asset: above every version it has had, so a change made after
    /// an undo never looks like the saved version.
    pub(crate) fn next_version(&mut self, id: AssetId) -> u64 {
        match self.assets.get_mut(&id) {
            Some(asset) => {
                asset.high_water += 1;
                asset.high_water
            }
            None => 1,
        }
    }

    /// After a tileset asset changed: the maps using its file take its new root, their versions
    /// unchanged.
    fn refresh_maps_using(&mut self, id: AssetId) {
        if self
            .get(id)
            .is_none_or(|asset| asset.kind() != AssetKind::Tileset)
        {
            return;
        }
        let maps: Vec<AssetId> = self
            .assets
            .values()
            .filter(|asset| asset.kind() == AssetKind::Map)
            .map(|asset| asset.id)
            .collect();
        for map_id in maps {
            let Some(map) = self.map(map_id).cloned() else {
                continue;
            };
            let mut refreshed = (*map).clone();
            if self.refresh_tilesets(&mut refreshed) {
                self.set_root_only(map_id, Root::Map(Arc::new(refreshed)));
            }
        }
    }

    /// Records that the asset's current version is what its file holds.
    pub(crate) fn mark_saved(&mut self, id: AssetId, content_hash: u64) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.on_disk = Some(OnDisk {
                version: asset.version,
                content_hash,
            });
            asset.missing = false;
        }
    }

    /// Records whether an asset's file is gone (the asset is then the only copy, and dirty).
    pub(crate) fn set_missing(&mut self, id: AssetId, missing: bool) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.missing = missing;
        }
    }

    /// Moves an asset to another file (Save As).
    pub(crate) fn retarget(&mut self, id: AssetId, path: PathBuf) {
        let Some(asset) = self.assets.get_mut(&id) else {
            return;
        };
        if let Some(old) = asset.path.take() {
            self.by_path.remove(&old);
        }
        self.by_path.insert(path.clone(), id);
        asset.path = Some(path);
        asset.on_disk = None;
    }

    /// Takes an asset out of the store.
    pub(crate) fn remove(&mut self, id: AssetId) -> Option<Asset> {
        let asset = self.assets.remove(&id)?;
        if let Some(path) = &asset.path {
            self.by_path.remove(path);
        }
        Some(asset)
    }

    /// Marks a recovered asset: it has no file version on disk, so it is dirty until saved.
    pub(crate) fn forget_disk(&mut self, id: AssetId) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.on_disk = None;
        }
    }
}
