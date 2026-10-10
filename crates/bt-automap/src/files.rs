//! The one part of the crate that touches files, as Tiled's `AutomappingManager` does: finding
//! the rules for a map, reading a `rules.txt` (or a single rule map) and the rule maps it lists
//! through a `bt-tmx` loader, so their tilesets are the map's own values, reloading them when
//! they change, keeping a broken rule map's previous rules until it is fixed, and saying which
//! files to watch for that. Unlike Tiled, a failed load never leaves entries behind to be
//! listed twice on the next, and an include cycle is skipped, with a note, rather than followed
//! for ever.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{CustomTypes, Map, Project};
use bt_tmx::{IncludeCycle, Loader, RulesFile, parse_rules_file, read_rules_file};

use crate::problem::{Problem, ProblemKind};
use crate::rulemap::resolve_match_type;
use crate::rules::RuleSet;

/// A failure to load rules: a rules file that cannot be read, or a rule map it lists that does
/// not exist (both stop Tiled from running any rule). A rule map that exists but cannot be read
/// is not an error: the rule set says so with a problem and goes on without it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The rules file (or one it includes) cannot be read.
    #[error("the rules file {} cannot be read: {source}", path.display())]
    RulesFile {
        /// The rules file.
        path: PathBuf,
        /// Why.
        #[source]
        source: bt_tmx::Error,
    },
    /// A rule map the rules file (or one it includes) lists does not exist.
    #[error(
        "the rule map {} does not exist (listed through {})",
        path.display(),
        rules_file.display()
    )]
    MissingRuleMap {
        /// The rule map.
        path: PathBuf,
        /// The rules file the run loaded.
        rules_file: PathBuf,
    },
}

/// The rules file for a map, as Tiled finds it: `chosen` (the AutoMap setup's own) if given,
/// else a `rules.txt` beside the map, else the project's rules file. `None` when there is none.
pub fn find_rules(
    map_path: Option<&Path>,
    chosen: Option<&Path>,
    project: Option<&Project>,
) -> Option<PathBuf> {
    if let Some(chosen) = chosen {
        return Some(chosen.to_path_buf());
    }
    let beside = map_path
        .and_then(Path::parent)
        .map(|dir| dir.join("rules.txt"))
        .filter(|path| path.is_file());
    beside.or_else(|| {
        project
            .and_then(|p| p.automapping_rules_file.as_ref())
            .map(|file| file.path.clone())
    })
}

/// Whether `path` names a rules file rather than a rule map.
fn is_rules_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
}

/// What a rules file lists for a map, with the files read on the way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Listed {
    /// The rule maps that apply to the map, in order.
    rule_maps: Vec<PathBuf>,
    /// The files whose change changes the list: the lists read and every rule map they list,
    /// whichever maps it applies to.
    files: Vec<PathBuf>,
    /// The includes skipped because they name a list being read.
    cycles: Vec<IncludeCycle>,
}

/// The rule maps `rules_path` lists for a map named `map_file_name`, in order, after checking
/// that every rule map it lists exists (a missing one stops Tiled from running any).
fn listed_rule_maps(rules_path: &Path, map_file_name: &str) -> Result<Listed, Error> {
    if !is_rules_file(rules_path) {
        if !rules_path.exists() {
            return Err(Error::MissingRuleMap {
                path: rules_path.to_path_buf(),
                rules_file: rules_path.to_path_buf(),
            });
        }
        return Ok(Listed {
            rule_maps: vec![rules_path.to_path_buf()],
            files: vec![rules_path.to_path_buf()],
            cycles: Vec::new(),
        });
    }
    let read = parse_rules_file(rules_path).map_err(|source| Error::RulesFile {
        path: rules_path.to_path_buf(),
        source,
    })?;
    if let Some(missing) = read.entries.iter().find(|e| !e.path.exists()) {
        return Err(Error::MissingRuleMap {
            path: missing.path.clone(),
            rules_file: rules_path.to_path_buf(),
        });
    }
    let mut files = read.lists;
    files.extend(read.entries.iter().map(|e| e.path.clone()));
    Ok(Listed {
        rule_maps: read
            .entries
            .into_iter()
            .filter(|e| e.applies_to(map_file_name))
            .map(|e| e.path)
            .collect(),
        files,
        cycles: read.cycles,
    })
}

/// The files a load of `rules_path` reads before its rule maps, as far as they can be read: the
/// rules files (nested ones, and one that cannot be read, included) and every rule map they
/// list. After a failed [`load`], what to watch for the rules to load again; after a load,
/// [`RuleSet::watched_files`] names every file.
pub fn listed_files(rules_path: &Path) -> Vec<PathBuf> {
    if !is_rules_file(rules_path) {
        return vec![rules_path.to_path_buf()];
    }
    let mut read = RulesFile::default();
    // The load says why a list cannot be read; here only the files read on the way matter.
    let _unreadable = read_rules_file(rules_path, &mut read).err();
    let mut files = read.lists;
    if files.is_empty() {
        files.push(rules_path.to_path_buf());
    }
    files.extend(read.entries.into_iter().map(|e| e.path));
    files
}

