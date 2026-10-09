//! Problems found in files that were read anyway: typed so that the Issues panel and `bt` can
//! group them and act on them.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use bt_core::{LayerId, ObjectId, TileId};

/// A problem found while reading or writing a file that did not stop it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Warning {
    /// The file the problem is in: a map's warnings name the tileset or template file a problem
    /// came from.
    pub file: PathBuf,
    /// What kind of problem it is.
    pub kind: WarningKind,
    /// What happened, in a sentence.
    pub message: String,
    /// What it is about, when that is one thing.
    pub target: Option<Target>,
}

/// What kind of problem a [`Warning`] reports.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WarningKind {
    /// A referenced file (tileset, template, image) could not be read; a placeholder stands in.
    MissingFile,
    /// A value could not be read and its default was used.
    InvalidValue,
    /// A cell or tile object refers to a global id no tileset owns; it was left empty.
    UnknownGid,
    /// Tile layer data could not be fully decoded; what could be read was kept.
    CorruptLayerData,
    /// Two layers or objects had the same id; the later one was given a new one.
    DuplicateId,
    /// An element, attribute or key that is not part of the format; it is not kept.
    UnknownKey,
    /// A shape from an older version of the format was converted.
    LegacyShape,
    /// A terrain id that names no colour of its set; the tile's terrain was dropped.
    InvalidWangId,
    /// A world pattern that does not compile; it is kept but finds no maps.
    InvalidPattern,
    /// A property type that is not one of Tiled's; the value was read as it is.
    UnknownPropertyType,
    /// A class member that makes its class contain itself; it was dropped.
    CircularClass,
    /// A tileset's first global id was changed on writing.
    Renumbered,
}

/// What a [`Warning`] is about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A layer of the map.
    Layer(LayerId),
    /// An object of the map.
    Object(ObjectId),
    /// A tileset, by name.
    Tileset(String),
    /// A tile of a tileset.
    Tile {
        /// The tileset's name.
        tileset: String,
        /// The tile.
        tile: TileId,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.file.file_name().map_or_else(
            || self.file.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        write!(f, "{name}: {}", self.message)
    }
}

/// The warnings of one file being read or written.
#[derive(Debug)]
pub(crate) struct Warnings {
    file: PathBuf,
    list: Vec<Warning>,
}

impl Warnings {
    /// No warnings yet, about `file`.
    pub(crate) fn new(file: &Path) -> Warnings {
        Warnings {
            file: file.to_path_buf(),
            list: Vec::new(),
        }
    }

    /// Records a problem.
    pub(crate) fn record(&mut self, kind: WarningKind, message: impl Into<String>) {
        self.record_about(kind, None, message);
    }

    /// Records a problem about `target`.
    pub(crate) fn record_about(
        &mut self,
        kind: WarningKind,
        target: Option<Target>,
        message: impl Into<String>,
    ) {
        self.list.push(Warning {
            file: self.file.clone(),
            kind,
            message: message.into(),
            target,
        });
    }

    /// Takes in warnings found in another file (an external tileset or template).
    pub(crate) fn record_all(&mut self, warnings: impl IntoIterator<Item = Warning>) {
        self.list.extend(warnings);
    }

    /// The warnings, each once, in the order they were found.
    pub(crate) fn into_list(self) -> Vec<Warning> {
        deduplicated(self.list)
    }
}

/// Warnings without repeats (a cached tileset used twice reports once), first ones first.
pub(crate) fn deduplicated(warnings: Vec<Warning>) -> Vec<Warning> {
    let mut seen = HashSet::new();
    warnings
        .into_iter()
        .filter(|warning| seen.insert(warning.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_name_their_file_and_repeat_once() {
        let mut warnings = Warnings::new(Path::new("/maps/a.tmx"));
        warnings.record(WarningKind::InvalidValue, "bad opacity");
        warnings.record(WarningKind::InvalidValue, "bad opacity");
        warnings.record_about(
            WarningKind::DuplicateId,
            Some(Target::Layer(LayerId(3))),
            "layer 3 twice",
        );
        let list = warnings.into_list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].to_string(), "a.tmx: bad opacity");
        assert_eq!(list[1].target, Some(Target::Layer(LayerId(3))));
    }
}
