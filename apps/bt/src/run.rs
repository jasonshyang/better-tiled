//! Running a command: its arguments in, what it prints out.

use anyhow::bail;

use crate::cli::{Cli, Command};
use crate::commands;
use crate::exit::Printed;
use crate::folders::Folders;

/// The guide, the only documentation an agent gets.
pub(crate) const GUIDE: &str = include_str!("guide.md");

/// Runs the command `cli` names, with the folders `folders` names.
pub(crate) fn run(cli: &Cli, folders: &Folders) -> anyhow::Result<Printed> {
    match &cli.command {
        Command::Guide => Ok(Printed::done(GUIDE)),
        Command::Export(args) => commands::export::export(cli, args, folders.home.as_deref()),
        Command::Undo(args) => commands::undo::undo(cli, args, folders),
        other => bail!("{} is not built yet", command_name(other)),
    }
}

/// The command's name as typed.
fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Guide => "guide",
        Command::Info(_) => "info",
        Command::View(_) => "view",
        Command::At(_) => "at",
        Command::Objects(_) => "objects",
        Command::Catalog(_) => "catalog",
        Command::Areas(_) => "areas",
        Command::Reach(_) => "reach",
        Command::Explain(_) => "explain",
        Command::Render(_) => "render",
        Command::New(_) => "new",
        Command::Edit(_) => "edit",
        Command::Undo(_) => "undo",
        Command::Check(_) => "check",
        Command::Diff(_) => "diff",
        Command::Editor(_) => "editor",
        Command::Mark(_) => "mark",
        Command::Open(_) => "open",
        Command::Save(_) => "save",
        Command::Export(_) => "export",
    }
}
