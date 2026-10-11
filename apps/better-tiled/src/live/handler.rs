//! A connection's request turned into jobs for the window (`11-live-link.md`, §8): the
//! handler runs on the connection's thread, so the window never waits for a connection.

use bt_editor::MarkCells;
use bt_live::{
    Handler, JOB_WAIT, Jobs, LONG_JOB_WAIT, Request, Response, canonical_path, cells_from_runs,
};

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
            Answered::Base(_) | Answered::Applied(_) => {
                Err("the editor answered another question".to_owned())
            }
        }
    }
}

impl Handler for LiveHandler {
    fn handle(&self, request: &Request) -> Result<Response, String> {
        match request {
            Request::Status => self.ask(LiveJob::Status, JOB_WAIT),
            Request::Snapshot { path } => self.ask(
                LiveJob::Snapshot {
                    path: canonical_path(path),
                },
                JOB_WAIT,
            ),
            Request::Edit(_) => Err("bt edit in the editor is not built yet".to_owned()),
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
