//! Settings files of older versions brought to this one, a step at a time.

use super::settings::SETTINGS_VERSION;

/// Why a settings file cannot be brought up to date.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    /// A file written by a newer build.
    #[error(
        "the settings were written by a newer better-tiled (version {found}, this one reads up to {SETTINGS_VERSION})"
    )]
    Newer {
        /// The file's version.
        found: i64,
    },
    /// A version that is not a number.
    #[error("the settings' version is not a number")]
    NotANumber,
}

/// Brings a settings table to the current version. Version 1 is the first: nothing to migrate
/// yet; each later version adds a step here.
pub(crate) fn migrate(mut table: toml::Table) -> Result<toml::Table, MigrateError> {
    let version = match table.get("version") {
        None => i64::from(SETTINGS_VERSION),
        Some(toml::Value::Integer(version)) => *version,
        Some(_) => return Err(MigrateError::NotANumber),
    };
    if version > i64::from(SETTINGS_VERSION) {
        return Err(MigrateError::Newer { found: version });
    }
    table.insert(
        "version".to_owned(),
        toml::Value::Integer(i64::from(SETTINGS_VERSION)),
    );
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_checked() {
        let table: toml::Table = toml::from_str("version = 99").unwrap();
        assert_eq!(migrate(table), Err(MigrateError::Newer { found: 99 }));
        let table: toml::Table = toml::from_str("version = 'one'").unwrap();
        assert_eq!(migrate(table), Err(MigrateError::NotANumber));
        let table: toml::Table = toml::from_str("").unwrap();
        assert_eq!(
            migrate(table).unwrap().get("version"),
            Some(&toml::Value::Integer(1))
        );
    }
}
