//! The command line: the files to open, and the checks that run without a person.

use std::path::PathBuf;

/// What `--help` prints.
pub(crate) const USAGE: &str = "\
Usage: better-tiled [options] [files...]

Opens the given maps.

Options:
  -h, --help               Show this help
  -v, --version            Show the version
  --screenshot <out.png>   Open the files, wait until they are drawn, save the window as a
                           picture and quit
  --bench                  Pan and zoom through the current map at every zoom, print frame
                           times and the renderer's account, and quit";

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cli {
    /// What to do.
    pub(crate) mode: Mode,
    /// Files to open.
    pub(crate) files: Vec<PathBuf>,
    /// `--screenshot`: where the picture of the window goes.
    pub(crate) screenshot: Option<PathBuf>,
    /// `--bench`: pan and zoom, print, quit.
    pub(crate) bench: bool,
}

/// What a run does.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Open the window.
    #[default]
    Edit,
    /// Print the usage.
    Help,
    /// Print the version.
    Version,
}

impl Cli {
    /// Reads the arguments (without the program's name). Unknown options are errors.
    pub(crate) fn parse(args: &[String]) -> Result<Cli, String> {
        let mut cli = Cli::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => cli.mode = Mode::Help,
                "-v" | "--version" if cli.mode != Mode::Help => cli.mode = Mode::Version,
                "-v" | "--version" => {}
                "--screenshot" => {
                    let out = args
                        .next()
                        .ok_or_else(|| "--screenshot needs a file to write".to_owned())?;
                    cli.screenshot = Some(PathBuf::from(out));
                }
                "--bench" => cli.bench = true,
                "--" => cli.files.extend(args.by_ref().map(PathBuf::from)),
                // macOS adds a process serial number when an app is started from the Finder.
                other if other.starts_with("-psn_") => {}
                other if other.starts_with('-') && other.len() > 1 => {
                    return Err(format!("unknown option “{other}”"));
                }
                file => cli.files.push(PathBuf::from(file)),
            }
        }
        if cli.screenshot.is_some() && cli.bench {
            return Err("--screenshot and --bench cannot be given together".to_owned());
        }
        if cli.bench && cli.files.is_empty() {
            return Err("--bench needs a map to pan and zoom through".to_owned());
        }
        Ok(cli)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn files_and_checks_are_read() {
        let cli = parse(&["a.tmx", "--screenshot", "out.png", "--", "-b.tmx"]).unwrap();
        assert_eq!(cli.files, [PathBuf::from("a.tmx"), PathBuf::from("-b.tmx")]);
        assert_eq!(cli.screenshot, Some(PathBuf::from("out.png")));
        assert_eq!(cli.mode, Mode::Edit);
        assert!(parse(&["--bench", "a.tmx"]).unwrap().bench);
        assert_eq!(parse(&["-psn_0_1234"]).unwrap(), Cli::default());
    }

    #[test]
    fn help_wins_and_mistakes_are_errors() {
        assert_eq!(parse(&["--help", "--version"]).unwrap().mode, Mode::Help);
        assert_eq!(parse(&["--version", "--help"]).unwrap().mode, Mode::Help);
        assert_eq!(parse(&["-v"]).unwrap().mode, Mode::Version);
        assert!(
            parse(&["--frobnicate"])
                .unwrap_err()
                .contains("--frobnicate")
        );
        assert!(parse(&["--screenshot"]).is_err());
        assert!(parse(&["--bench"]).is_err());
        assert!(parse(&["a.tmx", "--bench", "--screenshot", "x.png"]).is_err());
    }
}
