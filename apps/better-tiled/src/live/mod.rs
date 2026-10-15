//! The live link in the window (`06-application.md`, §11; `11-live-link.md`, §8–§11): a
//! listener answers `bt` on connection threads, and what needs the editor reaches the window as
//! jobs with a deadline, done between frames in `logic`, so they are answered while the window is
//! hidden. The preference "Let bt work with the open maps" starts and stops the listener, and a
//! listener whose socket vanished (macOS cleans `/tmp`) is started again.

mod edit;
mod handler;
mod jobs;

use std::path::PathBuf;
use std::time::Instant;

use bt_live::{Job, Jobs, SOCKET_CHECK_INTERVAL, Server};

pub(crate) use jobs::{Answered, LiveAnswer, LiveJob};

/// The window's side of the live link.
pub(crate) struct LiveLink {
    /// Where editors listen; none in checks and tests that do not ask for it.
    folder: Option<PathBuf>,
    server: Option<Server>,
    jobs: Jobs<LiveJob, LiveAnswer>,
    /// When the socket was last looked for.
    checked: Instant,
    /// The last reason the listener could not start, said once.
    failed: Option<String>,
}

impl LiveLink {
    /// A live link listening in `folder` once [`LiveLink::keep_listening`] allows it; `waker`
    /// runs a frame when a job arrives.
    pub(crate) fn new(folder: Option<PathBuf>, waker: impl Fn() + Send + Sync + 'static) -> Self {
        LiveLink {
            folder,
            server: None,
            jobs: Jobs::new(waker),
            checked: Instant::now(),
            failed: None,
        }
    }

    /// Starts or stops the listener as `enabled` says, and starts it again when its socket
    /// vanished; a reason it could not start, the first time.
    pub(crate) fn keep_listening(&mut self, enabled: bool) -> Option<String> {
        let Some(folder) = self.folder.as_ref().filter(|_| enabled) else {
            self.server = None;
            self.failed = None;
            return None;
        };
        if let Some(server) = &self.server {
            if self.checked.elapsed() < SOCKET_CHECK_INTERVAL {
                return None;
            }
            self.checked = Instant::now();
            if !server.lost() {
                return None;
            }
            self.server = None;
        }
        match Server::start(folder, handler::LiveHandler::new(self.jobs.clone())) {
            Ok(server) => {
                self.server = Some(server);
                self.failed = None;
                self.checked = Instant::now();
                None
            }
            Err(error) => {
                let reason = format!("bt cannot work with the open maps: {error}");
                if self.failed.as_ref() == Some(&reason) {
                    return None;
                }
                self.failed = Some(reason.clone());
                Some(reason)
            }
        }
    }

    /// Whether the listener is up.
    #[cfg(all(test, unix))]
    pub(crate) fn is_listening(&self) -> bool {
        self.server.is_some()
    }

    /// The jobs the connections asked for since the last frame.
    pub(crate) fn take_jobs(&self) -> Vec<Job<LiveJob, LiveAnswer>> {
        self.jobs.take()
    }
}
