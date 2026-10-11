//! The guide against the command table (`10-agents.md`, §14): every command and option the
//! guide names exists in the table, and every command and option of the table is in the guide;
//! and the guide's example runs on a map like the one it was written for, `bt check` clean
//! afterwards.

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

/// A command of the guide's example, with the script it reads from standard input, if any.
#[derive(Debug)]
struct ExampleStep {
    /// The command's words after `bt`.
    words: Vec<String>,
    /// The script between `<<'EOF'` and `EOF`.
    script: Option<String>,
}

/// The commands of the guide's example, in order.
fn example_steps() -> Vec<ExampleStep> {
    let example = GUIDE
        .split("\n## Example\n")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap();
    let mut steps: Vec<ExampleStep> = Vec::new();
    let mut script: Option<Vec<String>> = None;
    for line in example.lines().filter_map(|line| line.strip_prefix("    ")) {
        if let Some(lines) = script.as_mut() {
            if line == "EOF" {
                steps.last_mut().unwrap().script = script.take().map(|lines| lines.join("\n"));
            } else {
                lines.push(line.to_owned());
            }
            continue;
        }
        let Some(command) = line.strip_prefix("bt ") else {
            panic!("the example's line {line:?} is neither a command nor a script line");
        };
        let reads_input = command.ends_with("<<'EOF'");
        let command = command.trim_end_matches("<<'EOF'");
        steps.push(ExampleStep {
            words: command.split_whitespace().map(str::to_owned).collect(),
            script: None,
        });
        if reads_input {
            script = Some(Vec::new());
        }
    }
    steps
}

#[test]
fn the_example_runs_on_a_map_like_its_own_and_check_is_clean_afterwards() {
    use bt_test_data::Scratch;

    use crate::exit::Status;
    use crate::folders::Folders;
    use crate::run::{Run, run};

    let Some(pack) = bt_test_data::test_data("assets/Epic RPG World - Highlands V1.4.1") else {
        return;
    };
    if let Err(error) = bt_render::HeadlessDevice::acquire() {
        bt_test_data::missing(
            "a GPU adapter (the example draws a picture)",
            &error.to_string(),
        );
        return;
    }
    let scratch = Scratch::new("bt-guide-example").unwrap();
    let pack = scratch.mirror(&pack, "highlands").unwrap();
    let maps = pack.join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    std::fs::create_dir_all(scratch.join("locks")).unwrap();
    let folders = Folders {
        home: None,
        data: Some(scratch.join("data")),
        locks: Some(scratch.join("locks")),
        recovery: None,
    };
    let bt = |words: &[String]| {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        run(&Run::of_args(&words, folders.clone())).unwrap()
    };
    // The map the example was written for: the pack's terrains, platforms and sprites, a sky,
    // the ground its platform rules build walls on, ice, a path, and objects.
    let tilesets: Vec<String> = [
        "Terrain - Highlands.tsx",
        "Highlands-Tileset - platformV2-3tiles tall.tsx",
        "Atlas-Props-Sprites.tsx",
    ]
    .iter()
    .map(|name| {
        pack.join("TiledMap Editor/Tilesets")
            .join(name)
            .display()
            .to_string()
    })
    .collect();
    let made = bt(&[
        "new".to_owned(),
        maps.join("c2.tmx").display().to_string(),
        "--size".to_owned(),
        "44x32".to_owned(),
        "--tile".to_owned(),
        "32x32".to_owned(),
        "--tilesets".to_owned(),
        tilesets.join(","),
        "--layers".to_owned(),
        "sky:tiles,ground:tiles,ice:tiles,dirt:tiles,props:objects,design:objects".to_owned(),
    ]);
    assert_eq!(made.status, Status::Done, "{}", made.text);

    let steps = example_steps();
    assert!(steps.iter().any(|step| step.script.is_some()), "{steps:?}");
    for (number, step) in steps.iter().enumerate() {
        // The example runs in the map's folder: its files are named from there.
        let mut words: Vec<String> = step
            .words
            .iter()
            .map(|word| {
                if word.ends_with(".tmx") || word.ends_with(".png") {
                    maps.join(word).display().to_string()
                } else {
                    word.clone()
                }
            })
            .collect();
        if let Some(script) = &step.script {
            let file = maps.join(format!("step-{number}.txt"));
            std::fs::write(&file, script).unwrap();
            words.insert(2, file.display().to_string());
        }
        let printed = bt(&words);
        let ran = format!("bt {}:\n{}", step.words.join(" "), printed.text);
        assert_eq!(printed.status, Status::Done, "{ran}");
        assert_eq!(printed.failure, None, "{ran}");
        match step.words.first().map(String::as_str) {
            Some("edit") => assert!(
                printed
                    .text
                    .contains("no new problems for bt check, and no objects on other ground"),
                "{}",
                printed.text
            ),
            Some("check") => assert!(
                printed.text.contains(": no problems found"),
                "{}",
                printed.text
            ),
            _ => {}
        }
    }
}
