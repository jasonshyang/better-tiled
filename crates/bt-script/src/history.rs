//! The versions `bt undo` restores (`10-agents.md`, §12). Before `bt edit` writes a map, the
//! previous bytes are kept in `<history>/<stem>-<hash of the absolute path>/` as `N.before`, with
//! `N.after` holding a fingerprint of what the edit wrote and its label, and `N.entry` the map's
//! project-file record as it was (or `none`). Fifty versions are kept per map; `bt undo` refuses
//! when the file's fingerprint is not the one the last edit wrote (the file was saved since).

use std::path::{Path, PathBuf};

use bt_utils::atomic_write::{WriteMode, write_file};
use bt_utils::hash::fnv1a_64;
use serde::{Deserialize, Serialize};

use crate::report::{Line, Outcome, Report};

/// How many versions are kept per map.
pub const KEPT_PER_MAP: usize = 50;

/// What `N.entry` holds when the map had no project-file record.
const NO_ENTRY: &str = "none";

/// The versions kept of every map, in one folder (`<data>/history`).
#[derive(Clone, Debug)]
pub struct History {
    folder: PathBuf,
}

/// An edit to keep, before its result is written.
#[derive(Clone, Copy, Debug)]
pub struct EditToKeep<'a> {
    /// The file's bytes before the edit.
    pub before: &'a [u8],
    /// The bytes the edit writes.
    pub written: &'a [u8],
    /// The edit's label.
    pub label: &'a str,
    /// The map's project-file record before the edit, as JSON, or none.
    pub entry: Option<&'a str>,
}

/// An edit kept: what the file held before it, and what it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptEdit {
    /// Its number among the map's kept edits; later edits have higher numbers.
    pub number: u64,
    /// The edit's label.
    pub label: String,
    /// The fingerprint of the bytes the edit wrote.
    pub written: u64,
    /// The file's bytes before the edit.
    pub before: Vec<u8>,
    /// The map's project-file record before the edit, as JSON, or none.
    pub entry: Option<String>,
}

impl KeptEdit {
    /// Whether `bytes` are what this edit wrote.
    pub fn wrote(&self, bytes: &[u8]) -> bool {
        fingerprint(bytes) == self.written
    }
}

/// What `N.after` holds.
#[derive(Serialize, Deserialize)]
struct After {
    fingerprint: String,
    label: String,
}

/// Why the kept versions could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// A file of the history could not be read, written or removed.
    #[error("the bt undo history at {}: {source}", path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// A file of the history could not be written.
    #[error("the bt undo history: {0}")]
    Write(#[from] bt_utils::Error),
    /// A file of the history is not what bt wrote there.
    #[error("the bt undo history at {} is damaged", path.display())]
    Damaged {
        /// The file.
        path: PathBuf,
    },
}

/// The fingerprint `bt undo` compares a file with.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    fnv1a_64(bytes)
}

impl History {
    /// The history kept in `folder`.
    pub fn new(folder: PathBuf) -> History {
        History { folder }
    }

    /// The folder of the map at `map` (an absolute path): its stem and a hash of the path, so
    /// maps of one name in different folders are kept apart.
    fn folder_of(&self, map: &Path) -> PathBuf {
        let stem = map
            .file_stem()
            .map_or_else(|| "map".to_owned(), |s| s.to_string_lossy().into_owned());
        let hash = fnv1a_64(map.to_string_lossy().as_bytes());
        self.folder.join(format!("{stem}-{hash:016x}"))
    }

