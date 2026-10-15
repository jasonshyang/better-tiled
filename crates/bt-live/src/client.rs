//! `bt`'s side (`11-live-link.md`, §7): finding the editors listening in the folder, and asking
//! one of them.
//!
//! [`editors`] asks every socket in the folder for its `Status`: the editors that answer in
//! time and speak this protocol are the ones to work with, the focused one first; a socket whose
//! process has exited is removed as stale; an editor of another version, or one that does not
//! answer, is a [`Problem`] naming its process. [`Editor::ask`] sends one request and waits as
//! long as the request may take.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

use crate::error::Error;
use crate::protocol::{DocStatus, PROTOCOL, Request, Response, Status, canonical_path};

/// How long [`editors`] waits for each editor's status.
#[cfg(unix)]
const STATUS_WAIT: Duration = Duration::from_secs(8);

/// How long [`Editor::ask`] waits for most answers.
#[cfg(unix)]
const ANSWER_WAIT: Duration = Duration::from_secs(15);

/// How long [`Editor::ask`] waits for a save.
#[cfg(unix)]
const SAVE_WAIT: Duration = Duration::from_secs(60);

/// How long [`Editor::ask`] waits for an edit: a script may run AutoMap rules over a large map.
#[cfg(unix)]
const EDIT_WAIT: Duration = Duration::from_secs(600);

/// What [`editors`] found in the folder.
#[derive(Clone, Debug, Default)]
pub struct Editors {
    /// The editors that answered and speak this protocol: the one whose window has the keyboard
    /// focus first, then the newest process (the highest process id).
    pub answered: Vec<Editor>,
    /// The editors that are running but cannot be worked with, by process id.
    pub problems: Vec<Problem>,
}

/// A running editor that answered.
#[derive(Clone, Debug)]
pub struct Editor {
    socket: PathBuf,
    status: Status,
}

/// A running editor that cannot be worked with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// It speaks another version of the protocol: it was started from another build.
    OtherVersion {
        /// Its process id.
        pid: u32,
        /// The version it speaks.
        protocol: u32,
    },
    /// It did not answer in time: a dialog may be open in it.
    NoAnswer {
        /// Its process id.
        pid: u32,
    },
    /// It could not be asked, or its answer could not be used.
    Failed {
        /// Its process id.
        pid: u32,
        /// Why, in a sentence.
        reason: String,
    },
}

impl Problem {
    /// The process id of the editor with the problem.
    pub fn pid(&self) -> u32 {
        match self {
            Problem::OtherVersion { pid, .. }
            | Problem::NoAnswer { pid }
            | Problem::Failed { pid, .. } => *pid,
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::OtherVersion { pid, protocol } => write!(
                f,
                "better-tiled (pid {pid}) is another version (protocol {protocol}, not {PROTOCOL}): restart it"
            ),
            Problem::NoAnswer { pid } => write!(
                f,
                "better-tiled (pid {pid}) didn't answer in time: is a dialog open in it?"
            ),
            Problem::Failed { pid, reason } => write!(f, "better-tiled (pid {pid}): {reason}"),
        }
    }
}

impl Editors {
    /// The first editor, in the order of [`Editors::answered`], that has the map at `path` open.
    pub fn with_map_open(&self, path: &Path) -> Option<&Editor> {
        let wanted = canonical_path(path);
        self.answered
            .iter()
            .find(|editor| editor.document_named(&wanted).is_some())
    }
}

impl Editor {
    /// What the editor said about itself when it was found.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The editor's process id.
    pub fn pid(&self) -> u32 {
        self.status.pid
    }

    /// The socket the editor listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The map at `path` among the editor's documents, found by canonical path, so
    /// `/tmp/a.tmx` and `/private/tmp/a.tmx` are one map. Tilesets opened on their own are not
    /// maps.
    pub fn document(&self, path: &Path) -> Option<&DocStatus> {
        self.document_named(&canonical_path(path))
    }

    /// The map whose canonical path is `canonical`.
    fn document_named(&self, canonical: &Path) -> Option<&DocStatus> {
        self.status
            .documents
            .iter()
            .find(|doc| !doc.tileset && doc.path.as_deref() == Some(canonical))
    }

