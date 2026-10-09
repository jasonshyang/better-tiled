//! Custom properties as files state them.

use super::scalar::Scalar;

/// A custom property, a class member or a list item as a file states it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawProperty {
    /// The name; empty for a list item.
    pub(crate) name: String,
    /// The type name (`int`, `class`…); `None` when the file gives none (a JSON class member,
    /// or a JSON value without `type`).
    pub(crate) type_name: Option<String>,
    /// The custom type the value is of (`propertytype`).
    pub(crate) custom_type: Option<String>,
    /// The value.
    pub(crate) value: RawValue,
}

/// The value of a [`RawProperty`].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum RawValue {
    /// One value: the `value` attribute or the element's text, or a JSON scalar.
    Scalar(Scalar),
    /// A class value's members: typed in XML, untyped in JSON.
    Members(Vec<RawProperty>),
    /// A list's items, each typed.
    Items(Vec<RawProperty>),
    /// No value: a JSON `null`, or nothing at all.
    #[default]
    Absent,
}
