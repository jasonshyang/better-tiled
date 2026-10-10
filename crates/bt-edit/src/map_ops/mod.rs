//! Map commands: Resize Map, Crop to Selection, Autocrop, Offset Map, and making a map infinite
//! or finite. Every tile layer agrees with the map's size and bounds afterwards. Each module
//! names the Tiled sources it follows.

#[cfg(test)]
mod fuzz;
mod infinite;
mod offset;
mod outside;
mod resize;

pub use infinite::{make_finite, make_infinite};
pub use offset::offset_map;
pub use resize::{autocrop, autocrop_to_layer, crop_to, resize_map};