    /// Sends `request` and waits for the answer as long as the request may take: fifteen
    /// seconds for most, a minute for a save, ten minutes for an edit. The editor's own refusal
    /// is [`Error::Failed`] with its sentence; [`Response::into_done`] and its siblings take out
    /// the answer the request asks for.
    pub fn ask(&self, request: &Request) -> Result<Response, Error> {
        #[cfg(unix)]
        {
            let value = crate::wire::ask(&self.socket, request, answer_wait(request))?;
            serde_json::from_value(value).map_err(|error| Error::Garbled {
                reason: error.to_string(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err(Error::Unsupported)
        }
    }
}

/// The editors listening in `folder` (from [`live_dir`](crate::live_dir)) that answer their
/// `Status` within eight seconds, and the problems with the others. The folder is checked
/// first, as the editor checks it (made `0700` when missing, refused when it is not the user's
/// alone). Sockets whose process has exited are removed. On Windows there are none.
pub fn editors(folder: &Path) -> Result<Editors, Error> {
    #[cfg(unix)]
    {
        editors_within(folder, STATUS_WAIT)
    }
    #[cfg(not(unix))]
    {
        let _ = folder;
        Ok(Editors::default())
    }
}

/// How long to wait for the answer to `request`.
#[cfg(unix)]
fn answer_wait(request: &Request) -> Duration {
    match request {
        Request::RememberExport { .. } | Request::Edit(_) => EDIT_WAIT,
        Request::Save { .. } => SAVE_WAIT,
        Request::Status
        | Request::Snapshot { .. }
        | Request::Undo { .. }
        | Request::Mark { .. }
        | Request::Open { .. }
        | Request::Screenshot { .. } => ANSWER_WAIT,
    }
}

/// [`editors`], waiting up to `wait` for each status.
#[cfg(unix)]
pub(crate) fn editors_within(folder: &Path, wait: Duration) -> Result<Editors, Error> {
    use std::os::unix::fs::FileTypeExt;

    bt_utils::paths::make_private_dir(folder)?;
    let listing = |source: std::io::Error| {
        Error::from(bt_utils::Error::Folder {
            path: folder.to_path_buf(),
            source,
        })
    };
    let mut found = Editors::default();
    for entry in std::fs::read_dir(folder).map_err(listing)? {
        let entry = entry.map_err(listing)?;
        let socket = entry.path();
        let Some(pid) = crate::dir::socket_pid(&socket) else {
            continue;
        };
        if !entry.file_type().map_err(listing)?.is_socket() {
            continue;
        }
        match ask_status(&socket, wait) {
            Ok(StatusAnswer::Current(status)) => found.answered.push(Editor { socket, status }),
            Ok(StatusAnswer::Other { protocol }) => {
                found.problems.push(Problem::OtherVersion { pid, protocol });
            }
            Err(Error::NoAnswer { .. }) => found.problems.push(Problem::NoAnswer { pid }),
            Err(Error::Connection { .. }) if !crate::dir::process_alive(pid) => {
                // Left behind by an editor that crashed: no one will ever answer on it.
                if let Err(source) = std::fs::remove_file(&socket)
                    && source.kind() != std::io::ErrorKind::NotFound
                {
                    found.problems.push(Problem::Failed {
                        pid,
                        reason: format!(
                            "its process has exited, and its socket {} couldn't be removed: {source}",
                            socket.display()
                        ),
                    });
                }
            }
            Err(error) => found.problems.push(Problem::Failed {
                pid,
                reason: error.to_string(),
            }),
        }
    }
    found.answered.sort_by(|a, b| {
        b.status
            .focused
            .cmp(&a.status.focused)
            .then(b.pid().cmp(&a.pid()))
    });
    found.problems.sort_by_key(Problem::pid);
    Ok(found)
}

/// What an editor said its status was.
#[cfg(unix)]
enum StatusAnswer {
    /// The status of an editor of this protocol.
    Current(Status),
    /// An editor of another version, whose status is not read further.
    Other {
        /// The version it speaks.
        protocol: u32,
    },
}

/// Asks the editor on `socket` for its status, reading its protocol version before anything
/// else.
#[cfg(unix)]
fn ask_status(socket: &Path, wait: Duration) -> Result<StatusAnswer, Error> {
    let value = crate::wire::ask(socket, &Request::Status, wait)?;
    let protocol = value
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .and_then(|protocol| u32::try_from(protocol).ok())
        .ok_or_else(|| Error::Garbled {
            reason: "its status has no protocol version".to_owned(),
        })?;
    if protocol != PROTOCOL {
        return Ok(StatusAnswer::Other { protocol });
    }
    let response: Response = serde_json::from_value(value).map_err(|error| Error::Garbled {
        reason: error.to_string(),
    })?;
    Ok(StatusAnswer::Current(response.into_status()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_name_the_process() {
        let other = Problem::OtherVersion {
            pid: 4242,
            protocol: 9,
        };
        assert_eq!(
            other.to_string(),
            format!(
                "better-tiled (pid 4242) is another version (protocol 9, not {PROTOCOL}): restart it"
            )
        );
        assert_eq!(
            Problem::NoAnswer { pid: 7 }.to_string(),
            "better-tiled (pid 7) didn't answer in time: is a dialog open in it?"
        );
        let failed = Problem::Failed {
            pid: 8,
            reason: "couldn't talk to the editor".to_owned(),
        };
        assert_eq!(
            failed.to_string(),
            "better-tiled (pid 8): couldn't talk to the editor"
        );
        assert_eq!([other.pid(), failed.pid()], [4242, 8]);
    }

    #[cfg(not(unix))]
    #[test]
    fn there_are_no_editors_without_unix_sockets() {
        let found = editors(Path::new("C:\\nowhere")).unwrap();
        assert!(found.answered.is_empty() && found.problems.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn each_request_waits_as_long_as_it_may_take() {
        for request in crate::testing::every_request() {
            let expected = match &request {
                Request::RememberExport { .. } | Request::Edit(_) => Duration::from_secs(600),
                Request::Save { .. } => Duration::from_secs(60),
                _ => Duration::from_secs(15),
            };
            assert_eq!(answer_wait(&request), expected, "{request:?}");
        }
        assert_eq!(STATUS_WAIT, Duration::from_secs(8));
    }
}

#[cfg(test)]
#[cfg(unix)]
mod socket_tests {
    use std::time::Instant;

    use super::*;
    use crate::dir::socket_path;
    use crate::testing::{TestFolder, dead_socket, fake_editor, map_document, raw_editor};

    #[test]
    fn stale_sockets_of_exited_processes_are_removed() {
        let folder = TestFolder::new();
        let _editor = fake_editor(folder.path(), 4242, false, Vec::new());
        let stale = socket_path(folder.path(), crate::testing::NO_PROCESS);
        dead_socket(&stale);
        // A socket no one listens on whose process runs (its id taken by another program, say)
        // is kept, and named.
        let kept = socket_path(folder.path(), 1);
        dead_socket(&kept);
        // Files that are not sockets of the live link are none of its business.
        let lock = folder.path().join("project.lock");
        std::fs::write(&lock, "").unwrap();
        let not_a_socket = socket_path(folder.path(), crate::testing::NO_PROCESS + 1);
        std::fs::write(&not_a_socket, "").unwrap();

        let found = editors(folder.path()).unwrap();
        assert_eq!(found.answered.len(), 1);
        assert_eq!(found.answered[0].pid(), 4242);
        assert!(!stale.exists(), "removed as stale");
        assert!(kept.exists() && lock.exists() && not_a_socket.exists());
        let [Problem::Failed { pid: 1, reason }] = found.problems.as_slice() else {
            panic!("{:?}", found.problems);
        };
        assert!(
            reason.starts_with(&format!(
                "couldn't talk to the editor at {}: ",
                kept.display()
            )),
            "{reason}"
        );
    }

    #[test]
    fn another_protocol_version_is_a_problem_naming_the_process() {
        let folder = TestFolder::new();
        let _current = fake_editor(folder.path(), 10, false, Vec::new());
        // A later version whose status this one could not read: only its version is.
        let later = raw_editor(
            socket_path(folder.path(), 4242),
            "{\"ok\":{\"answer\":\"status\",\"protocol\":2,\"pid\":4242,\"windows\":[]}}\n",
        );
        let found = editors(folder.path()).unwrap();
        later.join().unwrap();
        assert_eq!(found.answered.len(), 1);
        assert_eq!(
            found.problems,
            [Problem::OtherVersion {
                pid: 4242,
                protocol: 2
            }]
        );
        assert_eq!(
            found.problems[0].to_string(),
            "better-tiled (pid 4242) is another version (protocol 2, not 1): restart it"
        );
    }

    #[test]
    fn a_status_without_a_version_is_a_problem() {
        let folder = TestFolder::new();
        let odd = raw_editor(socket_path(folder.path(), 77), "{\"ok\":{\"pid\":77}}\n");
        let found = editors(folder.path()).unwrap();
        odd.join().unwrap();
        assert!(found.answered.is_empty());
        assert_eq!(
            found.problems,
            [Problem::Failed {
                pid: 77,
                reason: "the editor's answer can't be read: its status has no protocol version"
                    .to_owned()
            }]
        );
    }

    #[test]
    fn the_focused_editor_comes_first_then_the_newest() {
        let folder = TestFolder::new();
        let _older = fake_editor(folder.path(), 10, false, Vec::new());
        let _newer = fake_editor(folder.path(), 30, false, Vec::new());
        let _focused = fake_editor(folder.path(), 20, true, Vec::new());
        let found = editors(folder.path()).unwrap();
        let pids: Vec<u32> = found.answered.iter().map(Editor::pid).collect();
        assert_eq!(pids, [20, 30, 10]);
        assert!(found.problems.is_empty());
    }

    #[test]
    fn an_editor_that_does_not_answer_in_time_is_a_problem() {
        let folder = TestFolder::new();
        // Listening, never accepting: as an editor whose window is stuck in a dialog.
        let _stuck =
            std::os::unix::net::UnixListener::bind(socket_path(folder.path(), 66)).unwrap();
        let started = Instant::now();
        let found = editors_within(folder.path(), Duration::from_millis(100)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(found.problems, [Problem::NoAnswer { pid: 66 }]);
        assert!(found.answered.is_empty());
    }

    #[test]
    fn a_map_is_found_by_its_canonical_path() {
        let folder = TestFolder::new();
        let real = folder.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("a.tmx"), "").unwrap();
        std::fs::write(real.join("t.tsx"), "").unwrap();
        let link = folder.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = std::fs::canonicalize(real.join("a.tmx")).unwrap();
        let tileset = DocStatus {
            tileset: true,
            ..map_document(
                Some(std::fs::canonicalize(real.join("t.tsx")).unwrap()),
                false,
            )
        };
        let documents = vec![
            map_document(None, false),
            tileset,
            map_document(Some(canonical.clone()), true),
        ];
        let _editor = fake_editor(folder.path(), 4242, true, documents);
        let found = editors(folder.path()).unwrap();
        let editor = &found.answered[0];
        // On macOS, `/tmp` itself is a link to `/private/tmp`.
        for path in [
            real.join("a.tmx"),
            link.join("a.tmx"),
            link.join("x/../a.tmx"),
        ] {
            let doc = editor.document(&path).unwrap();
            assert_eq!(
                doc.path.as_deref(),
                Some(canonical.as_path()),
                "{}",
                path.display()
            );
        }
        assert!(
            editor.document(&real.join("t.tsx")).is_none(),
            "a tileset is not a map"
        );
        assert!(editor.document(&real.join("b.tmx")).is_none());
        assert_eq!(
            found.with_map_open(&link.join("a.tmx")).map(Editor::pid),
            Some(4242)
        );
        assert!(found.with_map_open(&real.join("t.tsx")).is_none());
    }

    #[test]
    fn requests_go_to_the_editor_asked() {
        let folder = TestFolder::new();
        let _editor = fake_editor(folder.path(), 4242, false, Vec::new());
        let found = editors(folder.path()).unwrap();
        let open = Request::Open {
            path: "/maps/a.tmx".into(),
        };
        let done = found.answered[0].ask(&open).unwrap().into_done().unwrap();
        assert_eq!(done.message, format!("{open:?}"));
        assert!(matches!(
            found.answered[0].ask(&open).unwrap().into_map_text(),
            Err(Error::Unexpected { .. })
        ));
    }
}