    /// The numbers of a map's kept edits, oldest first.
    fn numbers(&self, folder: &Path) -> Result<Vec<u64>, HistoryError> {
        let entries = match std::fs::read_dir(folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(HistoryError::Io {
                    path: folder.to_path_buf(),
                    source,
                });
            }
        };
        let mut numbers: Vec<u64> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".after")?.parse().ok()
            })
            .collect();
        numbers.sort_unstable();
        Ok(numbers)
    }

    /// Keeps the version of `map` before an edit, and forgets the oldest beyond
    /// [`KEPT_PER_MAP`]. Called before the edit's result is written.
    pub fn keep_edit(&self, map: &Path, edit: EditToKeep<'_>) -> Result<(), HistoryError> {
        let folder = self.folder_of(map);
        std::fs::create_dir_all(&folder).map_err(|source| HistoryError::Io {
            path: folder.clone(),
            source,
        })?;
        let numbers = self.numbers(&folder)?;
        let number = numbers.last().map_or(1, |last| last + 1);
        let after = After {
            fingerprint: format!("{:016x}", fingerprint(edit.written)),
            label: edit.label.to_owned(),
        };
        let after = serde_json::to_vec(&after).map_err(|_| HistoryError::Damaged {
            path: folder.clone(),
        })?;
        write_file(
            &folder.join(format!("{number}.before")),
            edit.before,
            WriteMode::Atomic,
        )?;
        write_file(
            &folder.join(format!("{number}.entry")),
            edit.entry.unwrap_or(NO_ENTRY).as_bytes(),
            WriteMode::Atomic,
        )?;
        // Written last: an edit counts as kept once its `.after` is there.
        write_file(
            &folder.join(format!("{number}.after")),
            &after,
            WriteMode::Atomic,
        )?;
        let excess = (numbers.len() + 1).saturating_sub(KEPT_PER_MAP);
        for old in numbers.iter().take(excess) {
            remove_edit(&folder, *old)?;
        }
        Ok(())
    }

    /// The last edit kept of `map`, if any.
    pub fn last_edit(&self, map: &Path) -> Result<Option<KeptEdit>, HistoryError> {
        let folder = self.folder_of(map);
        let Some(&number) = self.numbers(&folder)?.last() else {
            return Ok(None);
        };
        let read = |extension: &str| {
            let path = folder.join(format!("{number}.{extension}"));
            std::fs::read(&path).map_err(|source| HistoryError::Io { path, source })
        };
        let after_path = folder.join(format!("{number}.after"));
        let damaged = || HistoryError::Damaged {
            path: after_path.clone(),
        };
        let after: After = serde_json::from_slice(&read("after")?).map_err(|_| damaged())?;
        let written = u64::from_str_radix(&after.fingerprint, 16).map_err(|_| damaged())?;
        let entry = String::from_utf8(read("entry")?).map_err(|_| damaged())?;
        Ok(Some(KeptEdit {
            number,
            label: after.label,
            written,
            before: read("before")?,
            entry: (entry != NO_ENTRY).then_some(entry),
        }))
    }

    /// Forgets the last edit kept of `map` (it was taken back); how many earlier ones remain.
    pub fn forget_last_edit(&self, map: &Path) -> Result<usize, HistoryError> {
        let folder = self.folder_of(map);
        let mut numbers = self.numbers(&folder)?;
        if let Some(last) = numbers.pop() {
            remove_edit(&folder, last)?;
        }
        Ok(numbers.len())
    }

    /// How many edits of `map` are kept.
    pub fn kept_edits(&self, map: &Path) -> Result<usize, HistoryError> {
        Ok(self.numbers(&self.folder_of(map))?.len())
    }
}

/// Removes edit `number`'s files, its `.after` first, so a half-removed edit no longer counts.
fn remove_edit(folder: &Path, number: u64) -> Result<(), HistoryError> {
    for extension in ["after", "before", "entry"] {
        let path = folder.join(format!("{number}.{extension}"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(HistoryError::Io { path, source }),
        }
    }
    Ok(())
}

/// What `bt undo` did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoOutcome {
    /// The map.
    pub map: PathBuf,
    /// What happened.
    pub undone: Undone,
}

/// What `bt undo` did, or why not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Undone {
    /// The file got back what it held before the edit.
    File {
        /// The edit's label.
        label: String,
        /// How many earlier edits can still be taken back.
        earlier: usize,
        /// Whether the file had been saved since the edit (`--force`).
        forced: bool,
    },
    /// The editor took the edit back, as one of its own undo steps; its words.
    InEditor {
        /// What the editor said.
        message: String,
    },
    /// Refused: the map has unsaved changes in the editor, and taking an earlier edit of its
    /// file back would write over them.
    UnsavedInEditor,
    /// Refused: the file was saved since the edit, and the edit would take that away.
    SavedSince {
        /// The edit's label.
        label: String,
    },
    /// Nothing to take back.
    Nothing,
}

