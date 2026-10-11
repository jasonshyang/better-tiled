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

mod at;
mod changes;
mod check;
mod context;
mod create;
mod diff;
mod edit_report;
mod error;
mod explain_text;
mod export;
mod feet;
pub mod grid;
mod ground;
mod history;
mod info;
mod layer_tree;
mod names;
mod objects;
mod property_text;
mod regions;
mod report;
mod rule_words;
mod run;
mod script;
mod shapes;
mod symbols;
mod tally;
mod view;
mod view_corners;
mod view_ids;
mod words;

pub use at::{CellLayer, CellOutcome, CellTile, LanesOf};
pub use changes::{
    CellChanges, LayerChanges, MapChanges, ObjectBrief, ObjectChanged, ObjectChanges, ObjectMoved,
    PropertyChange, Resized, TerrainCells,
};
pub use check::{
    CheckOutcome, GroundChange, GroundCount, LayerGrounds, MadeProblems, ObjectAt, Problem, Seam,
    StandingGroup, ZoneCover, made_by, seams,
};
pub use context::{Context, Selection};
pub use create::{Created, NewOutcome, NewRequest, Refill};
pub use diff::DiffOutcome;
pub use edit_report::{EditOutcome, LineNote, Saved};
pub use error::{Error, NameKind};
pub use explain_text::{ExplainOutcome, ExplainRequest, ExplainedRun, MissingLayer, RuleTry};
pub use export::{ExportFormat, ExportOutcome};
pub use feet::{Feet, pixel_to_point, point_to_pixel, standing_cell};
pub use ground::{GroundUnder, StandsOn};
pub use history::{
    EditToKeep, History, HistoryError, KEPT_PER_MAP, KeptEdit, UndoOutcome, Undone, fingerprint,
};
pub use info::{
    EditorCopy, InfoOutcome, InfoRequest, LayerInfo, MapRules, RuleNote, RulesInfo, RulesKind,
    TilesetInfo,
};
pub use names::{
    Ground, GroupPart, MaterialPart, NamedTerrain, Names, Piece, PieceObjects, Sprite, TileGroup,
    object_type,
};
pub use objects::{ObjectBox, ObjectInfo, ObjectsOutcome, ObjectsRequest};
pub use regions::{
    AreasOutcome, CellsShowing, ConnectedArea, GridAxis, MapEdge, NoWay, Reach, ReachOutcome,
    ReachRequest, ShapeCells, Way,
};
pub use report::{
    JsonError, Line, LineKind, Outcome, Report, Section, from_json, to_json, to_json_pretty,
};
pub use run::{
    DEFAULT_SEED, EditResult, RunError, RunFailure, RunInput, RunInputBuilder, run_script,
};
pub use script::{
    Amount, Area, Assignment, Binding, CopySource, Drawing, Flip, Geometry, IdRange, LayerOp,
    LayerPlace, LayerRef, Layers, LayersOp, Legend, LegendEntry, Located, Named, Nearness,
    ObjectOptions, ObjectRef, ObjectsWhere, Op, ParseError, Placement, Script, Shape, SpritePart,
    TerrainPart, TileIds, parse_area, parse_cell, parse_definitions, parse_extent, parse_point,
    parse_region, parse_script, parse_size, split_do,
};
pub use shapes::{area_cells, map_cells, object_cells, shape_cells};
pub use tally::NameCount;
pub use view::{ViewBy, ViewOutcome, ViewRequest};
pub use words::{capped_list, cell_text, cells_text, id_ranges_text, number_text, point_text, quoted_word, region_text, value_text};
