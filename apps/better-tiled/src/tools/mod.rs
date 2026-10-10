//! The canvas's tools: one type per tool, each a state machine over the canvas's gestures that
//! previews through overlays and edits by sending intents (`06-application.md`, §6).

mod brush;
mod eraser;
mod fills;
mod magic_wand;
mod painting;
mod picker;
mod rect_select;
mod selecting;
mod tool;
mod toolbox;

pub(crate) use selecting::region_polygons;
pub(crate) use tool::{Overlay, ToolContext};
pub(crate) use toolbox::ToolBox;
