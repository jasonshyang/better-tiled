//! The resolved JSON (`10-agents.md`, §10.8): a map written for other programs as plain JSON,
//! what `bt export MAP OUT.json --resolved` writes. Nothing in it needs Tiled's arithmetic:
//! cells hold keys into a list of tiles (no global ids), flips are booleans, tilesets are written
//! into it, objects made from templates are written whole with what they inherit (Tiled's
//! export options: tilesets embedded, templates detached, types and properties resolved, the
//! last through the project's custom types), tiles carry their sprite's and terrains' names as
//! bt gives them, every cell's centre and every object's screen position and outline are said
//! on every orientation, and every path is relative to the JSON file. Oblique maps are refused.
//!
//! `schema.json` is its JSON Schema (2020-12), [`RESOLVED_SCHEMA`], which `bt export --schema`
//! prints; the tests check every export against it. [`ResolvedRequest`] makes the JSON as serde
//! types (`document`) in one walk over the map: its grid (`grid`), layers (`layers`), objects
//! (`objects`, through the object geometry the window draws with), tiles and tilesets
//! (`palette`), laid out like Tiled's JSON with rows of numbers on one line (`layout`).
//! AutoMap's rule tiles mean something only in rule maps: their cells are written empty, their
//! objects and their tileset are left out, and a warning says so.

mod definition;
mod document;
mod error;
mod grid;
mod layers;
mod layout;
mod objects;
mod palette;
mod request;
mod resolving;
#[cfg(test)]
mod tests;
mod values;

pub use error::{ResolvedError, ResolvedWarning};
pub use request::ResolvedRequest;

/// The format the resolved JSON's `format` field names.
pub const RESOLVED_FORMAT: &str = "bt resolved map 1";

/// The resolved JSON's JSON Schema (2020-12), as `bt export --schema` prints it.
pub const RESOLVED_SCHEMA: &str = include_str!("schema.json");
