//! `bt`: the editor's operations as commands for agents and scripts
//! (`docs/design/10-agents.md`). `main` reads the command table, runs the command and prints its
//! outcome as text or JSON; the exit code says how it went (0 done, 1 failed, 2 wrong usage,
//! 3 refused, 4 out of date, 5 problems found). `bt guide` is the only documentation an agent
//! gets.

mod cli;
mod commands;
mod exit;
mod folders;
#[cfg(test)]
mod guide_tests;
mod load;
mod output;
mod run;

use std::io::Write;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use crate::cli::Cli;
use crate::exit::Status;

#[allow(
    clippy::print_stderr,
    reason = "a failure's reason goes to standard error, after `bt:`"
)]
fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            // Help and the version are answers; anything else is wrong usage.
            let status = if error.use_stderr() {
                Status::Usage
            } else {
                Status::Done
            };
            let _ = error.print();
            if error.kind() == ErrorKind::UnknownArgument
                && let Some(known) = known_options(std::env::args().skip(1))
            {
                eprintln!("\n{known}");
            }
            return status.code();
        }
    };
    match run::run(&cli, &folders::Folders::from_env()) {
        Ok(printed) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(printed.text.as_bytes()).is_err() || out.flush().is_err() {
                return Status::Failed.code();
            }
            printed.status.code()
        }
        Err(failure) => {
            eprintln!("bt: {failure:#}");
            Status::Failed.code()
        }
    }
}

/// The options of the command `args` names, for an unknown option's message: `bt view takes
/// --region, --layers, …`.
fn known_options(args: impl Iterator<Item = String>) -> Option<String> {
    let table = Cli::command();
    let name = args.into_iter().find(|arg| !arg.starts_with('-'))?;
    let command = table.find_subcommand(&name)?;
    let options: Vec<String> = command
        .get_arguments()
        .chain(table.get_arguments())
        .filter_map(|argument| argument.get_long())
        .filter(|long| *long != "help" && *long != "version")
        .map(|long| format!("--{long}"))
        .collect();
    Some(format!("bt {name} takes {}", options.join(", ")))
}
