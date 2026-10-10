//! Two programs writing one project file at once (`09-project-file.md`, §7): two processes,
//! and two threads with separate handles, editing different entries lose nothing, and an edit
//! waits for another's lock and reads what it wrote. The second process is this test binary run
//! again, with the variables below saying which file to edit.

#![cfg(test)]

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bt_project::{FILE_NAME, ProjectFile};
use bt_test_data::Scratch;

use common::{lock_folder, record};

/// The project file a child process edits.
const CHILD_FILE: &str = "BT_PROJECT_TEST_CHILD_FILE";
/// The lock folder a child process takes its locks in.
const CHILD_LOCKS: &str = "BT_PROJECT_TEST_CHILD_LOCKS";
/// The prefix of the keys a child process writes.
const CHILD_PREFIX: &str = "BT_PROJECT_TEST_CHILD_PREFIX";

/// How many edits each writer makes, one entry each.
const EDITS: usize = 15;

/// Adds `EDITS` entries to the file, one edit each, under keys starting with `prefix`.
fn add_entries(file: &Path, locks: &Path, prefix: &str) {
    for n in 0..EDITS {
        let key = format!("{prefix}-{n:02}.tmx");
        ProjectFile::edit(file, locks, |document| {
            document.set_automap(&key, record(&key));
        })
        .unwrap();
    }
}

/// Every entry each of `prefixes` wrote is in the file, with its own record.
fn assert_all_there(file: &Path, prefixes: &[&str]) {
    let document = ProjectFile::load(file).unwrap();
    assert_eq!(
        document.keys().count(),
        prefixes.len() * EDITS,
        "{:?}",
        document.keys().collect::<Vec<_>>()
    );
    for prefix in prefixes {
        for n in 0..EDITS {
            let key = format!("{prefix}-{n:02}.tmx");
            assert_eq!(
                document.automap(&key).map(|record| record.rules.as_str()),
                Some(key.as_str())
            );
        }
    }
}

/// Run as a child process by `two_processes_editing_one_file_lose_nothing`; without its
/// variables, it does nothing.
#[test]
fn a_child_process_adds_its_entries_when_asked() {
    let (Some(file), Some(locks), Some(prefix)) = (
        std::env::var_os(CHILD_FILE),
        std::env::var_os(CHILD_LOCKS),
        std::env::var(CHILD_PREFIX).ok(),
    ) else {
        return;
    };
    add_entries(Path::new(&file), Path::new(&locks), &prefix);
}

#[test]
fn two_processes_editing_one_file_lose_nothing() {
    let scratch = Scratch::new("bt-project-two-processes").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    let this_test = std::env::current_exe().unwrap();
    let children: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|prefix| {
            Command::new(&this_test)
                .args([
                    "a_child_process_adds_its_entries_when_asked",
                    "--exact",
                    "--test-threads=1",
                ])
                .env(CHILD_FILE, &file)
                .env(CHILD_LOCKS, &locks)
                .env(CHILD_PREFIX, prefix)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    // This process writes too, while the children do.
    add_entries(&file, &locks, "parent");
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "the child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_all_there(&file, &["first", "second", "parent"]);
}

#[test]
fn two_threads_with_separate_handles_editing_one_file_lose_nothing() {
    let scratch = Scratch::new("bt-project-two-threads").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join("game").join(FILE_NAME);
    std::fs::create_dir_all(scratch.join("game")).unwrap();
    // One thread reaches the file through a link where links can be made: one file, one lock.
    #[cfg(unix)]
    let other_path: PathBuf = {
        std::os::unix::fs::symlink(scratch.join("game"), scratch.join("link")).unwrap();
        scratch.join("link").join(FILE_NAME)
    };
    #[cfg(not(unix))]
    let other_path: PathBuf = file.clone();
    let writers: Vec<_> = [("left", file.clone()), ("right", other_path)]
        .into_iter()
        .map(|(prefix, path)| {
            let locks = locks.clone();
            thread::spawn(move || add_entries(&path, &locks, prefix))
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    assert_all_there(&file, &["left", "right"]);
}

#[test]
fn an_edit_waits_for_the_lock_and_reads_what_the_holder_wrote() {
    let scratch = Scratch::new("bt-project-lock-wait").unwrap();
    let locks = lock_folder(&scratch);
    let file = scratch.join(FILE_NAME);
    let (inside, holding) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let holder = {
        let (file, locks) = (file.clone(), locks.clone());
        thread::spawn(move || {
            ProjectFile::edit(&file, &locks, |document| {
                inside.send(()).unwrap();
                released.recv().unwrap();
                document.set_automap("held.tmx", record("held"));
            })
            .unwrap();
        })
    };
    holding.recv().unwrap();
    let (finished, waiter_done) = mpsc::channel();
    let waiter = {
        let (file, locks) = (file.clone(), locks.clone());
        thread::spawn(move || {
            ProjectFile::edit(&file, &locks, |document| {
                document.set_automap("waiting.tmx", record("waiting"));
            })
            .unwrap();
            finished.send(()).unwrap();
        })
    };
    assert!(
        waiter_done
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "an edit finished while another held the lock"
    );
    release.send(()).unwrap();
    holder.join().unwrap();
    waiter.join().unwrap();
    let document = ProjectFile::load(&file).unwrap();
    assert_eq!(
        document.keys().collect::<Vec<_>>(),
        ["held.tmx", "waiting.tmx"]
    );
}
