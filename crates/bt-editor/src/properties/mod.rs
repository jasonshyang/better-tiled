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
//! Built-in attributes: the rows ([`AttributeRow`]) of a map, layers, objects, a tileset or
//! tiles, and one [`Attribute`] set to an [`AttributeValue`] on each target that has it,
//! refusing values out of range.
//!
//! The rules follow Tiled's Properties panel (`reference/tiled/src/tiled/propertieswidget.cpp`,
//! `variantmapproperty.cpp`, `changeproperties.cpp`, `changemapobject.cpp`), on top of
//! `bt_core`'s custom types, which already resolve what is inherited.

mod attribute;
mod attribute_check;
mod attribute_row;
mod attribute_value;
mod change;
mod convert;
mod error;
mod layer_attributes;
mod lookup;
mod map_attributes;
mod object_attributes;
mod set;
mod shown;
mod step;
mod target;
mod target_id;
mod tile_attributes;
mod tileset_attributes;
mod value_type;

#[cfg(test)]
mod tests;

pub use attribute::Attribute;
pub use attribute_row::AttributeRow;
pub use attribute_value::AttributeValue;
pub use change::{Change, ListEdit, Op};
pub use convert::{conversions, convert};
pub use error::PropertyError;
pub use layer_attributes::{layer_attributes, set_layer_attribute};
pub use map_attributes::{map_attributes, set_map_attribute};
pub use object_attributes::{object_attributes, set_object_attribute};
pub use set::PropertySet;
pub use shown::Shown;
pub use step::Step;
pub use target::{Origin, Target};
pub use target_id::TargetId;
pub use tile_attributes::{set_tile_attribute, tile_attributes};
pub use tileset_attributes::{set_tileset_attribute, tileset_attributes};
pub use value_type::{ValueType, value_types};
