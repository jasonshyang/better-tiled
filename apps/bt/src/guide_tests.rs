//! The guide against the command table (`10-agents.md`, §14): every command and option the
//! guide names exists in the table, and every command and option of the table is in the guide.

use std::collections::BTreeSet;

use clap::CommandFactory;

use crate::cli::Cli;
use crate::run::GUIDE;

/// The commands and long options of the table, without clap's own `help` and `version`.
fn table() -> Named {
    let cli = Cli::command();
    let mut named = Named::default();
    let add_options = |command: &clap::Command, named: &mut Named| {
        for argument in command.get_arguments() {
            if let Some(long) = argument.get_long()
                && long != "help"
                && long != "version"
            {
                named.options.insert(long.to_owned());
            }
        }
    };
    add_options(&cli, &mut named);
    for command in cli.get_subcommands() {
        if command.get_name() == "help" {
            continue;
        }
        named.commands.insert(command.get_name().to_owned());
        add_options(command, &mut named);
    }
    named
}

/// Commands and options.
#[derive(Debug, Default, PartialEq, Eq)]
struct Named {
    commands: BTreeSet<String>,
    options: BTreeSet<String>,
}

/// The code of the guide: indented code lines, and inline code spans (which may run over a line
/// break), where commands are typed.
fn code_of(guide: &str) -> Vec<String> {
    let (blocks, prose): (Vec<&str>, Vec<&str>) =
        guide.lines().partition(|line| line.starts_with("    "));
    let mut code: Vec<String> = blocks.iter().map(|line| (*line).to_owned()).collect();
    let prose = prose.join("\n");
    code.extend(prose.split('`').skip(1).step_by(2).map(str::to_owned));
    code
}

/// The commands (`bt NAME`) and options (`--name`) the guide's code names.
fn named_in_guide() -> Named {
    let mut named = Named::default();
    for code in code_of(GUIDE) {
        let words: Vec<&str> = code.split_whitespace().collect();
        for pair in words.windows(2) {
            if let [first, second] = pair
                && *first == "bt"
                && second.chars().all(|c| c.is_ascii_lowercase())
            {
                named.commands.insert((*second).to_owned());
            }
        }
        for word in code.split(|c: char| c.is_whitespace() || "[]|(),\"'".contains(c)) {
            if let Some(option) = word.strip_prefix("--") {
                let option: String = option
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                if !option.is_empty() {
                    named.options.insert(option);
                }
            }
        }
    }
    named.commands.remove("help");
    named
}

#[test]
fn every_command_and_option_the_guide_names_exists_and_every_one_is_in_the_guide() {
    let table = table();
    let guide = named_in_guide();
    let unknown: Vec<&String> = guide.commands.difference(&table.commands).collect();
    assert!(
        unknown.is_empty(),
        "the guide names commands bt lacks: {unknown:?}"
    );
    let missing: Vec<&String> = table.commands.difference(&guide.commands).collect();
    assert!(
        missing.is_empty(),
        "the guide leaves out commands: {missing:?}"
    );
    let unknown: Vec<&String> = guide.options.difference(&table.options).collect();
    assert!(
        unknown.is_empty(),
        "the guide names options bt lacks: {unknown:?}"
    );
    let missing: Vec<&String> = table.options.difference(&guide.options).collect();
    assert!(
        missing.is_empty(),
        "the guide leaves out options: {missing:?}"
    );
}

#[test]
fn the_command_table_is_consistent() {
    Cli::command().debug_assert();
}
