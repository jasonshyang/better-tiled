//! The watcher's model: which files the window polls, and what a change on disk means. The
//! window reads and hashes the files on a worker every 800 ms and hands the results in; the
//! decisions are made here. Our own writes are recognised by content hash, never by time.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use bt_core::{ImageRef, ImageSource, LayerKind, Map, Tileset, TilesetImage};

use crate::editor::Editor;
use crate::effect::{Choice, Effect, Question};
use crate::files::{LoadKind, LoadPurpose, LoadSource, Pending};
use crate::notice::Notice;
use crate::store::{AssetId, AssetKind};

/// What the window found for a watched file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// The file.
    pub path: PathBuf,
    /// What is there.
    pub found: Found,
}

/// What a watched file holds now.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// It is gone.
    Missing,
    /// It is there, with the FNV-1a hash of its bytes.
    Present {
        /// The hash.
        content_hash: u64,
    },
}

/// What the editor remembers between polls.
#[derive(Clone, Debug, Default)]
pub(crate) struct WatchModel {
    /// Changes the user chose to keep their own work over: what the file held then.
    kept: HashMap<AssetId, u64>,
    /// Assets being reloaded or asked about, so a later poll does not start again.
    busy: HashSet<AssetId>,
    /// What a busy asset's file was last found to hold, to look at again once it settles: the
    /// window reports a file only when it changes.
    deferred: HashMap<AssetId, Found>,
    /// The content last seen of each image.
    images: HashMap<PathBuf, Found>,
}

