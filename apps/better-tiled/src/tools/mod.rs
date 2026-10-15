//! The canvas's tools: one type per tool, each a state machine over the canvas's gestures that
//! previews through overlays and edits by sending intents (`06-application.md`, §6).

mod brush;
mod carry_drag;
mod edit_polygons;
mod eraser;
mod explain;
mod fills;
mod insert_objects;
mod insert_polygon;
mod insert_template;
mod magic_wand;
mod offset_layers;
mod painting;
mod picker;
mod rect_select;
mod select_objects;
mod selecting;
mod terrain;
mod tool;
mod toolbox;
mod world;

pub(crate) use selecting::region_polygons;
pub(crate) use tool::{Overlay, ToolContext};
pub(crate) use toolbox::ToolBox;
