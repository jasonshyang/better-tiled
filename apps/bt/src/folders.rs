//! The folders `bt` works with besides the map's: the user's home folder (searches stop below
//! it), the data folder (where `bt edit` keeps what `bt undo` restores) and the runtime folder
//! (where project files are locked while written, and editors listen).

use std::path::PathBuf;

use anyhow::Context;

/// The folders, found once per run.
#[derive(Clone, Debug, Default)]
pub(crate) struct Folders {
    /// The user's home folder.
    pub(crate) home: Option<PathBuf>,
    /// better-tiled's data folder, when it can be found.
    pub(crate) data: Option<PathBuf>,
    /// Where project files are locked, when not the runtime folder (tests).
    pub(crate) locks: Option<PathBuf>,
}

impl Folders {
    /// The folders of this user, from the environment.
    pub(crate) fn from_env() -> Folders {
        Folders {
            home: std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(PathBuf::from),
            data: bt_utils::paths::data_dir().ok(),
            locks: None,
        }
    }

    /// Where the versions `bt undo` restores are kept.
    pub(crate) fn history(&self) -> anyhow::Result<PathBuf> {
        let data = match &self.data {
            Some(data) => data.clone(),
            None => bt_utils::paths::data_dir()?,
        };
        Ok(data.join("history"))
    }

    /// Where project files are locked while they are written.
    pub(crate) fn locks(&self) -> anyhow::Result<PathBuf> {
        match &self.locks {
            Some(locks) => Ok(locks.clone()),
            None => bt_utils::paths::runtime_dir().context("the folder that locks project files"),
        }
    }
}
