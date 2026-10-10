//! The tileset editor's terrain helpers (`06-application.md`, §8, on `bt-terrain`'s labelling
//! helpers, `07-terrain.md`, §9–§12): blocks of autotile layouts labelled at once (Layout…:
//! Apply and Label All). What reads the tileset's pixels runs on a worker.

mod image_job;
mod layouts;

pub use image_job::{FoundLayouts, TilesetImageJob, TilesetImageRead};
pub(crate) use layouts::LayoutSearches;
pub use layouts::{LayoutSearch, LayoutTarget};
