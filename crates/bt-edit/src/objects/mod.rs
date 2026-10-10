//! Object commands, for every object container: an object layer of a map, or a tile's collision
//! shapes (and a template's object, as a list of one). The operations act on one list of
//! objects, an `ObjectLayer` value, with the [`space::ObjectSpace`] it lives in; the map-level
//! entry points find objects by id across the map's layers and run them layer by layer.
//!
//! All positions are pixel space relative to the objects' layer; the caller removes layer
//! offsets and parallax. Isometric pixel space is the unprojected grid, so a screen rectangle
//! is a quad there.

pub(crate) mod align;
pub(crate) mod bounds;
pub(crate) mod convert;
pub(crate) mod edit;
pub(crate) mod find;
pub(crate) mod hit;
pub(crate) mod order;
pub(crate) mod outline;
pub(crate) mod polygon;
pub(crate) mod snap;
pub(crate) mod space;
pub(crate) mod transform;
