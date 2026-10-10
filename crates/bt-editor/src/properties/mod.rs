//! The property-editing model (`docs/design/05-editor-core.md`, §14): what the Properties panel
//! shows and changes for one or several things of one kind, without a window, a transaction
//! or an editor.
//!
//! Custom properties: a [`PropertySet`] of [`Target`]s (the map, layers, objects, the tileset,
//! tiles), each with the properties it keeps and what it inherits from its class, its tile or
//! its template, gives the [`Shown`] rows of everything any target has, and applies a
//! [`Change`] to a copy of the map or tileset it was read from, editing each target's own
//! properties. [`conversions`] and [`convert`] are Tiled's Convert To.
//!
//! The rules follow Tiled's Properties panel (`reference/tiled/src/tiled/propertieswidget.cpp`,
//! `variantmapproperty.cpp`, `changeproperties.cpp`), on top of `bt_core`'s custom types, which
//! already resolve what is inherited.

mod change;
mod convert;
mod error;
mod lookup;
mod set;
mod shown;
mod step;
mod target;
mod target_id;
mod value_type;

#[cfg(test)]
mod tests;

pub use change::{Change, ListEdit, Op};
pub use convert::{conversions, convert};
pub use error::PropertyError;
pub use set::PropertySet;
pub use shown::Shown;
pub use step::Step;
pub use target::{Origin, Target};
pub use target_id::TargetId;
pub use value_type::{ValueType, value_types};
