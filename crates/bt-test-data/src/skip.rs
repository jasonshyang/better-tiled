//! What a test does when something it needs is missing: it fails, unless skipping is asked for.

use std::io::Write as _;
use std::path::Path;

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
    if let Some(log) = std::env::var_os(SKIP_LOG_VAR)
        && let Err(error) = append_skip(Path::new(&log), what)
    {
        // Continuous integration counts on the log: a skip it can't see must not pass.
        panic!(
            "couldn't record the skip of {what} in {}: {error}",
            log.to_string_lossy()
        );
    }
}

/// Keeps an entry and its newline together, including when other test processes append.
fn append_skip(log: &Path, what: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(log)?;
    file.lock()?;
    writeln!(file, "{what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_skips_keep_one_entry_per_line() {
        let scratch = crate::Scratch::new("parallel-skip-log").unwrap();
        let path = scratch.join("skips.txt");
        let ready = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for worker in 0..16 {
                let path = &path;
                let ready = &ready;
                scope.spawn(move || {
                    ready.wait();
                    for entry in 0..32 {
                        append_skip(path, &format!("assets/pack-{worker}-{entry}")).unwrap();
                    }
                });
            }
        });
        let contents = std::fs::read_to_string(path).unwrap();
        let mut actual: Vec<_> = contents.lines().map(str::to_owned).collect();
        let mut expected: Vec<_> = (0..16)
            .flat_map(|worker| (0..32).map(move |entry| format!("assets/pack-{worker}-{entry}")))
            .collect();
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
