//! The script language (`10-agents.md`, §7): one operation per line, parsed for its syntax only;
//! names resolve when the script runs. The shape language (`area`) and the values (`values`)
//! are read the same way on `bt`'s command line.

mod area;
mod cursor;
mod op;
mod parse;
mod refs;
mod tokens;
mod values;

pub use area::{Area, Nearness, Shape, parse_area};
pub use op::{
    Amount, Assignment, Binding, CopySource, Drawing, Flip, Geometry, LayerOp, LayerPlace, Layers,
    LayersOp, Legend, LegendEntry, Located, ObjectOptions, ObjectsWhere, Op, Placement, Script,
    SpritePart, TerrainPart,
};
pub use parse::{ParseError, parse_definitions, parse_script};
pub use refs::{IdRange, LayerRef, Named, ObjectRef, TileIds};
pub use tokens::split_do;
pub use values::{parse_cell, parse_extent, parse_point, parse_region, parse_size};
