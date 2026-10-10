//! What the integration tests share.

#![allow(dead_code, reason = "each test file uses the helpers it needs")]

use std::path::{Path, PathBuf};

use bt_project::AutoMapRecord;
use bt_test_data::Scratch;

/// An AutoMap record whose rules are `rules`.
pub fn record(rules: &str) -> AutoMapRecord {
    AutoMapRecord {
        rules: rules.to_owned(),
        seed: 7,
        ..AutoMapRecord::default()
    }
}

/// A folder of the test's own for write locks, so no test touches the user's runtime folder.
pub fn lock_folder(scratch: &Scratch) -> PathBuf {
    let locks = scratch.join("locks");
    std::fs::create_dir_all(&locks).unwrap();
    locks
}

/// Writes `text` to `path`, making its folders.
pub fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}