impl Outcome for UndoOutcome {
    const SCHEMA: &'static str = "bt.undo/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        let map = bt_utils::paths::display_name(&self.map);
        let earlier = |n: usize| match n {
            0 => "no earlier bt edit is left to take back".to_owned(),
            1 => "1 earlier bt edit can be taken back".to_owned(),
            n => format!("{n} earlier bt edits can be taken back"),
        };
        match &self.undone {
            Undone::File {
                label,
                earlier: n,
                forced,
            } => {
                if *forced {
                    report.add_line(Line::note(format!(
                        "{map} was saved since “{label}”: what was saved since is gone too"
                    )));
                }
                report.add_line(Line::fact(format!(
                    "took back “{label}” in {map}; {}",
                    earlier(*n)
                )));
            }
            Undone::InEditor { message } => report.add_line(Line::fact(message.clone())),
            Undone::UnsavedInEditor => report.add_line(Line::problem(format!(
                "{map} has unsaved changes in the editor: taking back a bt edit of its file \
                 would write over them; save or undo them there first"
            ))),
            Undone::SavedSince { label } => report.add_line(Line::problem(format!(
                "{map} was saved since “{label}”: bt undo would take away what was saved since; \
                 --force takes the edit back anyway"
            ))),
            Undone::Nothing => {
                report.add_line(Line::fact(format!("no bt edit of {map} to take back")));
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::Scratch;

    #[test]
    fn edits_are_kept_and_taken_back_last_first() {
        let scratch = Scratch::new("history-keep").unwrap();
        let history = History::new(scratch.join("history"));
        let map = scratch.join("a.tmx");
        assert_eq!(history.last_edit(&map).unwrap(), None);
        for (number, label) in ["one", "two"].into_iter().enumerate() {
            let before = format!("v{number}");
            let written = format!("v{}", number + 1);
            history
                .keep_edit(
                    &map,
                    EditToKeep {
                        before: before.as_bytes(),
                        written: written.as_bytes(),
                        label,
                        entry: (label == "two").then_some("{\"x\":1}"),
                    },
                )
                .unwrap();
        }
        let last = history.last_edit(&map).unwrap().unwrap();
        assert_eq!(last.label, "two");
        assert_eq!(last.before, b"v1");
        assert!(last.wrote(b"v2") && !last.wrote(b"v3"));
        assert_eq!(last.entry.as_deref(), Some("{\"x\":1}"));
        assert_eq!(history.forget_last_edit(&map).unwrap(), 1);
        let first = history.last_edit(&map).unwrap().unwrap();
        assert_eq!((first.label.as_str(), first.entry), ("one", None));
        assert_eq!(history.forget_last_edit(&map).unwrap(), 0);
        assert_eq!(history.last_edit(&map).unwrap(), None);
    }

    #[test]
    fn fifty_versions_are_kept_per_map_and_maps_of_one_name_apart() {
        let scratch = Scratch::new("history-limit").unwrap();
        let history = History::new(scratch.join("history"));
        let map = scratch.join("a/map.tmx");
        let other = scratch.join("b/map.tmx");
        for n in 0..(KEPT_PER_MAP + 3) {
            let label = format!("edit {n}");
            history
                .keep_edit(
                    &map,
                    EditToKeep {
                        before: b"x",
                        written: b"y",
                        label: &label,
                        entry: None,
                    },
                )
                .unwrap();
        }
        assert_eq!(history.kept_edits(&map).unwrap(), KEPT_PER_MAP);
        assert_eq!(
            history.last_edit(&map).unwrap().unwrap().label,
            format!("edit {}", KEPT_PER_MAP + 2)
        );
        assert_eq!(history.kept_edits(&other).unwrap(), 0);
    }

    #[test]
    fn a_damaged_history_says_so() {
        let scratch = Scratch::new("history-damaged").unwrap();
        let history = History::new(scratch.join("history"));
        let map = scratch.join("a.tmx");
        history
            .keep_edit(
                &map,
                EditToKeep {
                    before: b"x",
                    written: b"y",
                    label: "one",
                    entry: None,
                },
            )
            .unwrap();
        let folder = history.folder_of(&map);
        std::fs::write(folder.join("1.after"), "not json").unwrap();
        assert!(matches!(
            history.last_edit(&map),
            Err(HistoryError::Damaged { .. })
        ));
    }
}
