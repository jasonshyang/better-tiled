//! Projects (`.tiled-project` files).

use std::collections::BTreeMap;

use crate::custom_types::CustomTypes;
use crate::file_ref::FileRef;
use crate::property::Properties;

/// A project: the folders of a game's files, project-wide settings, and the custom types its
/// maps use.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// The folders shown in the project (`.` is the project's own folder).
    pub folders: Vec<FileRef>,
    /// The folder of the project's extensions (`extensions`).
    pub extensions_path: FileRef,
    /// The automapping rules every map uses, if any.
    pub automapping_rules_file: Option<FileRef>,
    /// A legacy object types file (before Tiled 1.8); loading imports it into
    /// [`Project::custom_types`] and clears it.
    pub object_types_file: Option<FileRef>,
    /// External commands.
    pub commands: Vec<Command>,
    /// The Tiled version files are saved for.
    pub compatibility_version: CompatibilityVersion,
    /// The project's classes and enums.
    pub custom_types: CustomTypes,
    /// Custom properties of the project (Tiled 1.11).
    pub properties: Properties,
    /// Keys of the project file that this version does not know, with their values as JSON
    /// text, written back as they were.
    pub unknown_keys: BTreeMap<String, String>,
}

impl Default for Project {
    fn default() -> Project {
        Project {
            folders: Vec::new(),
            extensions_path: FileRef::new("extensions", ""),
            automapping_rules_file: None,
            object_types_file: None,
            commands: Vec::new(),
            compatibility_version: CompatibilityVersion::Latest,
            custom_types: CustomTypes::default(),
            properties: Properties::new(),
            unknown_keys: BTreeMap::new(),
        }
    }
}

/// An external command (Tiled's Commands).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The name shown.
    pub name: String,
    /// The program.
    pub command: String,
    /// Its arguments, with Tiled's `%mapfile`-style placeholders.
    pub arguments: String,
    /// The folder it runs in.
    pub working_directory: String,
    /// Its shortcut, in Qt's text form (`Ctrl+Shift+R`).
    pub shortcut: String,
    /// Whether it can be run.
    pub enabled: bool,
    /// Whether the map is saved first.
    pub save_before_execute: bool,
    /// Whether its output is shown.
    pub show_output: bool,
}

impl Default for Command {
    fn default() -> Command {
        Command {
            name: String::new(),
            command: String::new(),
            arguments: String::new(),
            working_directory: String::new(),
            shortcut: String::new(),
            enabled: true,
            save_before_execute: true,
            show_output: true,
        }
    }
}

/// The Tiled version files are saved for. It decides the `version` new files get and, for 1.9,
/// the `class` spelling of tile and object classes; nothing else.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompatibilityVersion {
    /// Tiled 1.8.
    Tiled1_8,
    /// Tiled 1.9.
    Tiled1_9,
    /// Tiled 1.10.
    Tiled1_10,
    /// The current format (the default; not written to project files).
    #[default]
    Latest,
    /// Another number a project file holds, kept as it is.
    Other(i64),
}

impl CompatibilityVersion {
    /// The version a project file's number stands for (1080, 1090, 1100; 65535 is latest).
    pub fn from_number(number: i64) -> CompatibilityVersion {
        match number {
            1080 => CompatibilityVersion::Tiled1_8,
            1090 => CompatibilityVersion::Tiled1_9,
            1100 => CompatibilityVersion::Tiled1_10,
            65535 => CompatibilityVersion::Latest,
            other => CompatibilityVersion::Other(other),
        }
    }

    /// The number project files store.
    pub fn number(self) -> i64 {
        match self {
            CompatibilityVersion::Tiled1_8 => 1080,
            CompatibilityVersion::Tiled1_9 => 1090,
            CompatibilityVersion::Tiled1_10 => 1100,
            CompatibilityVersion::Latest => 65535,
            CompatibilityVersion::Other(number) => number,
        }
    }

    /// The format `version` new files get.
    pub fn version_string(self) -> &'static str {
        match self {
            CompatibilityVersion::Tiled1_8 => "1.8",
            CompatibilityVersion::Tiled1_9 => "1.9",
            CompatibilityVersion::Tiled1_10 => "1.10",
            CompatibilityVersion::Latest | CompatibilityVersion::Other(_) => "1.11",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_numbers_round_trip() {
        for number in [1080, 1090, 1100, 65535, 1110] {
            assert_eq!(CompatibilityVersion::from_number(number).number(), number);
        }
        assert_eq!(
            CompatibilityVersion::from_number(1090).version_string(),
            "1.9"
        );
        assert_eq!(CompatibilityVersion::default().version_string(), "1.11");
    }
}
