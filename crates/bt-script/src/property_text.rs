//! How property values read in every report: as a script writes them after `key=`, so what a
//! report shows can be typed back (`10-agents.md`, §8.5). Text is quoted when it has spaces, a
//! number has a `.` even when it is whole, an object is `#ID`, a colour Tiled's `#rrggbb` (with
//! its alpha first when it has one).

use std::collections::BTreeMap;

use bt_core::{Properties, Value};

/// A value as a script writes it.
pub(crate) fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) | Value::File(text) => quoted(text),
        Value::Int(number) => number.to_string(),
        Value::Float(number) if number.is_finite() && number.fract() == 0.0 => {
            format!("{number:.1}")
        }
        Value::Float(number) => number.to_string(),
        Value::Bool(yes) => yes.to_string(),
        Value::Color(Some(color)) => color.to_tiled_text(),
        Value::Color(None) => "\"\"".to_owned(),
        Value::Object(id) => format!("#{}", id.0),
        Value::Class(members) => {
            let inner: Vec<String> = members
                .iter()
                .map(|(key, member)| format!("{key}={}", value_text(&member.value)))
                .collect();
            format!("{{{}}}", inner.join(" "))
        }
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(|item| value_text(&item.value)).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

/// `text`, or `"text"` when it is empty or holds spaces, quotes, `=` or `#`, as scripts read
/// words.
pub(crate) fn quoted(text: &str) -> String {
    let plain = !text.is_empty()
        && !text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '=' | '#' | ';'));
    if plain {
        text.to_owned()
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Every property by name, its value as a script writes it.
pub(crate) fn properties_text(properties: &Properties) -> BTreeMap<String, String> {
    properties
        .iter()
        .map(|(key, property)| (key.clone(), value_text(&property.value)))
        .collect()
}

/// Properties as `key=value` words, in the order of their names.
pub(crate) fn assignments_text(properties: &BTreeMap<String, String>) -> String {
    let words: Vec<String> = properties
        .iter()
        .map(|(key, value)| format!("{}={value}", quoted(key)))
        .collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use bt_core::{Color, ObjectId, Property};

    use super::*;

    #[test]
    fn values_read_as_scripts_write_them() {
        assert_eq!(value_text(&Value::String("a pond".into())), "\"a pond\"");
        assert_eq!(value_text(&Value::String("golem".into())), "golem");
        assert_eq!(value_text(&Value::String(String::new())), "\"\"");
        assert_eq!(value_text(&Value::Int(3)), "3");
        assert_eq!(value_text(&Value::Float(2.0)), "2.0");
        assert_eq!(value_text(&Value::Float(2.5)), "2.5");
        assert_eq!(value_text(&Value::Bool(true)), "true");
        assert_eq!(value_text(&Value::Object(ObjectId(12))), "#12");
        assert_eq!(
            value_text(&Value::Color(Some(Color::rgb(255, 0, 16)))),
            "#ff0010"
        );
        let mut members = Properties::new();
        members.insert("hp".into(), Property::new(Value::Int(4)));
        assert_eq!(value_text(&Value::Class(members.clone())), "{hp=4}");
        assert_eq!(
            assignments_text(&properties_text(&members)),
            "hp=4",
            "properties as key=value words"
        );
    }
}
