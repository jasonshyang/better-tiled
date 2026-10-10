//! Terrain painting and filling with Tiled's terrain sets (Wang sets), with results equal to
//! Tiled's and the reason for every cell no tile fits.
//!
//! A [`Terrain`] is a terrain set prepared once per tileset version. [`paint`] is the terrain
//! brush (corners, edges or whole tiles as hard constraints, neighbours re-solved out to the
//! set's maximum terrain distance) and [`fill`] the terrain fill (cells get terrain that connects
//! to the unchanged tiles around them); [`preview_paint`] and [`preview_fill`] return what they
//! would place without placing it, and with the same seed that is exactly what they place.
//! Every random choice is a function of the seed and the cell. A cell no tile fits is left as it
//! was and reported with what it needed ([`Needed`], [`Cause`]).
//!
//! This crate ports Tiled's `WangFiller` and `WangBrush` (`reference/tiled/src/tiled/
//! wangfiller.cpp`, `wangbrush.cpp`) and the parts of libtiled's `WangSet` the solver needs
//! (`reference/tiled/src/libtiled/wangset.cpp`). Tiled's editor code is GPL-2.0-or-later and is
//! used here under GPL-3.0-or-later. `docs/design/07-terrain.md` is the design.

pub mod autolabel;
mod brush;
mod color;
mod diagnose;
mod error;
mod feedback;
mod filler;
mod lanes;
pub mod layouts;
#[cfg(test)]
mod oracle_tests;
mod paint;
pub mod patterns;
mod pixels;
mod request;
pub mod rpg_maker;
mod terrain;
mod topology;
mod wangid;

pub use brush::{Stroke, hit_test, line_targets};
pub use color::TerrainColor;
pub use diagnose::{Cause, Needed};
pub use error::TerrainError;
pub use feedback::{
    Pattern, PushedBack, ReplacedSet, TerrainName, fill_note, missing_text, pushed_back,
    pushed_back_text, replaced_sets, replaced_text,
};
pub use paint::{fill, paint, preview_fill, preview_paint, write_preview};
pub use request::{BrushTarget, FillRequest, PaintRequest, Preview, Report};
pub use terrain::Terrain;
