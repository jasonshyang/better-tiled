//! What an export did (docs/design/12-godot-export.md): the files it wrote, what the scene holds, and notes on what
//! Godot will do differently from Tiled, each with a stable code and the elements it is about.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The files, scene contents and diagnostics of one export.
#[derive(Debug, Default, serde::Serialize)]
pub struct Report {
    /// The output files and what happened to each.
    pub files: Vec<File>,
    /// The content counts of the generated scene.
    pub counts: Counts,
    /// Grouped diagnostics, including expected differences in Godot.
    pub notes: Vec<Note>,
    /// Nodes whose path changed since the last export.
    pub renamed: Vec<Renamed>,
    /// The property names the export gave a meaning to, so they aren't metadata.
    pub consumed: BTreeSet<String>,
    /// Nothing was written (`--check`): the file states say what an export would do.
    pub checked: bool,
    /// The scenes that inherit or instance the export, and what Godot will do with them.
    pub level_scenes: Vec<super::inheritors::LevelScene>,
}

impl Report {
    /// The notes as lines (`--export-godot` prints them as warnings).
    pub fn warnings(&self) -> Vec<String> {
        self.notes.iter().map(|n| n.message.clone()).collect()
    }

    /// How many images were copied into the Godot project.
    pub fn copied(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.state == State::Copied)
            .count()
    }

    /// Every file was already as the export writes it (so nothing was, or would be, written).
    pub fn up_to_date(&self) -> bool {
        self.files.iter().all(|f| f.state == State::Unchanged)
    }
}

/// One output file and the action taken on it.
#[derive(Debug, serde::Serialize)]
pub struct File {
    /// The path of the file on disk.
    pub path: PathBuf,
    /// The action taken for this file.
    pub state: State,
}

/// The action taken on an output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Written or replaced by this export.
    Written,
    /// Already what the export would write: left alone.
    Unchanged,
    /// Not replaced: see the refusal.
    Refused,
    /// The file as it was, kept beside it before it was replaced.
    BackedUp,
    /// An image copied into the project.
    Copied,
    /// An image copy nothing uses any more, removed.
    Pruned,
}

/// What the scene holds.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Counts {
    /// The number of tile, object, image and group layers.
    pub layers: usize,
    /// Cells of tile layers.
    pub cells: usize,
    /// Tiles in the TileSet.
    pub tiles: usize,
    /// The TileSet's atlas sources.
    pub sources: usize,
    /// The number of terrain sets in the TileSet.
    pub terrain_sets: usize,
    /// The number of per-tile custom-data layers.
    pub custom_data_layers: usize,
    /// The number of object nodes in the scene.
    pub objects: usize,
    /// Objects written as instances of a scene (`resPath`).
    pub instances: usize,
    /// Images from outside the Godot project that the scene and its TileSet use, each copied
    /// into it.
    pub images: usize,
}

/// A grouped diagnostic and the map elements it concerns.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Note {
    /// Stable, for tools: `godot/blend-mode`, `godot/half-pixel-origin`, …
    pub code: crate::Code,
    /// Whether this is a warning or an informational note.
    pub severity: Severity,
    /// What Godot will do, and what to do about it.
    pub message: String,
    /// The layers, objects or tiles this diagnostic concerns.
    pub elements: Vec<Element>,
}

impl Note {
    /// Creates a warning associated with the given map elements.
    pub fn warning(code: crate::Code, message: String, elements: Vec<Element>) -> Note {
        Note {
            code,
            severity: Severity::Warning,
            message,
            elements,
        }
    }
}

/// The severity of an exporter diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// A difference or problem that may need attention.
    Warning,
    /// An expected behavior or information about the export.
    Info,
}

/// A part of the map a note is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Element {
    /// A layer, by id.
    Layer(u32),
    /// An object, by id.
    Object(u32),
    /// A tileset, by its slot in the map.
    Tileset(usize),
    /// A tile: its tileset's slot and its id.
    Tile {
        /// The map tileset slot.
        tileset: usize,
        /// The tile ID within that tileset.
        id: u32,
    },
}

/// A node whose path changed, by the Tiled id of what it stands for.
#[derive(Debug, serde::Serialize)]
pub struct Renamed {
    /// Element.
    pub element: Element,
    /// From.
    pub from: String,
    /// To.
    pub to: String,
}