impl Editor {
    /// The files the window polls: the documents' files, the external tilesets, the images of
    /// every tileset and image layer, the project file, and the files the maps' rules come from
    /// or may come from.
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut paths = BTreeSet::new();
        for doc in self.documents.in_order() {
            if let Some(path) = self.store.get(doc.asset()).and_then(|a| a.path()) {
                paths.insert(path.to_path_buf());
            }
        }
        for asset in self.store.assets() {
            match asset.kind() {
                AssetKind::Tileset
                | AssetKind::Project
                | AssetKind::Template
                | AssetKind::World => {
                    if let Some(path) = asset.path() {
                        paths.insert(path.to_path_buf());
                    }
                }
                AssetKind::Map => {}
            }
            if let Some(map) = self.store.map(asset.id()) {
                image_paths_of_map(map, &mut paths);
            }
            if let Some(tileset) = self.store.tileset(asset.id()) {
                image_paths_of_tileset(tileset, &mut paths);
            }
            if let Some(template) = self.store.template(asset.id()) {
                for tileset in &template.tilesets {
                    image_paths_of_tileset(&tileset.tileset, &mut paths);
                }
            }
        }
        self.watched_rule_files(&mut paths);
        paths.into_iter().collect()
    }

    /// Takes in what the window found and decides what each change means.
    pub(crate) fn observe(&mut self, observations: Vec<Observation>) -> Vec<Effect> {
        let mut effects = self.observe_rule_files(&observations);
        let mut changed_images = Vec::new();
        for observation in observations {
            match self.store.by_path(&observation.path) {
                Some(asset) => effects.extend(self.observe_asset(asset, observation.found)),
                None if self.is_rule_file(&observation.path) => {}
                None => {
                    let before = self
                        .watch
                        .images
                        .insert(observation.path.clone(), observation.found);
                    if before.is_some_and(|seen| seen != observation.found)
                        && observation.found != Found::Missing
                    {
                        changed_images.push(observation.path);
                    }
                }
            }
        }
        if !changed_images.is_empty() {
            effects.push(Effect::ReloadImages(changed_images));
        }
        effects
    }

    /// A reload or question about `asset` finished: what its file did meanwhile counts now.
    pub(crate) fn settle_watch(&mut self, asset: AssetId) -> Vec<Effect> {
        self.watch.busy.remove(&asset);
        match self.watch.deferred.remove(&asset) {
            Some(found) => self.observe_asset(asset, found),
            None => Vec::new(),
        }
    }

    fn observe_asset(&mut self, id: AssetId, found: Found) -> Vec<Effect> {
        if self.watch.busy.contains(&id) {
            self.watch.deferred.insert(id, found);
            return Vec::new();
        }
        let Some(asset) = self.store.get(id) else {
            return Vec::new();
        };
        let name = asset.display_name();
        let known = asset.on_disk().map(|disk| disk.content_hash);
        let was_missing = asset.is_missing();
        let dirty = asset.is_dirty();
        let kind = asset.kind();
        let Some(path) = asset.path().map(PathBuf::from) else {
            return Vec::new();
        };
        let content_hash = match found {
            Found::Missing => {
                if was_missing {
                    return Vec::new();
                }
                self.store.set_missing(id, true);
                return vec![Effect::Notify(Notice::warning(format!(
                    "{name} was deleted or moved: the open copy is the only one"
                )))];
            }
            Found::Present { content_hash } => content_hash,
        };
        if known == Some(content_hash) {
            if was_missing {
                self.store.set_missing(id, false);
                return vec![Effect::Notify(Notice::info(format!("{name} is back")))];
            }
            return Vec::new();
        }
        if self.watch.kept.get(&id) == Some(&content_hash) {
            return Vec::new();
        }
        let load_kind = match kind {
            AssetKind::Map => LoadKind::Map,
            AssetKind::Tileset => LoadKind::Tileset,
            AssetKind::Template => LoadKind::Template,
            AssetKind::World => LoadKind::World,
            // The project reloads with the Custom Types editor, which keeps unsaved types.
            AssetKind::Project => return Vec::new(),
        };
        self.watch.busy.insert(id);
        if dirty {
            let question = Question::Conflict {
                name,
                returned: was_missing,
            };
            return vec![self.ask(
                question,
                Pending::Conflict {
                    asset: id,
                    content_hash,
                },
            )];
        }
        vec![self.reload(id, path, load_kind)]
    }

    /// A load that replaces an asset's content with what its file holds.
    fn reload(&self, asset: AssetId, path: PathBuf, kind: LoadKind) -> Effect {
        let mut job = self.load_job(path, LoadSource::File);
        job.kind = kind;
        job.purpose = LoadPurpose::Reload(asset);
        Effect::Load(job)
    }

    /// The answer to a file that changed under unsaved work.
    pub(crate) fn decide_conflict(
        &mut self,
        asset: AssetId,
        content_hash: u64,
        choice: Choice,
    ) -> Vec<Effect> {
        let target = self
            .store
            .get(asset)
            .and_then(|a| a.path().map(|p| (p.to_path_buf(), a.kind())));
        match (choice, target) {
            (Choice::Reload, Some((path, kind))) => {
                let kind = match kind {
                    AssetKind::Tileset => LoadKind::Tileset,
                    AssetKind::Map => LoadKind::Map,
                    AssetKind::Project => LoadKind::Project,
                    AssetKind::World => LoadKind::World,
                    AssetKind::Template => LoadKind::Template,
                };
                vec![self.reload(asset, path, kind)]
            }
            _ => {
                self.watch.kept.insert(asset, content_hash);
                self.settle_watch(asset)
            }
        }
    }
}

fn file_of(image: &ImageRef) -> Option<PathBuf> {
    match &image.source {
        ImageSource::File(file) => Some(file.path.clone()),
        ImageSource::Embedded(_) => None,
    }
}

fn image_paths_of_tileset(tileset: &Tileset, paths: &mut BTreeSet<PathBuf>) {
    if let TilesetImage::Atlas { image, .. } = &tileset.image {
        paths.extend(file_of(image));
    }
    for tile in tileset.tiles.values() {
        paths.extend(tile.image.as_ref().and_then(file_of));
    }
}

fn image_paths_of_map(map: &Map, paths: &mut BTreeSet<PathBuf>) {
    for tileset in &map.tilesets {
        image_paths_of_tileset(&tileset.tileset, paths);
    }
    map.for_each_layer(|layer, _| {
        if let LayerKind::Image(image) = &layer.kind {
            paths.extend(image.image.as_ref().and_then(file_of));
        }
    });
}
