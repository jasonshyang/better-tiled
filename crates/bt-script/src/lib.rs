//! The library behind `bt` and the editor's live link (`docs/design/10-agents.md`): the names,
//! shapes and feet that let an agent speak of a map without tile ids, the script language and
//! the runner that applies a script all or nothing, the checks, the text views, and the typed
//! outcomes every command returns, laid out for reading through one report model.
//!
//! It works on maps in memory and knows neither the editor nor sockets: `bt` loads maps from
//! files and the editor hands the live link its own copy, and both run scripts through this
//! crate, so an edit made in the editor is the same edit, with the same report. It runs
//! `bt-edit`, `bt-terrain` and `bt-automap`, which port Tiled's editor code, and is
//! GPL-3.0-or-later with them.

mod context;
mod error;
mod export;
mod history;
mod names;
mod report;
mod script;
mod words;

pub use context::{Context, Selection};
pub use error::{Error, NameKind};
pub use export::{ExportFormat, ExportOutcome};
pub use history::{EditToKeep, History, HistoryError, KEPT_PER_MAP, KeptEdit, fingerprint};
pub use names::{
    Ground, GroupPart, MaterialPart, NamedTerrain, Names, Piece, PieceObjects, Sprite, TileGroup,
    object_type,
};
pub use report::{
    JsonError, Line, LineKind, Outcome, Report, Section, from_json, to_json, to_json_pretty,
};
pub use script::{
    Amount, Area, Assignment, Binding, CopySource, Drawing, Flip, Geometry, IdRange, LayerOp,
    LayerPlace, LayerRef, Layers, LayersOp, Legend, LegendEntry, Located, Named, Nearness,
    ObjectOptions, ObjectRef, ObjectsWhere, Op, ParseError, Placement, Script, Shape, SpritePart,
    TerrainPart, TileIds, parse_area, parse_cell, parse_definitions, parse_extent, parse_point,
    parse_region, parse_script, parse_size, split_do,
};
pub use words::{capped_list, cell_text, cells_text, number_text, point_text, region_text};
