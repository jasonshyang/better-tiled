//! Properties as every report lists them: by name, each value as a script writes it after `key=`
//! ([`crate::words::value_text`]), so what a report shows can be typed back (`10-agents.md`,
//! §8.5).

use std::collections::BTreeMap;

use bt_core::Properties;

use crate::words::{quoted_word, value_text};

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
        .map(|(key, value)| format!("{}={value}", quoted_word(key)))
        .collect();
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use bt_core::{Color, ObjectId, Property, Value};

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
