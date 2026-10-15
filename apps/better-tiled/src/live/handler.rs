//! A connection's request turned into jobs for the window (`11-live-link.md`, §8): the
//! handler runs on the connection's thread, so the window never waits for a connection.

use std::path::Path;

use bt_editor::{ExternalBase, ExternalEdit, ExternalEditError, MarkCells};
use bt_live::{
    Handler, JOB_WAIT, Jobs, LONG_JOB_WAIT, Request, Response, canonical_path, cells_from_runs,
};
use bt_utils::IRect;

use super::edit::{Window, edit_in_editor};
use super::jobs::{Answered, LiveAnswer, LiveJob};

/// Answers `bt`'s requests through the window's jobs.
pub(crate) struct LiveHandler {
    jobs: Jobs<LiveJob, LiveAnswer>,
}

impl LiveHandler {
    /// A handler asking the window through `jobs`.
    pub(crate) fn new(jobs: Jobs<LiveJob, LiveAnswer>) -> LiveHandler {
        LiveHandler { jobs }
    }

    /// Asks the window and waits up to `wait` for an answer for `bt`.
    fn ask(&self, job: LiveJob, wait: std::time::Duration) -> Result<Response, String> {
        match self.jobs.ask(job, wait)?? {
            Answered::Response(response) => Ok(response),
            Answered::Base(_) | Answered::Applied(_) => Err(another_question()),
        }
    }
}

impl Window for LiveHandler {
    fn prepare(&self, path: &Path) -> Result<ExternalBase, String> {
        let job = LiveJob::Prepare {
            path: path.to_path_buf(),
        };
        match self.jobs.ask(job, JOB_WAIT)?? {
            Answered::Base(base) => Ok(*base),
            Answered::Response(_) | Answered::Applied(_) => Err(another_question()),
        }
    }

    fn apply(
        &self,
        edit: ExternalEdit,
        area: Option<IRect>,
    ) -> Result<Result<(), ExternalEditError>, String> {
        let job = LiveJob::Apply {
            edit: Box::new(edit),
            area,
        };
        match self.jobs.ask(job, LONG_JOB_WAIT)?? {
            Answered::Applied(applied) => Ok(applied),
            Answered::Response(_) | Answered::Base(_) => Err(another_question()),
        }
    }
}

/// Why an answer is not the one asked for.
fn another_question() -> String {
    "the editor answered another question".to_owned()
}

impl Handler for LiveHandler {
    fn handle(&self, request: &Request) -> Result<Response, String> {
        match request {
            Request::RememberExport { path, target } => self.ask(
                LiveJob::RememberExport {
                    path: canonical_path(path),
                    target: target.clone(),
                },
                LONG_JOB_WAIT,
            ),
            Request::Status => self.ask(LiveJob::Status, JOB_WAIT),
            Request::Snapshot { path } => self.ask(
                LiveJob::Snapshot {
                    path: canonical_path(path),
                },
                JOB_WAIT,
            ),
            Request::Edit(edit) => edit_in_editor(self, edit),
            Request::Undo { path } => self.ask(
                LiveJob::Undo {
                    path: canonical_path(path),
                },
                LONG_JOB_WAIT,
            ),
            Request::Mark {
                path,
                marks,
                add,
                focus,
            } => self.ask(
                LiveJob::Mark {
                    path: canonical_path(path),
                    marks: marks
                        .iter()
                        .map(|mark| MarkCells {
                            label: mark.label.clone(),
                            cells: cells_from_runs(&mark.runs),
                        })
                        .collect(),
                    add: *add,
                    focus: *focus,
                },
                JOB_WAIT,
            ),
            Request::Open { path } => self.ask(
                LiveJob::Open {
                    path: canonical_path(path),
                },
                LONG_JOB_WAIT,
            ),
            Request::Save { path } => self.ask(
                LiveJob::Save {
                    path: canonical_path(path),
                },
                LONG_JOB_WAIT,
            ),
            Request::Screenshot { out } => {
                self.ask(LiveJob::Screenshot { out: out.clone() }, JOB_WAIT)
            }
        }
    }
}
