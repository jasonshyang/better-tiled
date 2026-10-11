//! Custom properties as the resolved JSON writes them: resolved through the project's custom
//! types as Tiled's "resolve types and properties" export option resolves them (an element's
//! class members under its own properties, an object's tile's properties too), and each value
//! as plain JSON.

use bt_core::{ClassUsage, Color, CustomTypes, MapObject, Properties, TileData, Value};
use serde_json::json;

use super::document::JsonProperties;

/// The properties of an element of class `class` used as `usage`: its class's members, then
/// its own.
pub(crate) fn resolved_properties(
    types: &CustomTypes,
    class: &str,
    usage: ClassUsage,
    own: &Properties,
) -> JsonProperties {
    properties_json(&types.resolved_properties(class, usage, own))
}

/// The properties of an object: its class's members (its own class, else its tile's), then its
/// tile's properties, then its own. A loaded template instance already holds its template's.
pub(crate) fn object_properties(
    types: &CustomTypes,
    object: &MapObject,
    tile: Option<&TileData>,
) -> JsonProperties {
    properties_json(&types.object_properties(object, tile, None))
}

/// Properties as JSON, by name.
fn properties_json(properties: &Properties) -> JsonProperties {
    properties
        .iter()
        .map(|(name, property)| (name.clone(), value_json(&property.value)))
        .collect()
}

/// A value as JSON: text, a number, a boolean, a colour's text (null for Tiled's empty colour),
/// a file's path, `{"object": ID}`, a class value's members, a list's values.
fn value_json(value: &Value) -> serde_json::Value {
    match value {
        Value::String(text) | Value::File(text) => json!(text),
        Value::Int(number) => json!(number),
        // A number JSON cannot hold (infinity, NaN) is null.
        Value::Float(number) => json!(number),
        Value::Bool(flag) => json!(flag),
        Value::Color(color) => color.map_or(serde_json::Value::Null, |c| json!(color_text(c))),
        Value::Object(id) => json!({ "object": id.0 }),
        Value::Class(members) => serde_json::Value::Object(properties_json(members)),
        Value::List(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|property| value_json(&property.value))
                .collect(),
        ),
    }
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque: alpha last, as CSS and Godot write colours.
pub(crate) fn color_text(color: Color) -> String {
    if color.a == 255 {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color.r, color.g, color.b, color.a
        )
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{ClassType, CustomType, ObjectId, Property};

    use super::*;

    #[test]
    fn values_are_plain_json_and_classes_add_their_members() {
        let mut own = Properties::new();
        own.insert("speed".into(), Property::new(Value::Float(1.5)));
        own.insert(
            "tint".into(),
            Property::new(Value::Color(Some(Color::rgba(255, 0, 16, 128)))),
        );
        own.insert("none".into(), Property::new(Value::Color(None)));
        own.insert("target".into(), Property::new(Value::Object(ObjectId(7))));
        own.insert(
            "zones".into(),
            Property::new(Value::List(vec![Property::new(Value::String("a".into()))])),
        );
        let mut types = CustomTypes::new();
        let mut members = Properties::new();
        members.insert("hp".into(), Property::new(Value::Int(10)));
        members.insert("speed".into(), Property::new(Value::Float(9.0)));
        types.add(CustomType::class(
            "Enemy",
            ClassType {
                members,
                ..ClassType::default()
            },
        ));
        let resolved = resolved_properties(&types, "Enemy", ClassUsage::LAYER, &own);
        assert_eq!(resolved["hp"], json!(10), "the class's member");
        assert_eq!(
            resolved["speed"],
            json!(1.5),
            "the element's own value wins"
        );
        assert_eq!(resolved["tint"], json!("#ff001080"));
        assert_eq!(resolved["none"], serde_json::Value::Null);
        assert_eq!(resolved["target"], json!({"object": 7}));
        assert_eq!(resolved["zones"], json!(["a"]));
        assert_eq!(color_text(Color::rgb(1, 2, 255)), "#0102ff");
    }
}
