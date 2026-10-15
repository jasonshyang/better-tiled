//! The asset store: every loaded map, tileset and project, found by id or by file.
//!
//! The store is the authority on tilesets: a map's external `TilesetRef`s hold the store's root
//! for that file, put back whenever a map root is restored or loaded, without changing the map's
//! version (the map did not change, its tileset did).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{Map, ObjectTemplate, Project, Tileset, TilesetRef, World};
use bt_tmx::Warning;

use super::asset::{Asset, AssetId, AssetKind, OnDisk, Root};

/// Every loaded asset.
#[derive(Clone, Debug, Default)]
pub struct AssetStore {
    assets: BTreeMap<AssetId, Asset>,
    by_path: HashMap<PathBuf, AssetId>,
    next_id: u64,
    generation: u64,
    template_views: HashMap<AssetId, Arc<Map>>,
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

    /// Revision of loaded asset state, including references, paths and on-disk status.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn set_warnings(&mut self, id: AssetId, warnings: Vec<Warning>) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.warnings = warnings;
            self.generation += 1;
        }
    }

    pub(crate) fn clear_file_warnings(&mut self, path: &Path) {
        for asset in self.assets.values_mut() {
            let before = asset.warnings.len();
            asset
                .warnings
                .retain(|warning| warning.related_file.as_deref() != Some(path));
            if asset.warnings.len() != before {
                self.generation += 1;
            }
        }
    }

    /// Adds an asset; one read from a file starts clean at version 1, a new one dirty.
    pub fn insert(&mut self, mut new: NewAsset) -> AssetId {
        self.generation += 1;
        new.root = self.refresh_root_references(new.root);
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
        self.refresh_template_view(id);
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

    /// A world asset's world.
    pub fn world(&self, id: AssetId) -> Option<&Arc<World>> {
        match &self.get(id)?.root {
            Root::World(world) => Some(world),
            _ => None,
        }
    }

    /// A template asset's definition.
    pub fn template(&self, id: AssetId) -> Option<&Arc<ObjectTemplate>> {
        match &self.get(id)?.root {
            Root::Template(template) => Some(template),
            _ => None,
        }
    }

    /// A map document's map or a template document's derived editing view.
    pub(crate) fn editing_map(&self, id: AssetId) -> Option<&Arc<Map>> {
        self.map(id).or_else(|| self.template_views.get(&id))
    }

    fn refresh_template_view(&mut self, id: AssetId) {
        if let Some(template) = self.template(id) {
            self.template_views
                .insert(id, Arc::new(super::template_view::of(template)));
        }
    }

    /// Normalises edited template views and includes their instances in the same transaction.
    pub(crate) fn finish_working_roots(
        &self,
        mut working: BTreeMap<AssetId, Root>,
    ) -> Result<BTreeMap<AssetId, Root>, bt_edit::EditError> {
        for (id, root) in &mut working {
            if let Some(original) = self.template(*id)
                && let Root::Map(map) = root
            {
                *root = Root::Template(Arc::new(super::template_view::to_template(map, original)?));
            }
        }
        let changed_templates = working
            .iter()
            .filter_map(|(id, root)| {
                let Root::Template(after) = root else {
                    return None;
                };
                let asset = self.get(*id)?;
                let before = self.template(*id)?;
                if before == after {
                    return None;
                }
                Some(TemplateChange {
                    path: asset.path()?.to_path_buf(),
                    before: Arc::clone(before),
                    after: Arc::clone(after),
                })
            })
            .collect::<Vec<_>>();
        for change in changed_templates {
            for asset in self.assets() {
                let mut root = working
                    .get(&asset.id())
                    .unwrap_or_else(|| asset.root())
                    .clone();
                if root.refresh_template_instances(&change.path, &change.before, &change.after)? {
                    working.insert(asset.id(), root);
                }
            }
        }
        Ok(working)
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
        self.tileset_assets(&map.tilesets)
    }

    fn tileset_assets(&self, references: &[TilesetRef]) -> Vec<AssetId> {
        references
            .iter()
            .filter_map(|reference| reference.file.as_ref())
            .filter_map(|file| self.by_path(&file.path))
            .filter(|id| self.tileset(*id).is_some())
            .collect()
    }

    /// Points a map's external tilesets at the store's roots for their files.
    pub fn refresh_tilesets(&self, map: &mut Map) -> bool {
        self.refresh_references(&mut map.tilesets)
    }

    fn refresh_references(&self, references: &mut [TilesetRef]) -> bool {
        let mut changed = false;
        for tileset in references {
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

    fn refresh_root_references(&self, mut root: Root) -> Root {
        match &mut root {
            Root::Map(map) => {
                let mut references = map.tilesets.clone();
                if self.refresh_references(&mut references) {
                    Arc::make_mut(map).tilesets = references;
                }
            }
            Root::Template(template) => {
                let mut references = template.tilesets.clone();
                if self.refresh_references(&mut references) {
                    Arc::make_mut(template).tilesets = references;
                }
            }
            _ => {}
        }
        root
    }

    /// Replaces an asset's root and version (a transaction, undo or redo).
    pub(crate) fn set_root(&mut self, id: AssetId, root: Root, version: u64) {
        self.generation += 1;
        let root = self.refresh_root_references(root);
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.root = root;
            asset.version = version;
        }
        self.refresh_template_view(id);
        self.refresh_maps_using(id);
    }

    /// Replaces an asset's root and version without refreshing what uses it (a map whose own
    /// tilesets were just refreshed).
    fn set_root_only(&mut self, id: AssetId, root: Root) {
        self.generation += 1;
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
        let ids: Vec<AssetId> = self
            .assets
            .values()
            .filter(|asset| matches!(asset.kind(), AssetKind::Map | AssetKind::Template))
            .map(|asset| asset.id)
            .collect();
        for id in ids {
            let root = match self.get(id).map(Asset::root) {
                Some(Root::Map(map)) => {
                    let mut refreshed = (**map).clone();
                    self.refresh_tilesets(&mut refreshed)
                        .then(|| Root::Map(Arc::new(refreshed)))
                }
                Some(Root::Template(template)) => {
                    let mut refreshed = (**template).clone();
                    self.refresh_references(&mut refreshed.tilesets)
                        .then(|| Root::Template(Arc::new(refreshed)))
                }
                _ => None,
            };
            if let Some(root) = root {
                self.set_root_only(id, root);
                self.refresh_template_view(id);
            }
        }
    }

    /// Records that the asset's current version is what its file holds.
    pub(crate) fn mark_saved(&mut self, id: AssetId, content_hash: u64) {
        self.generation += 1;
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
        self.generation += 1;
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.missing = missing;
        }
    }

    /// Moves an asset to another file (Save As).
    pub(crate) fn retarget(&mut self, id: AssetId, path: PathBuf) {
        self.generation += 1;
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
        self.generation += 1;
        self.template_views.remove(&id);
        let asset = self.assets.remove(&id)?;
        if let Some(path) = &asset.path {
            self.by_path.remove(path);
        }
        Some(asset)
    }

    /// Marks a recovered asset: it has no file version on disk, so it is dirty until saved.
    pub(crate) fn forget_disk(&mut self, id: AssetId) {
        self.generation += 1;
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.on_disk = None;
        }
    }
}

struct TemplateChange {
    path: PathBuf,
    before: Arc<ObjectTemplate>,
    after: Arc<ObjectTemplate>,
}
