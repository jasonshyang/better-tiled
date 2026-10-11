//! Running a command: its arguments in, what it prints out.

use std::cell::OnceCell;

use anyhow::bail;

use crate::cli::{Cli, Command};
use crate::commands;
use crate::exit::Printed;
use crate::folders::Folders;
use crate::link::Link;

/// What a command runs with: its arguments, the folders, and the running editors, looked for
/// once, when the command first needs them (a command that reads no map never waits for one).
pub(crate) struct Run {
    /// The arguments.
    pub(crate) cli: Cli,
    /// The folders.
    pub(crate) folders: Folders,
    link: OnceCell<Link>,
}

impl Run {
    /// A run of `cli` with `folders`.
    pub(crate) fn new(cli: Cli, folders: Folders) -> Run {
        Run {
            cli,
            folders,
            link: OnceCell::new(),
        }
    }

    /// A run that finds no editor: `bt` works with files only.
    #[cfg(test)]
    pub(crate) fn offline(cli: Cli, folders: Folders) -> Run {
        Run {
            cli,
            folders,
            link: OnceCell::from(Link::none()),
        }
    }

    /// A run of `bt` with `args` (without the program's name), offline, with `folders`.
    #[cfg(test)]
    pub(crate) fn of_args(args: &[&str], folders: Folders) -> Run {
        use clap::Parser;
        let cli = Cli::try_parse_from(std::iter::once("bt").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{error}"));
        Run::offline(cli, folders)
    }

    /// A run for a test that reads maps with `project`'s types.
    #[cfg(test)]
    pub(crate) fn for_test(project: Option<&std::path::Path>) -> Run {
        let mut args = vec!["guide"];
        let project = project.map(|p| p.to_string_lossy().into_owned());
        if let Some(project) = &project {
            args.extend(["--project", project.as_str()]);
        }
        Run::of_args(&args, Folders::default())
    }

    /// The running editors.
    pub(crate) fn link(&self) -> &Link {
        self.link.get_or_init(Link::find)
    }
}

/// The guide, the only documentation an agent gets.
pub(crate) const GUIDE: &str = include_str!("guide.md");

/// Runs the command `run` names.
pub(crate) fn run(run: &Run) -> anyhow::Result<Printed> {
    match &run.cli.command {
        Command::Guide => Ok(Printed::done(GUIDE)),
        Command::Export(args) => commands::export::export(run, args),
        Command::Undo(args) => commands::undo::undo(run, args),
        Command::Edit(args) => commands::edit::edit(run, args),
        Command::Areas(args) => commands::know::areas(run, args),
        Command::Reach(args) => commands::know::reach(run, args),
        Command::Editor(args) => commands::editor::editor(run, args),
        Command::Mark(args) => commands::editor::mark(run, args),
        Command::Open(args) => commands::editor::open(run, args),
        Command::Save(args) => commands::editor::save(run, args),
        Command::Diff(args) => commands::diff::diff(run, args),
        Command::Check(args) => commands::check::check(run, args),
        Command::Info(args) => commands::look::info(run, args),
        Command::At(args) => commands::look::at(run, args),
        Command::Objects(args) => commands::look::objects(run, args),
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
