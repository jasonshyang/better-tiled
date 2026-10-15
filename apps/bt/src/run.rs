//! Running a command: its arguments in, what it prints out.

use std::cell::OnceCell;

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

    /// A run of `bt` with `args`, with the editors listening in `live`, with `folders`.
    #[cfg(test)]
    pub(crate) fn with_editors_in(args: &[&str], folders: Folders, live: &std::path::Path) -> Run {
        use clap::Parser;
        let cli = Cli::try_parse_from(std::iter::once("bt").chain(args.iter().copied()))
            .unwrap_or_else(|error| panic!("{error}"));
        Run {
            cli,
            folders,
            link: OnceCell::from(Link::in_folder(live)),
        }
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
        Command::Godot { command } => commands::godot::run(run, command),
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
        Command::View(args) => commands::view::view(run, args),
        Command::Catalog(args) => commands::catalog::catalog(run, args),
        Command::Explain(args) => commands::explain::explain(run, args),
        Command::Render(args) => commands::render::render(run, args),
        Command::New(args) => commands::create::new_map(run, args),
        Command::At(args) => commands::look::at(run, args),
        Command::Objects(args) => commands::look::objects(run, args),
    }
}
