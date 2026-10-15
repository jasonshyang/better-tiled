//! Tiled's file formats for better-tiled's model: maps, tilesets and templates in XML and JSON,
//! worlds, projects, legacy object types, stamps, rule lists and clipboard fragments.
//!
//! Reading is lenient: a file fails to load only when it cannot be read, is not well-formed, or
//! is not the kind asked for; every other problem is a typed [`Warning`] next to what was read.
//! Writing gives the bytes Tiled would write, so a file Tiled saved comes back unchanged.
//!
//! The XML reader and writer follow Tiled's `MapReader` and `MapWriter`
//! (`src/libtiled/mapreader.cpp`, `mapwriter.cpp`), and the JSON ones its variant converters,
//! all from libtiled (BSD-2-Clause, Copyright 2008-2026 Thorbjørn Lindeijer and contributors).

mod automap_tiles;
mod data;
mod deflate;
mod error;
mod export;
mod finish;
mod format;
mod fragment;
mod gid;
mod json;
mod loader;
mod names;
mod numbers;
mod object_types;
mod paths;
mod project;
mod raw;
mod rebase;
mod rules;
mod stamp;
mod text;
mod unfinish;
mod warning;
mod world;
mod write;
mod xml;

pub use automap_tiles::builtin_automap_tiles;
pub use error::Error;
pub use export::ExportOptions;
pub use format::Format;
pub use fragment::fragment_text;
pub use loader::{Loaded, Loader, LoaderBuilder};
pub use object_types::{load_object_types, parse_object_types};
pub use project::{load_project, parse_project};
pub use rebase::rebase_file_references;
pub use rules::{IncludeCycle, RuleMapEntry, RulesFile, parse_rules_file, read_rules_file};
pub use stamp::{Stamp, StampVariation};
pub use warning::{Target, Warning, WarningKind};
pub use world::{resolve_world_maps, world_pattern_problem};
pub use write::{Rendered, WriteRequest};
