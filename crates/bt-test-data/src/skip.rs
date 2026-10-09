//! What a test does when something it needs is missing: it fails, unless skipping is asked for.

use std::io::Write as _;

/// The environment variable that turns missing data into skipped tests.
pub const SKIP_VAR: &str = "BT_SKIP_MISSING_TEST_DATA";

/// The environment variable naming a file every skip is appended to, one line each.
pub const SKIP_LOG_VAR: &str = "BT_SKIP_LOG";

/// Whether missing data skips tests rather than failing them.
pub fn skipping() -> bool {
    std::env::var_os(SKIP_VAR).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Fails the calling test because `what` is missing (a path relative to the workspace, or a
/// program such as "Godot 4"), saying how to get it. When skipping is asked for, it prints that
/// the test skips, appends `what` to the skip log, and returns, and the test should return too.
#[allow(
    clippy::panic,
    clippy::print_stderr,
    reason = "failing or skipping the calling test, and saying so, is this function's purpose"
)]
pub fn missing(what: &str, hint: &str) {
    if !skipping() {
        panic!("{what} is missing ({hint}); set {SKIP_VAR}=1 to skip the tests that need it");
    }
    eprintln!("skipping: {what} is missing ({hint})");
    if let Some(log) = std::env::var_os(SKIP_LOG_VAR) {
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .and_then(|mut file| writeln!(file, "{what}"));
        if let Err(error) = appended {
            // Continuous integration counts on the log: a skip it can't see must not pass.
            panic!(
                "couldn't record the skip of {what} in {}: {error}",
                log.to_string_lossy()
            );
        }
    }
}
