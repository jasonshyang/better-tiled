//! The model back into the raw tree, every value present, for the printers to lay out: global
//! ids assigned, references written relative to the target, template instances reduced to what
//! they override.

mod gids;
mod layer;
mod map;
mod object;
mod property;
mod tileset;

pub(crate) use map::Unfinisher;
pub(crate) use property::raw_properties;
