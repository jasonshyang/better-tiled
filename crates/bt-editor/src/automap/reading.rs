//! Finding and reading each map's rules: the first time the map is current, when AutoMap runs,
//! and when a file they were read from changes or a `rules.txt` appears beside the map. A read
//! that fails keeps the rules there were, with a notice saying why.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::files;

use super::job::{KnownTileset, RulesJob, RulesLoaded};
use super::rules::AutoMapRules;
use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::notice::Notice;
use crate::store::{AssetId, AssetKind};
use crate::watch::{Found, Observation};

/// The rules file Tiled looks for beside a map.
const BESIDE_THE_MAP: &str = "rules.txt";

impl Editor {
    /// The rules of a document's map, once they were looked for.
    pub fn automap_rules(&self, doc: DocumentId) -> Option<&AutoMapRules> {
        let asset = self.documents.get(doc)?.asset();
        self.automap.maps.get(&asset)
    }

    /// What is due for the rules: the current map's looked for the first time it is current,
    /// and rules read again when the project's custom types now give one of their tiles another
    /// `MatchType` through its class.
    pub(crate) fn tick_rules(&mut self) -> Vec<Effect> {
        let mut effects = self.look_for_current_rules();
        let project = self.project_root();
        let seen = match (&self.automap.types_seen, &project) {
            (Some(seen), Some(now)) => Arc::ptr_eq(seen, now),
            (None, None) => true,
            _ => false,
        };
        if seen {
            return effects;
        }
        self.automap.types_seen = project;
        let types = self.custom_types();
        let stale: Vec<AssetId> = self
            .automap
            .maps
            .iter()
            .filter(|(_, rules)| {
                rules
                    .rules
                    .as_ref()
                    .is_some_and(|set| set.classes_changed(types))
            })
            .map(|(asset, _)| *asset)
            .collect();
        for asset in stale {
            effects.extend(self.find_rules(asset, true));
        }
        effects
    }

    /// Looks for the current map's rules the first time it is current, so the AutoMap panel
    /// can say what they are before they run.
    fn look_for_current_rules(&mut self) -> Vec<Effect> {
        let Some(asset) = self
            .documents
            .current()
            .and_then(|doc| self.documents.get(doc))
            .map(|d| d.asset())
        else {
            return Vec::new();
        };
        if self.automap.maps.contains_key(&asset) || self.store.map(asset).is_none() {
            return Vec::new();
        }
        self.find_rules(asset, false)
    }

    /// Looks for a map's rules (the project's rules file, else a `rules.txt` beside the map)
    /// and reads them when they are another file's, were never read, or `reread` asks.
    pub(crate) fn find_rules(&mut self, asset: AssetId, reread: bool) -> Vec<Effect> {
        let map_path = self.map_path(asset);
        let project = self.project_root();
        let chosen = self
            .documents
            .in_order()
            .find(|d| d.asset() == asset)
            .and_then(|d| d.automap.as_ref())
            .map(|setup| setup.rules.clone());
        let found = files::find_rules(map_path.as_deref(), chosen.as_deref(), project.as_deref());
        let rules = self.automap.maps.entry(asset).or_default();
        let another_file = rules.file != found;
        if another_file {
            *rules = AutoMapRules {
                file: found,
                waiting: rules.waiting.take(),
                last_run: rules.last_run.take(),
                ..AutoMapRules::default()
            };
        }
        let unread = rules.rules.is_none() && rules.reading.is_none();
        if rules.file.is_none() || !(reread || another_file || unread) {
            return Vec::new();
        }
        self.read_rules(asset).into_iter().collect()
    }

    /// Reads a map's rules on a worker; a read already under way no longer counts.
    fn read_rules(&mut self, asset: AssetId) -> Option<Effect> {
        let map_path = self.map_path(asset);
        let custom_types = Arc::new(self.custom_types().clone());
        let tilesets = self
            .store
            .assets()
            .filter(|a| a.kind() == AssetKind::Tileset)
            .filter_map(|a| {
                Some(KnownTileset {
                    path: a.path()?.to_path_buf(),
                    tileset: Arc::clone(self.store.tileset(a.id())?),
                })
            })
            .collect();
        let read = self.automap.next_read;
        self.automap.next_read += 1;
        let rules = self.automap.maps.get_mut(&asset)?;
        let file = rules.file.clone()?;
        rules.reading = Some(read);
        Some(Effect::LoadRules(RulesJob {
            map: asset,
            read,
            file,
            map_path,
            custom_types,
            tilesets,
            previous: rules.rules.clone(),
        }))
    }

