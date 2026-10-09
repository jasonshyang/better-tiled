//! The raw tree into the model, in one place: defaults, legacy conversions, references, global
//! ids, validation and warnings.

mod context;
mod layer;
mod map;
mod object;
mod property;
mod template;
mod tileset;
mod wang;

pub(crate) use context::Finisher;
pub(crate) use property::PropertyDecoder;
