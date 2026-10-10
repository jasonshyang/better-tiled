//! The script language (`10-agents.md`, §7): one operation per line, parsed for its syntax only;
//! names resolve when the script runs.

mod area;
mod refs;

pub use area::{Area, Nearness, Shape};
pub use refs::{IdRange, LayerRef, Named, ObjectRef, TileIds};
