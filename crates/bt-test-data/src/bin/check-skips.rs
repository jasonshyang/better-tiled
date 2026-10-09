//! `check-skips LOG [--allow PREFIX]…`: fails when the skip log (`BT_SKIP_LOG`) records a skip
//! that starts with none of the allowed prefixes. A log that doesn't exist recorded no skip.
//!
//! Continuous integration runs it after the tests with `--allow assets/`: the purchased packs are
//! never on its machines, but a skip of `reference/` means the fetch or a path is broken.

use std::process::ExitCode;

#[allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a program's output"
)]
fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut log = None;
    let mut allowed = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--allow" {
            match args.next() {
                Some(prefix) => allowed.push(prefix),
                None => return usage("--allow needs a prefix"),
            }
        } else if arg.starts_with("--") || log.is_some() {
            return usage(&format!("unexpected {arg}"));
        } else {
            log = Some(arg);
        }
    }
    let Some(log) = log else {
        return usage("name the skip log");
    };
    let text = match std::fs::read_to_string(&log) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            eprintln!("check-skips: couldn't read {log}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let allowed: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let refused = bt_test_data::disallowed_skips(&text, &allowed);
    if refused.is_empty() {
        println!(
            "check-skips: no skips beyond {}",
            if allowed.is_empty() {
                "none".to_owned()
            } else {
                allowed.join(", ")
            }
        );
        return ExitCode::SUCCESS;
    }
    eprintln!("check-skips: tests skipped what this run should have had:");
    for line in refused {
        eprintln!("  {line}");
    }
    ExitCode::FAILURE
}

#[allow(clippy::print_stderr, reason = "a program's output")]
fn usage(problem: &str) -> ExitCode {
    eprintln!("check-skips: {problem}\nusage: check-skips LOG [--allow PREFIX]…");
    ExitCode::from(2)
}
