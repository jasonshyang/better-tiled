//! Reading a map's rules on a worker: the rules file and the rule maps it lists, through a
//! loader that has the editor's tilesets, so the rules name the very tilesets the map uses,
//! unsaved changes and all. The files read are hashed, so a change on disk is told from none.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::{RuleSet, files};
use bt_core::{CustomTypes, Tileset};
use bt_tmx::Loader;

use super::rules::ReadFile;
use crate::files::hash_file;
use crate::store::AssetId;
use crate::watch::Found;

/// A tileset the editor has, standing in for its file.
#[derive(Clone, Debug)]
pub(crate) struct KnownTileset {
    /// The file.
    pub(crate) path: PathBuf,
    /// The tileset, as the editor has it.
    pub(crate) tileset: Arc<Tileset>,
}

/// A read of a map's rules, to run on a worker: [`RulesJob::run`] does it, and the result goes
/// back as [`crate::Intent::RulesLoaded`].
#[derive(Clone, Debug)]
pub struct RulesJob {
    pub(crate) map: AssetId,
    pub(crate) read: u64,
    pub(crate) file: PathBuf,
    pub(crate) map_path: Option<PathBuf>,
    pub(crate) custom_types: Arc<CustomTypes>,
    pub(crate) tilesets: Vec<KnownTileset>,
    /// The rules read before, whose rule maps are kept while they cannot be read.
    pub(crate) previous: Option<Arc<RuleSet>>,
}

/// What a read of rules gave.
#[derive(Debug)]
pub struct RulesLoaded {
    pub(crate) map: AssetId,
    pub(crate) read: u64,
    pub(crate) file: PathBuf,
    /// The files read, with what they held: a change to one reads the rules again.
    pub(crate) files: Vec<ReadFile>,
    pub(crate) outcome: Result<RuleSet, String>,
}

impl RulesJob {
    /// The rules file read.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Reads the rules; run it on a worker.
    pub fn run(&self) -> RulesLoaded {
        let mut loader = Loader::builder()
            .custom_types(Arc::clone(&self.custom_types))
            .build();
        for known in &self.tilesets {
            if let Err(error) = loader.insert_tileset(&known.path, Arc::clone(&known.tileset)) {
                // The file is gone: rule maps naming it read it from disk, and say what failed.
                log::warn!("the rules read {} from disk: {error}", known.path.display());
            }
        }
        let read = match &self.previous {
            Some(previous) => {
                files::reload(&mut loader, &self.file, self.map_path.as_deref(), previous)
            }
            None => files::load(&mut loader, &self.file, self.map_path.as_deref()),
        };
        // Every file whose change must read the rules again: after a failed read, those the
        // read got to, so fixing one is seen.
        let outcome = read.map_err(|error| error.to_string());
        let mut paths = match &outcome {
            Ok(rules) => rules.watched_files(),
            Err(_) => files::listed_files(&self.file),
        };
        paths.push(self.file.clone());
        paths.sort();
        paths.dedup();
        RulesLoaded {
            map: self.map,
            read: self.read,
            file: self.file.clone(),
            files: paths
                .into_iter()
                .map(|path| ReadFile {
                    found: found_at(&path),
                    path,
                })
                .collect(),
            outcome,
        }
    }
}

/// What a file holds now.
fn found_at(path: &Path) -> Found {
    match hash_file(path) {
        Ok(content_hash) => Found::Present { content_hash },
        Err(_) => Found::Missing,
    }
}
