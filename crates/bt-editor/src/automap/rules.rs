//! What the editor keeps of each map's rules: the rules file found, the rules read (kept
//! through a failed read), the files they were read from, a run waiting for them, and the last
//! run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::{Area, IdleRuleMap, Report, RuleSet};
use bt_core::{Map, Project};

use super::explaining::Explained;
use super::live::LiveResult;
use crate::document::DocumentId;
use crate::store::AssetId;
use crate::watch::Found;

/// The seed of the first AutoMap run; each run moves it on, as the brush's does.
const FIRST_SEED: u64 = 0xA070_3A9D_5EED;

/// AutoMap's state: the rules of each map they were looked for, and the next run's seed.
#[derive(Debug)]
pub(crate) struct AutoMapState {
    /// Each map's rules, by its asset.
    pub(crate) maps: HashMap<AssetId, AutoMapRules>,
    /// Problems restoring painted state, retained alongside the document.
    pub(crate) restoration: HashMap<AssetId, Vec<bt_automap::Problem>>,
    /// The seed of the next run.
    pub(crate) seed: u64,
    /// The number of the next read, so that only the latest read of a map's rules counts.
    pub(crate) next_read: u64,
    /// The project whose custom types the rules were last checked against.
    pub(crate) types_seen: Option<Arc<Project>>,
    /// AutoMap layers' result of each document they are on in.
    pub(crate) live: HashMap<DocumentId, LiveResult>,
    /// The map as its file holds it, for each document opened with AutoMap layers, until the
    /// first result is checked against it.
    pub(crate) saved: HashMap<DocumentId, Arc<Map>>,
    /// The cell the Explain tool chose in each document.
    pub(crate) explaining: HashMap<DocumentId, glam::IVec2>,
    /// What the rules do at that cell, with what it was made from.
    pub(crate) explained: HashMap<DocumentId, Explained>,
}

impl Default for AutoMapState {
    fn default() -> AutoMapState {
        AutoMapState {
            maps: HashMap::new(),
            restoration: HashMap::new(),
            seed: FIRST_SEED,
            next_read: 1,
            types_seen: None,
            live: HashMap::new(),
            saved: HashMap::new(),
            explaining: HashMap::new(),
            explained: HashMap::new(),
        }
    }
}

/// A map's rules, as the AutoMap command and panel see them.
#[derive(Clone, Debug, Default)]
pub struct AutoMapRules {
    /// The rules file found for the map; `None` when there is none.
    pub(crate) file: Option<PathBuf>,
    /// The rules read from it (the earlier ones while a read fails).
    pub(crate) rules: Option<Arc<RuleSet>>,
    /// The read under way, by its number.
    pub(crate) reading: Option<u64>,
    /// Why the last read failed, until a read succeeds.
    pub(crate) error: Option<String>,
    /// The files the rules were read from, with what they held then.
    pub(crate) files: Vec<ReadFile>,
    /// A run asked for while the rules were being read: it runs when they arrive.
    pub(crate) waiting: Option<Area>,
    /// AutoMap layers asked to start while the rules were being read: they start when the
    /// rules arrive.
    pub(crate) waiting_start: bool,
    /// The last run.
    pub(crate) last_run: Option<LastRun>,
}

impl AutoMapRules {
    /// The rules file the map's rules come from: the project's or a `rules.txt` beside the map;
    /// `None` when there is none.
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The rules read (the earlier ones while a read of changed files fails).
    pub fn rules(&self) -> Option<&Arc<RuleSet>> {
        self.rules.as_ref()
    }

    /// Whether the rules are being read.
    pub fn is_reading(&self) -> bool {
        self.reading.is_some()
    }

    /// Why the rules could not be read the last time, until they can.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The last run.
    pub fn last_run(&self) -> Option<&LastRun> {
        self.last_run.as_ref()
    }
}

/// A run of the rules: what the AutoMap panel reports and Explain explains.
#[derive(Clone, Debug)]
pub struct LastRun {
    /// The rules that ran.
    pub rules: Arc<RuleSet>,
    /// What the run covered.
    pub area: Area,
    /// The seed it drew from.
    pub seed: u64,
    /// The map before the run.
    pub before: Arc<Map>,
    /// The map right after it.
    pub after: Arc<Map>,
    /// What the run did.
    pub report: Report,
    /// The rule maps that matched nothing, with the reasons their inputs give.
    pub idle: Vec<IdleRuleMap>,
}

/// A file rules were read from, with what it held then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReadFile {
    /// The file.
    pub(crate) path: PathBuf,
    /// What it held.
    pub(crate) found: Found,
}
