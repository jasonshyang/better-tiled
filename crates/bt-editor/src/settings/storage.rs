//! Where settings and the rest of the editor's own files live, and reading and writing
//! `settings.toml`.

use std::path::{Path, PathBuf};

use bt_utils::atomic_write::{WriteMode, write_file};

use super::migrate::{MigrateError, migrate};
use super::settings::Settings;

/// Why the settings could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid TOML or holds a value of the wrong kind.
    #[error("{}: {message}", path.display())]
    Invalid {
        /// The file.
        path: PathBuf,
        /// What is wrong, with its line.
        message: String,
    },
    /// The file's version cannot be read.
    #[error("{}: {source}", path.display())]
    Version {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: MigrateError,
    },
    /// The file could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: bt_utils::Error,
    },
    /// No data folder can be found: the home folder is not known.
    #[error("no folder for settings: {0} is not set")]
    NoDataFolder(&'static str),
}

/// The folder of the editor's own files: `~/Library/Application Support/better-tiled` on
/// macOS, `%APPDATA%\better-tiled` on Windows, `$XDG_DATA_HOME/better-tiled` (or
/// `~/.local/share/better-tiled`) elsewhere.
pub fn data_folder() -> Result<PathBuf, SettingsError> {
    let var = |name: &'static str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        let home = var("HOME").ok_or(SettingsError::NoDataFolder("HOME"))?;
        return Ok(home.join("Library/Application Support/better-tiled"));
    }
    if cfg!(target_os = "windows") {
        let appdata = var("APPDATA").ok_or(SettingsError::NoDataFolder("APPDATA"))?;
        return Ok(appdata.join("better-tiled"));
    }
    if let Some(xdg) = var("XDG_DATA_HOME") {
        return Ok(xdg.join("better-tiled"));
    }
    let home = var("HOME").ok_or(SettingsError::NoDataFolder("HOME"))?;
    Ok(home.join(".local/share/better-tiled"))
}

impl Settings {
    /// The settings in `path`: the defaults when there is no file, an error when the file is
    /// unreadable, invalid, of a newer version, or names a setting that does not exist.
    pub fn load(path: &Path) -> Result<Settings, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(source) => {
                return Err(SettingsError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        Settings::parse(&text, path)
    }

    /// The settings in `text` (read from `path`).
    pub fn parse(text: &str, path: &Path) -> Result<Settings, SettingsError> {
        let invalid = |error: toml::de::Error| SettingsError::Invalid {
            path: path.to_path_buf(),
            message: error.to_string(),
        };
        let table: toml::Table = toml::from_str(text).map_err(invalid)?;
        let table = migrate(table).map_err(|source| SettingsError::Version {
            path: path.to_path_buf(),
            source,
        })?;
        let text = toml::to_string(&table).map_err(|error| SettingsError::Invalid {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        toml::from_str(&text).map_err(invalid)
    }

    /// Writes the settings to `path` atomically.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let text = toml::to_string_pretty(self).map_err(|error| SettingsError::Invalid {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|source| SettingsError::Write {
                path: path.to_path_buf(),
                source: bt_utils::Error::Write {
                    path: folder.to_path_buf(),
                    source,
                },
            })?;
        }
        write_file(path, text.as_bytes(), WriteMode::Atomic).map_err(|source| {
            SettingsError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use super::*;
    use crate::settings::settings::SettingColor;

    #[test]
    fn settings_default_round_trip_and_refuse_typos() {
        let scratch = Scratch::new("bt-editor-settings").unwrap();
        let path = scratch.join("settings.toml");
        assert_eq!(
            Settings::load(&path).unwrap(),
            Settings::default(),
            "no file: the defaults"
        );
        let mut settings = Settings::default();
        settings.general.safe_writing = false;
        settings.interface.grid_color = SettingColor(bt_core::Color::rgba(10, 20, 30, 128));
        settings
            .keymap
            .insert("save".into(), vec!["alt+cmd+s".into()]);
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("grid_color = \"#800a141e\""), "{text}");
        // A partial file takes defaults for the rest.
        let partial = Settings::parse("[general]\nrecovery = false\n", &path).unwrap();
        assert!(!partial.general.recovery && partial.general.restore_session);
        for bad in [
            "[general]\nrestore_sesion = true\n",
            "[interface]\ngrid_color = 'red'\n",
            "version = 7\n",
            "[[[",
        ] {
            assert!(Settings::parse(bad, &path).is_err(), "{bad}");
        }
    }
}
