//! The primitives several of better-tiled's crates need, each defined once: rectangles, seeded
//! randomness, rasterised lines and ellipses, sets of cells as banded rectangles, hashes, atomic
//! file writes, path helpers and text helpers. Nothing here reads configuration, keeps state
//! between calls or depends on another crate of the workspace.

mod error;

pub mod atomic_write;
pub mod bands;
pub mod hash;
pub mod paths;
pub mod raster;
pub mod rect;
pub mod rng;
pub mod text;

pub use error::{Error, NotPrivate, Refused};
pub use rect::{DRect, IRect};
