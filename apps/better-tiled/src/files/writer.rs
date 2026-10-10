//! The writer thread: recovery snapshots written and deleted and the session written, in the
//! order asked, and the run's heartbeat touched every few seconds while the window lives.
//! Finishing it does what was asked first and removes the heartbeat, so the snapshots a run
//! leaves are offered back at the next start.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bt_editor::{Snapshot, delete_snapshot, remove_heartbeat, touch_heartbeat};
use bt_utils::atomic_write::{WriteMode, write_file};

/// How often the heartbeat is touched (a run counts as alive for a minute after).
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);

/// What the writer is asked to do.
enum WriteJob {
    Snapshot(Snapshot),
    DeleteSnapshot(PathBuf),
    Session { path: PathBuf, text: String },
}

/// Where the run's heartbeat goes.
#[derive(Clone, Debug)]
struct Heartbeat {
    folder: PathBuf,
    pid: u32,
}

/// The writer thread and the problems it ran into.
pub(crate) struct BackgroundWriter {
    jobs: Option<Sender<WriteJob>>,
    problems: Receiver<String>,
    thread: Option<JoinHandle<()>>,
}

impl BackgroundWriter {
    /// Starts the writer; the heartbeat of run `pid` goes in `recovery_folder`.
    pub(crate) fn start(recovery_folder: PathBuf, pid: u32) -> Result<BackgroundWriter, String> {
        let (jobs, received) = channel();
        let (report, problems) = channel();
        let heartbeat = Heartbeat {
            folder: recovery_folder,
            pid,
        };
        let thread = std::thread::Builder::new()
            .name("bt-writer".to_owned())
            .spawn(move || run(&received, &report, &heartbeat))
            .map_err(|error| format!("the file writer did not start: {error}"))?;
        Ok(BackgroundWriter {
            jobs: Some(jobs),
            problems,
            thread: Some(thread),
        })
    }

    /// Writes a recovery snapshot.
    pub(crate) fn write_snapshot(&self, snapshot: Snapshot) {
        self.send(WriteJob::Snapshot(snapshot));
    }

    /// Deletes a recovery snapshot.
    pub(crate) fn delete_snapshot(&self, path: PathBuf) {
        self.send(WriteJob::DeleteSnapshot(path));
    }

    /// Writes the session.
    pub(crate) fn write_session(&self, path: PathBuf, text: String) {
        self.send(WriteJob::Session { path, text });
    }

    /// What went wrong since the last call.
    pub(crate) fn take_problems(&self) -> Vec<String> {
        self.problems.try_iter().collect()
    }

    /// Does everything asked so far, removes the heartbeat and stops; later requests are
    /// dropped.
    pub(crate) fn finish(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("the file writer stopped with a panic");
        }
    }

    fn send(&self, job: WriteJob) {
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|jobs| jobs.send(job).is_ok());
        if !sent {
            log::warn!("the file writer has stopped; a write was dropped");
        }
    }
}

impl Drop for BackgroundWriter {
    fn drop(&mut self) {
        self.finish();
    }
}

fn run(jobs: &Receiver<WriteJob>, report: &Sender<String>, heartbeat: &Heartbeat) {
    let mut touched = touch(heartbeat, report);
    loop {
        match jobs.recv_timeout(HEARTBEAT_EVERY) {
            Ok(job) => {
                if let Err(problem) = carry_out(job) {
                    log::warn!("{problem}");
                    let _ = report.send(problem);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if touched.elapsed() >= HEARTBEAT_EVERY {
            touched = touch(heartbeat, report);
        }
    }
    if let Err(error) = remove_heartbeat(&heartbeat.folder, heartbeat.pid) {
        log::warn!("the heartbeat could not be removed: {error}");
    }
}

fn touch(heartbeat: &Heartbeat, report: &Sender<String>) -> Instant {
    if let Err(error) = touch_heartbeat(&heartbeat.folder, heartbeat.pid) {
        let problem = format!(
            "Recovery may not work: {} cannot be written ({error})",
            heartbeat.folder.display()
        );
        log::warn!("{problem}");
        let _ = report.send(problem);
    }
    Instant::now()
}

fn carry_out(job: WriteJob) -> Result<(), String> {
    match job {
        WriteJob::Snapshot(snapshot) => snapshot.write().map_err(|error| {
            format!(
                "The recovery snapshot {} could not be written: {error}",
                snapshot.path.display()
            )
        }),
        WriteJob::DeleteSnapshot(path) => delete_snapshot(&path).map_err(|error| {
            format!(
                "The recovery snapshot {} could not be deleted: {error}",
                path.display()
            )
        }),
        WriteJob::Session { path, text } => {
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder).map_err(|error| {
                    format!(
                        "The session could not be written to {}: {error}",
                        folder.display()
                    )
                })?;
            }
            write_file(&path, text.as_bytes(), WriteMode::Atomic).map_err(|error| {
                format!(
                    "The session could not be written to {}: {error}",
                    path.display()
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_editor::SnapshotMeta;

    use super::*;

    #[test]
    fn writes_happen_in_order_and_finishing_removes_the_heartbeat() {
        let folder = std::env::temp_dir()
            .join("better-tiled-writer-tests")
            .join(std::process::id().to_string());
        let _ = std::fs::remove_dir_all(&folder);
        let mut writer = BackgroundWriter::start(folder.clone(), 7).unwrap();
        let snapshot = Snapshot {
            path: folder.join("7-1.snapshot"),
            text: "<map/>".to_owned(),
            meta: SnapshotMeta {
                original: None,
                pid: 7,
            },
        };
        writer.write_snapshot(snapshot.clone());
        writer.write_session(folder.join("session.json"), "{}".to_owned());
        writer.delete_snapshot(snapshot.path.clone());
        writer.write_snapshot(Snapshot {
            path: folder.join("7-2.snapshot"),
            ..snapshot
        });
        writer.finish();
        assert!(!folder.join("7-1.snapshot").exists());
        assert!(folder.join("7-2.snapshot").exists());
        assert_eq!(
            std::fs::read_to_string(folder.join("session.json")).unwrap(),
            "{}"
        );
        assert!(!folder.join("7.alive").exists());
        assert!(writer.take_problems().is_empty());
    }
}