/// An include cycle as a problem: a note naming the files.
fn cycle_problem(cycle: &IncludeCycle) -> Problem {
    Problem::new(
        ProblemKind::IncludeCycle,
        format!(
            "the rules file {} includes {}, which is already being read, so the include is \
             skipped",
            cycle.list.display(),
            cycle.included.display()
        ),
    )
}

/// A rule map read, or why it could not be.
#[derive(Clone, Debug)]
enum ReadRuleMap {
    /// The rule map, with the problems found reading it.
    Read {
        map: Arc<Map>,
        problems: Vec<Problem>,
    },
    /// It cannot be read and there are no previous rules to keep.
    Unreadable { problem: Problem },
}

/// Reads the rule map at `path`, or keeps its previous rules from `previous`.
fn read_rule_map(loader: &mut Loader, path: &Path, previous: Option<&RuleSet>) -> ReadRuleMap {
    let error = match loader.load_map(path) {
        Ok(loaded) => {
            return ReadRuleMap::Read {
                map: Arc::new(loaded.value),
                problems: loaded
                    .warnings
                    .iter()
                    .map(|w| Problem::new(ProblemKind::FileWarning, w.to_string()))
                    .collect(),
            };
        }
        Err(error) => error,
    };
    let kept = previous.and_then(|p| {
        p.rule_maps()
            .iter()
            .find(|r| r.path() == path)
            .map(|r| Arc::clone(r.map()))
    });
    match kept {
        Some(map) => ReadRuleMap::Read {
            map,
            problems: vec![Problem::new(
                ProblemKind::UnreadableRuleMap,
                format!(
                    "the rule map {} cannot be read ({error}); its previous rules are kept until \
                     it is fixed",
                    path.display()
                ),
            )],
        },
        None => ReadRuleMap::Unreadable {
            problem: Problem::new(
                ProblemKind::UnreadableRuleMap,
                format!(
                    "the rule map {} cannot be read ({error}), so it is left out",
                    path.display()
                ),
            ),
        },
    }
}

/// Loads the rules `rules_path` (a `rules.txt` or a single rule map) holds for the map at
/// `map_path` (`None` for a map not saved yet), reading the rule maps through `loader`.
pub fn load(
    loader: &mut Loader,
    rules_path: &Path,
    map_path: Option<&Path>,
) -> Result<RuleSet, Error> {
    load_keeping(loader, rules_path, map_path, None)
}

/// [`load`] again after some files changed: a rule map that cannot be read now keeps the rules
/// it has in `previous`, with a problem saying so, until it is fixed.
pub fn reload(
    loader: &mut Loader,
    rules_path: &Path,
    map_path: Option<&Path>,
    previous: &RuleSet,
) -> Result<RuleSet, Error> {
    load_keeping(loader, rules_path, map_path, Some(previous))
}

fn load_keeping(
    loader: &mut Loader,
    rules_path: &Path,
    map_path: Option<&Path>,
    previous: Option<&RuleSet>,
) -> Result<RuleSet, Error> {
    let map_file_name = map_path
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let listed = listed_rule_maps(rules_path, &map_file_name)?;
    let mut builder = RuleSet::builder()
        .custom_types(Arc::clone(loader.custom_types()))
        .rules_file(rules_path.to_path_buf())
        .listed_files(listed.files);
    for cycle in &listed.cycles {
        builder = builder.problem(cycle_problem(cycle));
    }
    let mut read: HashMap<PathBuf, ReadRuleMap> = HashMap::new();
    for path in listed.rule_maps {
        let seen = read.contains_key(&path);
        let outcome = read
            .entry(path.clone())
            .or_insert_with(|| read_rule_map(loader, &path, previous))
            .clone();
        builder = match outcome {
            ReadRuleMap::Read { map, problems } => {
                builder.rule_map_with_problems(path, map, problems)
            }
            ReadRuleMap::Unreadable { problem } if !seen => builder.problem(problem),
            ReadRuleMap::Unreadable { .. } => builder,
        };
    }
    Ok(builder.build())
}

