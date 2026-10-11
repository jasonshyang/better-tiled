//! What travels over the live link (`11-live-link.md`, §5): the requests `bt` sends, the answers
//! the editor gives, the version every `Status` carries, cells as runs, and how both sides name
//! a map.
//!
//! Every type is written as JSON with `serde`, and refuses fields it does not know: a request or
//! an answer of another version is an error, never half understood. The one exception is the
//! `status` request, which every version answers whatever else it carries, so that a client of
//! any version learns the editor's.

mod request;
mod response;
mod run;
mod status;

use std::path::{Path, PathBuf};

pub use request::{EditRequest, MARK_PALETTE, MarkSpec, Request};
pub use response::{Done, Edited, MapText, Response, TilesetText};
pub use run::{Run, cells_from_runs, runs_from_cells};
pub use status::{DocStatus, KeyboardFocus, Status};

/// The protocol's version, carried by every [`Status`]. It changes when a request or an answer
/// changes incompatibly; a client that meets another version says so and asks nothing else.
/// `{"op": "status"}` and the place of `protocol` in its answer never change, so every version
/// can tell another apart.
pub const PROTOCOL: u32 = 1;

/// How both sides name a map: its canonical path, with links resolved, so `/tmp/a.tmx` and
/// `/private/tmp/a.tmx` are one map. A path that does not lead to a file (a map whose file was
/// deleted, a folder not made yet) is made absolute and cleaned, and the deepest folder of it
/// that exists is resolved, so it is named the same way whichever link it is given through.
pub fn canonical_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let Ok(absolute) = bt_utils::paths::absolute_path(path) else {
        return bt_utils::paths::clean_path(path);
    };
    for existing in absolute.ancestors() {
        let Ok(resolved) = std::fs::canonicalize(existing) else {
            continue;
        };
        let Ok(rest) = absolute.strip_prefix(existing) else {
            continue;
        };
        return if rest.as_os_str().is_empty() {
            resolved
        } else {
            resolved.join(rest)
        };
    }
    absolute
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{every_answer, every_request};

    #[test]
    fn every_request_and_answer_round_trips() {
        for request in every_request() {
            let line = serde_json::to_string(&request).unwrap();
            assert!(!line.contains('\n'), "one line: {line}");
            assert_eq!(
                serde_json::from_str::<Request>(&line).unwrap(),
                request,
                "{line}"
            );
        }
        for answer in every_answer() {
            let line = serde_json::to_string(&answer).unwrap();
            assert!(!line.contains('\n'), "one line: {line}");
            assert_eq!(
                serde_json::from_str::<Response>(&line).unwrap(),
                answer,
                "{line}"
            );
        }
    }

    #[test]
    fn the_samples_hold_every_kind() {
        // A kind added to the protocol fails to compile here until it has a sample.
        let kinds: Vec<usize> = every_request()
            .iter()
            .map(|request| match request {
                Request::Status => 0,
                Request::Snapshot { .. } => 1,
                Request::Edit(_) => 2,
                Request::Undo { .. } => 3,
                Request::Mark { .. } => 4,
                Request::Open { .. } => 5,
                Request::Save { .. } => 6,
                Request::Screenshot { .. } => 7,
            })
            .collect();
        assert_eq!(kinds, (0..8).collect::<Vec<_>>());
        let kinds: Vec<usize> = every_answer()
            .iter()
            .map(|answer| match answer {
                Response::Status(_) => 0,
                Response::MapText(_) => 1,
                Response::Edited(_) => 2,
                Response::Done(_) => 3,
            })
            .collect();
        assert_eq!(kinds, (0..4).collect::<Vec<_>>());
    }

    #[cfg(unix)]
    #[test]
    fn a_map_has_one_name_whichever_link_leads_to_it() {
        let folder = crate::testing::TestFolder::new();
        let real = folder.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("a.tmx"), "").unwrap();
        let link = folder.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = canonical_path(&real.join("a.tmx"));
        assert_eq!(
            canonical,
            std::fs::canonicalize(real.join("a.tmx")).unwrap()
        );
        assert_eq!(canonical_path(&link.join("a.tmx")), canonical);
        assert_eq!(
            canonical_path(&link.join("./x/../a.tmx")),
            canonical,
            "through a folder that does not exist"
        );
        assert_eq!(canonical_path(&link), std::fs::canonicalize(&real).unwrap());

        // A file that does not exist (deleted while open, say) is named through its folder.
        let gone = canonical_path(&link.join("gone/b.tmx"));
        assert_eq!(
            gone,
            std::fs::canonicalize(&real).unwrap().join("gone/b.tmx")
        );
        assert_eq!(canonical_path(&real.join("gone/./b.tmx")), gone);
    }

    #[test]
    fn a_missing_file_is_named_by_its_absolute_path() {
        let name = canonical_path(Path::new("no-such-folder/./maps/../a.tmx"));
        assert!(name.is_absolute(), "{}", name.display());
        assert!(name.ends_with("no-such-folder/a.tmx"), "{}", name.display());
        #[cfg(unix)]
        assert_eq!(
            canonical_path(&name),
            name,
            "a name is its own name: {}",
            name.display()
        );
    }
}
