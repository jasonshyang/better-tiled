//! The Properties panel's side of the editor (`docs/design/05-editor-core.md`, §14): what a
//! document's panel inspects (Tiled's current objects, `reference/tiled/src/tiled/document.cpp`,
//! `mapdocument.cpp` and `tilesetdocument.cpp`), the rows it shows, and its changes applied to
//! what it inspects as transactions, on the map's asset or on the tileset's home.

mod changes;
mod inspected;
mod rows;
mod source;

pub use inspected::Inspected;
