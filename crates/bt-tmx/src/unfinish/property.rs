//! Custom properties back into their written form.

use bt_core::{Properties, Property, Value};

use crate::raw::{RawProperty, RawValue, Scalar};

/// Properties as written, sorted by name.
pub(crate) fn raw_properties(properties: &Properties) -> Vec<RawProperty> {
    properties
        .iter()
        .map(|(name, property)| RawProperty {
            name: name.clone(),
            ..raw_property(property)
        })
        .collect()
}

/// One property, member or list item, typed.
pub(crate) fn raw_property(property: &Property) -> RawProperty {
    let value = match &property.value {
        Value::String(text) | Value::File(text) => RawValue::Scalar(Scalar::Text(text.clone())),
        Value::Int(value) => RawValue::Scalar(Scalar::Int(*value)),
        Value::Float(value) => RawValue::Scalar(Scalar::Real(*value)),
        Value::Bool(value) => RawValue::Scalar(Scalar::Bool(*value)),
        // Colour properties are written with their alpha, as Tiled writes them.
        Value::Color(Some(color)) => RawValue::Scalar(Scalar::Text(format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color.a, color.r, color.g, color.b
        ))),
        Value::Color(None) => RawValue::Scalar(Scalar::text("")),
        Value::Object(id) => RawValue::Scalar(Scalar::Int(i64::from(id.0))),
        Value::Class(members) => RawValue::Members(raw_properties(members)),
        Value::List(items) => RawValue::Items(items.iter().map(raw_property).collect()),
    };
    RawProperty {
        name: String::new(),
        type_name: Some(property.value.type_name().to_owned()),
        custom_type: property.custom_type.clone(),
        value,
    }
}