    /// Takes in rules read on a worker, and runs what waited for them.
    pub(crate) fn finish_reading_rules(&mut self, loaded: RulesLoaded) -> Vec<Effect> {
        let Some(rules) = self.automap.maps.get_mut(&loaded.map) else {
            return Vec::new();
        };
        if rules.reading != Some(loaded.read) {
            return Vec::new();
        }
        rules.reading = None;
        let mut effects = Vec::new();
        match loaded.outcome {
            Ok(read) => {
                rules.rules = Some(Arc::new(read));
                rules.files = loaded.files;
                rules.error = None;
            }
            Err(error) => {
                for file in loaded.files {
                    rules.files.retain(|known| known.path != file.path);
                    rules.files.push(file);
                }
                let name = bt_utils::paths::display_name(&loaded.file);
                let kept = if rules.rules.is_some() {
                    ": the rules read before stay in use until it is fixed"
                } else {
                    ""
                };
                effects.push(Effect::Notify(
                    Notice::error(format!("Cannot read the rules in {name}{kept}"))
                        .with_detail(error.clone()),
                ));
                rules.error = Some(error);
                if rules.rules.is_none() {
                    rules.waiting = None;
                    rules.waiting_start = false;
                }
            }
        }
        let waiting = rules.waiting.take();
        let start = std::mem::take(&mut rules.waiting_start);
        let doc = self
            .documents
            .in_order()
            .find(|d| d.asset() == loaded.map)
            .map(|d| d.id());
        if let (Some(area), Some(doc)) = (waiting, doc) {
            effects.extend(self.run_rules(doc, area));
        }
        if let (true, Some(doc)) = (start, doc) {
            effects.extend(self.start_live(doc));
        }
        effects
    }

    /// The files the maps' rules came from, and the `rules.txt` that would be read beside each
    /// map, which counts once it appears.
    pub(crate) fn watched_rule_files(&self, paths: &mut BTreeSet<PathBuf>) {
        for (asset, rules) in &self.automap.maps {
            paths.extend(rules.files.iter().map(|file| file.path.clone()));
            if let Some(beside) = self.rules_beside(*asset) {
                paths.insert(beside);
            }
        }
    }

    /// Whether a watched file is one the maps' rules come from or may come from.
    pub(crate) fn is_rule_file(&self, path: &Path) -> bool {
        self.automap.maps.iter().any(|(asset, rules)| {
            rules.files.iter().any(|file| file.path == path)
                || self.rules_beside(*asset).as_deref() == Some(path)
        })
    }

    /// Takes in what the window found of the maps' rule files: rules whose files changed are
    /// read again, and a map that gained or lost a `rules.txt` looks for its rules again.
    pub(crate) fn observe_rule_files(&mut self, observations: &[Observation]) -> Vec<Effect> {
        let mut changed = BTreeSet::new();
        for (asset, rules) in &self.automap.maps {
            let beside = self.rules_beside(*asset);
            let touched = observations.iter().any(|observation| {
                let read = rules.files.iter().find(|f| f.path == observation.path);
                match read {
                    Some(read) => read.found != observation.found,
                    // A rules.txt beside the map that is not the rules read, appearing.
                    None => {
                        beside.as_deref() == Some(observation.path.as_path())
                            && observation.found != Found::Missing
                    }
                }
            });
            if touched {
                changed.insert(*asset);
            }
        }
        changed
            .into_iter()
            .flat_map(|asset| self.find_rules(asset, true))
            .collect()
    }

    /// Forgets the rules of a map that closed.
    pub(crate) fn forget_rules(&mut self, asset: AssetId) {
        self.automap.maps.remove(&asset);
    }

    /// The `rules.txt` beside a map that has a file.
    fn rules_beside(&self, asset: AssetId) -> Option<PathBuf> {
        let map_path = self.store.get(asset)?.path()?;
        Some(map_path.parent()?.join(BESIDE_THE_MAP))
    }

    fn map_path(&self, asset: AssetId) -> Option<PathBuf> {
        self.store
            .get(asset)
            .and_then(|a| a.path())
            .map(Path::to_path_buf)
    }
}
