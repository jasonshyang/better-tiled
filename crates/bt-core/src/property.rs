//! Custom properties.

use std::collections::BTreeMap;

use crate::color::Color;
use crate::ids::ObjectId;

/// Custom properties by name, sorted as Tiled writes them.
pub type Properties = BTreeMap<String, Property>;

/// A property's value and the custom type it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    /// The value, in the form the file stores it.
    pub value: Value,
    /// The class or enum the value is of (Tiled's `propertytype`), kept even when the project
    /// that defines it is not at hand.
    pub custom_type: Option<String>,
}

impl Property {
    /// A value of a built-in type.
    pub fn new(value: Value) -> Property {
        Property {
            value,
            custom_type: None,
        }
    }

    /// A value of the custom type `custom_type`.
    pub fn typed(value: Value, custom_type: impl Into<String>) -> Property {
        Property {
            value,
            custom_type: Some(custom_type.into()),
        }
    }
}

/// A property value, in the form the file stores it: an enum value as its name or number, a
/// class value with only the members that are set.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Text.
    String(String),
    /// A whole number.
    Int(i64),
    /// A real number.
    Float(f64),
    /// Yes or no.
    Bool(bool),
    /// A colour; `None` is Tiled's empty colour.
    Color(Option<Color>),
    /// A file, as written (relative to the containing file's folder).
    File(String),
    /// An object of the map; `ObjectId(0)` is none.
    Object(ObjectId),
    /// The members of a class value that are set.
    Class(Properties),
    /// A list; each item has its own type.
    List(Vec<Property>),
}

impl Value {
    /// Tiled's name of the value's type.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "string",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Color(_) => "color",
            Value::File(_) => "file",
            Value::Object(_) => "object",
            Value::Class(_) => "class",
            Value::List(_) => "list",
        }
    }
}
