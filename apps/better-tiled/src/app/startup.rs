//! What the window starts from: the command line, the data folder, the settings (an invalid
//! settings file stops the start, with the file and the reason) and the session (an unreadable
//! one is reported in the window and left out), and how a run that checks something ends.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use bt_editor::{Session, Settings, data_folder};

use crate::cli::Cli;

/// The settings file in the data folder.
const SETTINGS_FILE: &str = "settings.toml";
/// The session file in the data folder.
const SESSION_FILE: &str = "session.json";
/// The recovery folder in the data folder.
const RECOVERY_FOLDER: &str = "recovery";

/// Everything the window needs before it opens.
pub(crate) struct Startup {
    /// Files to open.
    pub(crate) files: Vec<PathBuf>,
    /// `--screenshot`: where the picture goes.
    pub(crate) screenshot: Option<PathBuf>,
    /// The commands `BT_SCREENSHOT_ACTIONS` names, run before the picture.
    pub(crate) screenshot_actions: Vec<bt_editor::ActionId>,
    /// The panel tabs `BT_SCREENSHOT_TABS` names, brought forward for the picture.
    pub(crate) screenshot_tabs: Vec<super::frame::PanelTab>,
    /// The cell `BT_SCREENSHOT_EXPLAIN` names, explained before the picture.
    pub(crate) screenshot_explain: Option<glam::IVec2>,
    /// `--bench`.
    pub(crate) bench: bool,
    /// The preferences.
    pub(crate) settings: Settings,
    /// The last session, when there was one and it could be read.
    pub(crate) session: Option<Session>,
    /// Why the last session could not be read.
    pub(crate) session_problem: Option<String>,
    /// Where the session is written.
    pub(crate) session_path: PathBuf,
    /// Where the preferences are written.
    pub(crate) settings_path: PathBuf,
    /// Where recovery snapshots go.
    pub(crate) recovery_folder: PathBuf,
    /// Where project files are locked while they are written: the user's runtime folder, shared
    /// with `bt`.
    pub(crate) lock_folder: Option<PathBuf>,
    /// Where the live link listens for `bt`: none in checks (`--screenshot`, `--bench`) and on
    /// platforms without it.
    pub(crate) live_folder: Option<PathBuf>,
    /// This run's process id, which names its recovery snapshots and heartbeat.
    pub(crate) pid: u32,
    outcome: RunOutcome,
}

impl Startup {
    /// The start the command line asks for, with the settings and session of the data folder.
    pub(crate) fn from_cli(cli: Cli) -> Result<Startup, String> {
        let data = data_folder().map_err(|error| error.to_string())?;
        let mut startup = Startup::from_folder(cli, &data)?;
        match bt_utils::paths::runtime_dir() {
            Ok(folder) => startup.lock_folder = Some(folder),
            // Saving says so when it needs one: AutoMap layers' painted state is not kept.
            Err(error) => log::warn!("no runtime folder to lock project files in: {error}"),
        }
        let checking = startup.screenshot.is_some() || startup.bench;
        if cfg!(unix) && !checking {
            match bt_live::live_dir() {
                Ok(folder) => startup.live_folder = Some(folder),
                Err(error) => log::warn!("bt cannot work with the open maps: {error}"),
            }
        }
        Ok(startup)
    }

    /// The start with the settings and session of `data`.
    pub(crate) fn from_folder(cli: Cli, data: &Path) -> Result<Startup, String> {
        let settings_path = data.join(SETTINGS_FILE);
        let settings = Settings::load(&settings_path).map_err(|error| {
            format!("{error}\n(fix the file, or move it away to start with the defaults)")
        })?;
        let session_path = data.join(SESSION_FILE);
        let (session, session_problem) = match std::fs::read_to_string(&session_path) {
            Ok(text) => match Session::parse(&text) {
                Ok(session) => (Some(session), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "The last session could not be read ({}: {error}); it starts empty",
                        session_path.display()
                    )),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(error) => (
                None,
                Some(format!(
                    "The last session could not be read ({}: {error}); it starts empty",
                    session_path.display()
                )),
            ),
        };
        let checking = cli.screenshot.is_some();
        let variable = |name: &str| checking.then(|| std::env::var(name).ok()).flatten();
        let screenshot_actions =
            crate::dev::screenshot_actions(variable("BT_SCREENSHOT_ACTIONS").as_deref())?;
        let screenshot_tabs =
            crate::dev::screenshot_tabs(variable("BT_SCREENSHOT_TABS").as_deref())?;
        let screenshot_explain =
            crate::dev::screenshot_explain(variable("BT_SCREENSHOT_EXPLAIN").as_deref())?;
        Ok(Startup {
            files: cli.files,
            screenshot: cli.screenshot,
            screenshot_actions,
            screenshot_tabs,
            screenshot_explain,
            bench: cli.bench,
            settings,
            session,
            session_problem,
            session_path,
            settings_path,
            recovery_folder: data.join(RECOVERY_FOLDER),
            lock_folder: None,
            live_folder: None,
            pid: std::process::id(),
            outcome: RunOutcome::default(),
        })
    }

    /// How the run ends, for `main` to read once the window has closed.
    pub(crate) fn outcome(&self) -> RunOutcome {
        self.outcome.clone()
    }
}

/// Whether a run that checks something (`--screenshot`, `--bench`) failed, and why; shared
/// between the window and `main`.
#[derive(Clone, Debug, Default)]
pub(crate) struct RunOutcome {
    failure: Arc<Mutex<Option<String>>>,
}

impl RunOutcome {
    /// Records that the run failed.
    pub(crate) fn record_failure(&self, message: impl Into<String>) {
        let mut failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
        failure.get_or_insert(message.into());
    }

    /// Why the run failed, if it did.
    pub(crate) fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir()
            .join("better-tiled-startup-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn an_invalid_settings_file_stops_the_start() {
        let data = scratch("settings");
        std::fs::write(data.join(SETTINGS_FILE), "no_such_setting = 1\n").unwrap();
        let error = Startup::from_folder(Cli::default(), &data).err().unwrap();
        assert!(error.contains("settings.toml"), "{error}");
    }

    #[test]
    fn an_unreadable_session_is_reported_and_left_out() {
        let data = scratch("session");
        std::fs::write(data.join(SESSION_FILE), "{ not json").unwrap();
        let startup = Startup::from_folder(Cli::default(), &data).unwrap();
        assert!(startup.session.is_none());
        assert!(startup.session_problem.unwrap().contains("session.json"));
        let fresh = scratch("fresh");
        let startup = Startup::from_folder(Cli::default(), &fresh).unwrap();
        assert!(startup.session.is_none() && startup.session_problem.is_none());
        assert_eq!(startup.recovery_folder, fresh.join("recovery"));
    }

    #[test]
    fn the_first_failure_is_kept() {
        let outcome = RunOutcome::default();
        assert_eq!(outcome.failure(), None);
        outcome.clone().record_failure("first");
        outcome.record_failure("second");
        assert_eq!(outcome.failure().as_deref(), Some("first"));
    }
}
