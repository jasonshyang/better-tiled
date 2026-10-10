//! The watcher: a thread that looks at the editor's watched files every 800 ms and hands what
//! changed to the window, which gives it to the editor. A file is read and hashed again only
//! when its modification time or length changed; the editor decides what a change means.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use bt_editor::{Found, Observation};
use bt_utils::hash::fnv1a_64;

/// How often the files are looked at.
pub(crate) const WATCH_INTERVAL: Duration = Duration::from_millis(800);

/// What the watcher shares with its thread.
struct Shared {
    /// The files to look at, as the editor last listed them.
    watched: Mutex<Vec<PathBuf>>,
    /// What was found last, so only changes are reported.
    poller: Mutex<Poller>,
    stop: AtomicBool,
}

/// The window's side of the watcher.
pub(crate) struct Watcher {
    shared: Arc<Shared>,
    results: Receiver<Vec<Observation>>,
    sender: Sender<Vec<Observation>>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// A watcher with no thread yet: [`Watcher::start`] polls in the background, and
    /// [`Watcher::poll_now`] polls on the calling thread (for tests).
    pub(crate) fn new() -> Watcher {
        let (sender, results) = channel();
        Watcher {
            shared: Arc::new(Shared {
                watched: Mutex::new(Vec::new()),
                poller: Mutex::new(Poller::default()),
                stop: AtomicBool::new(false),
            }),
            results,
            sender,
            thread: None,
        }
    }

    /// Starts looking at the files every `interval`, waking `waker` when something changed.
    pub(crate) fn start(&mut self, waker: egui::Context, interval: Duration) -> Result<(), String> {
        let shared = Arc::clone(&self.shared);
        let sender = self.sender.clone();
        let thread = std::thread::Builder::new()
            .name("bt-watcher".to_owned())
            .spawn(move || {
                while sleep_unless_stopped(&shared.stop, interval) {
                    let changed = poll(&shared);
                    if changed.is_empty() {
                        continue;
                    }
                    if sender.send(changed).is_err() {
                        return;
                    }
                    waker.request_repaint();
                }
            })
            .map_err(|error| format!("no thread to watch files: {error}"))?;
        self.thread = Some(thread);
        Ok(())
    }

    /// The files to look at from now on.
    pub(crate) fn set_watched(&self, paths: Vec<PathBuf>) {
        let mut watched = self
            .shared
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *watched != paths {
            *watched = paths;
        }
    }

    /// Looks at the files now, on this thread.
    #[cfg(test)]
    pub(crate) fn poll_now(&self) {
        let changed = poll(&self.shared);
        if !changed.is_empty() {
            let _ = self.sender.send(changed);
        }
    }

    /// What changed since the last call, if anything.
    pub(crate) fn take_observations(&self) -> Option<Vec<Observation>> {
        let mut all = Vec::new();
        while let Ok(batch) = self.results.try_recv() {
            all.extend(batch);
        }
        (!all.is_empty()).then_some(all)
    }

    /// Stops the thread and waits for it.
    pub(crate) fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Sleeps for `interval` in short steps; whether to go on (false once stopped).
fn sleep_unless_stopped(stop: &AtomicBool, interval: Duration) -> bool {
    const STEP: Duration = Duration::from_millis(50);
    let mut slept = Duration::ZERO;
    while slept < interval {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(STEP);
        slept += STEP;
    }
    !stop.load(Ordering::Relaxed)
}

/// Looks at the watched files once; the observations that differ from last time.
fn poll(shared: &Shared) -> Vec<Observation> {
    let watched = shared
        .watched
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    shared
        .poller
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .poll(&watched)
}

/// What a file was found to be: its stamp, so it is hashed again only when that changes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Seen {
    modified: Option<SystemTime>,
    len: u64,
    found: Found,
}

/// The last look at each file.
#[derive(Debug, Default)]
struct Poller {
    seen: HashMap<PathBuf, Seen>,
}

impl Poller {
    /// Looks at `paths`; the observations that differ from the last look (all of them the
    /// first time). Files no longer watched are forgotten, so watching one again reports it.
    fn poll(&mut self, paths: &[PathBuf]) -> Vec<Observation> {
        self.seen.retain(|path, _| paths.contains(path));
        let mut changed = Vec::new();
        for path in paths {
            let seen = look(path, self.seen.get(path));
            let before = self.seen.insert(path.clone(), seen);
            if before.map(|b| b.found) != Some(seen.found) {
                changed.push(Observation {
                    path: path.clone(),
                    found: seen.found,
                });
            }
        }
        changed
    }
}

/// A file as it is now, reusing the last hash when its stamp is unchanged.
fn look(path: &PathBuf, last: Option<&Seen>) -> Seen {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Seen {
            modified: None,
            len: 0,
            found: Found::Missing,
        };
    };
    let modified = metadata.modified().ok();
    let len = metadata.len();
    if let Some(last) = last
        && last.modified == modified
        && last.len == len
        && last.found != Found::Missing
    {
        return *last;
    }
    let found = match std::fs::read(path) {
        Ok(bytes) => Found::Present {
            content_hash: fnv1a_64(&bytes),
        },
        Err(_) => Found::Missing,
    };
    Seen {
        modified,
        len,
        found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changes_are_reported() {
        let scratch = bt_test_data::Scratch::new("bt-watcher").unwrap();
        let file = scratch.join("a.txt");
        std::fs::write(&file, "one").unwrap();
        let mut poller = Poller::default();
        let paths = vec![file.clone(), scratch.join("missing.txt")];
        assert_eq!(
            poller.poll(&paths).len(),
            2,
            "the first look reports everything"
        );
        assert!(poller.poll(&paths).is_empty());
        std::fs::write(&file, "two!").unwrap();
        let changed = poller.poll(&paths);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].path, file);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(poller.poll(&paths)[0].found, Found::Missing);
        assert!(poller.poll(&[]).is_empty());
        std::fs::write(&file, "two!").unwrap();
        assert_eq!(
            poller.poll(&paths).len(),
            2,
            "watched again: reported again"
        );
    }
}
