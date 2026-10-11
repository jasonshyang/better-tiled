//! `bt`'s side of the live link (`10-agents.md`, §13; `11-live-link.md`, §7): the editors that
//! answered, the map a command names as an editor has it, and the refusal to touch a file an
//! editor that does not answer may hold unsaved changes to.

use std::path::{Path, PathBuf};

use anyhow::bail;
use bt_live::{DocStatus, Editor, Problem, Request, Response, canonical_path};
use serde::Deserialize;

/// The running editors `bt` found.
pub(crate) struct Link {
    /// The editors that answered: the focused one first, then the newest process.
    pub(crate) editors: Vec<Editor>,
    /// Why some did not answer, or why none could be looked for.
    pub(crate) problems: Vec<String>,
    /// Editors that did not answer, by process id.
    silent: Vec<u32>,
}

/// A map open in an editor.
pub(crate) struct OpenMap<'l> {
    /// The editor.
    pub(crate) editor: &'l Editor,
    /// The map's document there.
    pub(crate) document: &'l DocStatus,
}

/// What a recovery snapshot's description says.
#[derive(Deserialize)]
struct SnapshotMeta {
    original: Option<PathBuf>,
    pid: u32,
}

impl Link {
    /// No editors: `bt` works with files only.
    pub(crate) fn none() -> Link {
        Link {
            editors: Vec::new(),
            problems: Vec::new(),
            silent: Vec::new(),
        }
    }

    /// The editors listening for this user. Windows has no live link.
    pub(crate) fn find() -> Link {
        if !cfg!(unix) {
            return Link::none();
        }
        let folder = match bt_live::live_dir() {
            Ok(folder) => folder,
            Err(error) => {
                return Link {
                    problems: vec![format!("bt cannot look for running editors: {error}")],
                    ..Link::none()
                };
            }
        };
        match bt_live::editors(&folder) {
            Ok(found) => Link {
                silent: found
                    .problems
                    .iter()
                    .filter(|problem| matches!(problem, Problem::NoAnswer { .. }))
                    .map(Problem::pid)
                    .collect(),
                problems: found.problems.iter().map(ToString::to_string).collect(),
                editors: found.answered,
            },
            Err(error) => Link {
                problems: vec![format!("bt cannot ask the running editors: {error}")],
                ..Link::none()
            },
        }
    }

    /// The map at `path` as an editor has it open, if one does.
    pub(crate) fn open_map(&self, path: &Path) -> Option<OpenMap<'_>> {
        let path = canonical_path(path);
        self.editors.iter().find_map(|editor| {
            editor
                .document(&path)
                .map(|document| OpenMap { editor, document })
        })
    }

    /// Refuses when an editor that did not answer may hold unsaved changes to the map at
    /// `path`: its recovery snapshot of the map says so (`recovery` is the editor's recovery
    /// folder).
    pub(crate) fn refuse_unanswered(
        &self,
        path: &Path,
        recovery: Option<&Path>,
    ) -> anyhow::Result<()> {
        let Some(recovery) = recovery else {
            return Ok(());
        };
        if self.silent.is_empty() {
            return Ok(());
        }
        let path = canonical_path(path);
        let Ok(entries) = std::fs::read_dir(recovery) else {
            return Ok(());
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".meta.json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<SnapshotMeta>(&text) else {
                continue;
            };
            let names_map = meta
                .original
                .as_deref()
                .is_some_and(|original| canonical_path(original) == path);
            if names_map && self.silent.contains(&meta.pid) {
                bail!(
                    "the editor (process {}) may hold unsaved changes to {} and didn't answer \
                     in time: is a dialog open in it? bt leaves the file alone until it answers",
                    meta.pid,
                    path.display()
                );
            }
        }
        Ok(())
    }
}

impl OpenMap<'_> {
    /// Asks the map's editor.
    pub(crate) fn ask(&self, request: &Request) -> anyhow::Result<Response> {
        Ok(self.editor.ask(request)?)
    }
}
