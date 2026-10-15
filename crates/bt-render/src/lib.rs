//! better-tiled's renderer: a [`bt_core::Map`] drawn on the GPU through a camera, with Tiled's
//! placement, draw order and layer effects on every orientation, for the editor's canvas (through
//! a paint callback) and for headless pictures ([`Render`]).
//!
//! Every image is packed into the pages of a 2D-array texture; tiles are instances that name a
//! sprite, cached per chunk and merged in Tiled's draw order; animations go through a remap
//! table; the fragment shader samples texels clamped to the sprite, so pixel art stays crisp at
//! any zoom without bleeding. Lines, shapes and text are drawn by the same renderer, so the
//! canvas and the pictures are the same picture.

mod chunks;
mod compositor;
mod draw_list;
mod error;
mod grid;
mod headless;
mod impostors;
mod input;
mod instance;
mod map_images;
mod objects;
mod pages;
mod renderer;
mod sprites;
mod stats;
mod supply;
mod text;
mod vector;

pub use bt_image::image_request;
pub use error::RenderError;
pub use grid::{GridStyle, grid_vectors};
pub use headless::{HeadlessDevice, Picture, Render};
pub use input::{Camera, FrameInput, LayerVisibility, ObjectStyle, PreviewTile};
pub use map_images::MapImages;
pub use renderer::{MapRenderer, MapRendererBuilder};
pub use stats::RenderStats;
pub use supply::{ImageSupply, NestedSupply};
pub use text::{TextItem, TextStyle};
pub use vector::{Dash, Placement, Stroke, VectorItem, VectorShape};
pub use wgpu;