impl RuleSet {
    /// Every file whose change must reload the rules: the rules files read, nested ones
    /// included; every rule map they list, whichever maps it applies to and whether it could be
    /// read (so fixing one reloads); the external tilesets the rule maps use; and the
    /// templates their objects come from. Sorted, each once.
    pub fn watched_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.listed_files().to_vec();
        files.extend(self.tileset_files());
        for rule_map in self.rule_maps() {
            files.push(rule_map.path().to_path_buf());
            rule_map.map().for_each_layer(|layer, _| {
                if let Some(objects) = layer.as_object_layer() {
                    files.extend(
                        objects
                            .objects
                            .iter()
                            .filter_map(|o| o.template.as_ref())
                            .map(|t| t.path.clone()),
                    );
                }
            });
        }
        files.retain(|path| path.is_absolute());
        files.sort();
        files.dedup();
        files
    }

    /// The external tileset files the rule maps use: what to watch for the rules to change.
    pub fn tileset_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self
            .table
            .entries()
            .iter()
            .filter_map(|e| e.reference.file.as_ref())
            .map(|f| f.path.clone())
            .filter(|path| path.is_absolute())
            .collect();
        files.sort();
        files.dedup();
        files
    }

    /// Whether `types` would give some tile the rules use another `MatchType` than it has now,
    /// through its class: then the rules must be loaded again.
    pub fn classes_changed(&self, types: &CustomTypes) -> bool {
        self.match_types.entries.iter().any(|(key, was)| {
            was.class.is_some()
                && self.table.get(key.tileset).is_some_and(|entry| {
                    let now = resolve_match_type(&entry.reference.tileset, key.tile, types);
                    now.match_type != was.match_type || now.unknown_name != was.unknown_name
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use bt_core::FileRef;
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn rules_are_the_setups_then_beside_the_map_then_the_projects() {
        let scratch = Scratch::new("bt-automap-find-rules").unwrap();
        let map = scratch.join("maps/town.tmx");
        std::fs::create_dir_all(scratch.join("maps")).unwrap();
        let project = Project {
            automapping_rules_file: Some(FileRef::new(
                "rules/all.txt",
                scratch.join("rules/all.txt"),
            )),
            ..Project::default()
        };
        assert_eq!(
            find_rules(Some(&map), None, Some(&project)),
            Some(scratch.join("rules/all.txt"))
        );
        assert_eq!(find_rules(Some(&map), None, None), None);
        std::fs::write(scratch.join("maps/rules.txt"), "").unwrap();
        assert_eq!(
            find_rules(Some(&map), None, Some(&project)),
            Some(scratch.join("maps/rules.txt"))
        );
        let chosen = scratch.join("chosen.txt");
        assert_eq!(
            find_rules(Some(&map), Some(&chosen), Some(&project)),
            Some(chosen)
        );
        assert_eq!(
            find_rules(None, None, Some(&project)),
            Some(scratch.join("rules/all.txt")),
            "a map not saved yet takes the project's"
        );
    }

    #[test]
    fn lists_are_filtered_by_the_maps_name_and_every_rule_map_must_exist() {
        let scratch = Scratch::new("bt-automap-listed").unwrap();
        for name in ["a.tmx", "town.tmx", "cave.tmx"] {
            std::fs::write(scratch.join(name), "").unwrap();
        }
        std::fs::write(
            scratch.join("rules.txt"),
            "a.tmx\n[town*]\ntown.tmx\n[cave*]\ncave.tmx\n[*]\na.tmx\n",
        )
        .unwrap();
        let names = |map: &str| -> Vec<String> {
            listed_rule_maps(&scratch.join("rules.txt"), map)
                .unwrap()
                .rule_maps
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names("Town2.tmx"), ["a.tmx", "town.tmx", "a.tmx"]);
        assert_eq!(names("cave.tmx"), ["a.tmx", "cave.tmx", "a.tmx"]);
        assert_eq!(names(""), ["a.tmx", "a.tmx"], "a map not saved yet");
        // The files to watch are the list and every rule map it lists, for any map.
        let listed = listed_rule_maps(&scratch.join("rules.txt"), "Town2.tmx").unwrap();
        let mut files = listed.files;
        files.sort();
        files.dedup();
        let mut expected: Vec<PathBuf> = ["a.tmx", "cave.tmx", "rules.txt", "town.tmx"]
            .iter()
            .map(|name| scratch.join(name))
            .collect();
        expected.sort();
        assert_eq!(files, expected);
        // A rule map that is missing, even for other maps, stops the load as it stops Tiled.
        std::fs::remove_file(scratch.join("cave.tmx")).unwrap();
        assert!(matches!(
            listed_rule_maps(&scratch.join("rules.txt"), "town.tmx"),
            Err(Error::MissingRuleMap { .. })
        ));
        assert!(matches!(
            listed_rule_maps(&scratch.join("missing.txt"), "town.tmx"),
            Err(Error::RulesFile { .. })
        ));
        // A rule map named directly is its own list.
        let alone = listed_rule_maps(&scratch.join("a.tmx"), "town.tmx").unwrap();
        assert_eq!(alone.rule_maps, [scratch.join("a.tmx")]);
        assert_eq!(alone.files, [scratch.join("a.tmx")]);
    }
}
