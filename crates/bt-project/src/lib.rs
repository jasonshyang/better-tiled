//! `better-tiled.json`, the project file: what better-tiled keeps about maps that Tiled's files
//! have no place for, so that those files stay exactly what Tiled writes. It holds the AutoMap
//! layers of each map (the rules, how they are bound to the map's layers, and what was painted
//! on the layers they write) and the Godot setup of the maps it covers.
//!
//! This crate is the only code that reads or writes the file. It finds the file for a map
//! (upward from the map, stopping at the repository's top and below the home folder), defines
//! the file's records and keeps every key it does not know, and performs each write as a
//! re-read under a lock followed by an atomic rename, so the editor and `bt` can write one file
//! at the same moment without losing each other's entries. What the records mean belongs to the
//! crates that use them: `bt-automap` for the AutoMap record, `bt-godot` for the Godot setup.
//!
//! The design is `docs/design/09-project-file.md`.

mod automap;
mod checksum;
mod error;
mod file;
mod godot;
pub mod paths;
pub mod rows;
mod schema;

pub use automap::{AutoMapRecord, BindingRecord, PaintedRecord, RunRecord};
pub use error::Error;
pub use rows::{RowCell, RowError};
pub use schema::{Document, MapEntry, Note};
