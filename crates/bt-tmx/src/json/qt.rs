//! JSON laid out as `QJsonDocument::toJson` writes it, which Tiled uses for projects, worlds
//! and object types: four spaces a level, `"key": value`, one item per line, empty containers
//! across two lines, text as UTF-8, reals in their shortest form, a line break at the end.

use std::fmt::Write;

use serde_json::Value as Json;

use crate::numbers::real_shortest;

/// `value` in `QJsonDocument`'s indented layout.
pub(crate) fn qt_layout(value: &Json) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out.push('\n');
    out
}

/// `value` in `QJsonDocument`'s compact layout (`QJsonDocument::Compact`), as Tiled writes stamps:
/// no whitespace and no line break at the end.
pub(crate) fn qt_compact_layout(value: &Json) -> String {
    let mut out = String::new();
    write_compact(&mut out, value);
    out
}

fn write_compact(out: &mut String, value: &Json) {
    match value {
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                write_compact(out, item);
            }
            out.push(']');
        }
        Json::Object(map) => {
            out.push('{');
            for (i, (key, value)) in map.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                out.push('"');
                escape_into(out, key);
                out.push_str("\":");
                write_compact(out, value);
            }
            out.push('}');
        }
        scalar => write_value(out, scalar, 0),
    }
}

fn write_value(out: &mut String, value: &Json, level: usize) {
    match value {
        Json::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(out, level + 1);
                write_value(out, item, level + 1);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            push_indent(out, level);
            out.push(']');
        }
        Json::Object(map) => {
            out.push_str("{\n");
            for (i, (key, value)) in map.iter().enumerate() {
                push_indent(out, level + 1);
                out.push('"');
                escape_into(out, key);
                out.push_str("\": ");
                write_value(out, value, level + 1);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            push_indent(out, level);
            out.push('}');
        }
        Json::Null => out.push_str("null"),
        Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Json::Number(number) => match (number.as_i64(), number.as_u64(), number.as_f64()) {
            (Some(int), _, _) => {
                let _ = write!(out, "{int}");
            }
            (None, Some(int), _) => {
                let _ = write!(out, "{int}");
            }
            (None, None, Some(real)) if real.is_finite() => out.push_str(&real_shortest(real)),
            _ => out.push_str("null"),
        },
        Json::String(text) => {
            out.push('"');
            escape_into(out, text);
            out.push('"');
        }
    }
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level * 4 {
        out.push(' ');
    }
}

/// `QJsonDocument`'s escaping: quotes, backslashes and control characters only.
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn layout_follows_qjsondocument() {
        let value = json!({"b": "x/é\n", "a": [1, 2], "empty": [], "list": [{"k": 0.5}, {}]});
        assert_eq!(
            qt_layout(&value),
            "{\n    \"a\": [\n        1,\n        2\n    ],\n    \"b\": \"x/é\\n\",\n    \"empty\": [\n    ],\n    \"list\": [\n        {\n            \"k\": 0.5\n        },\n        {\n        }\n    ]\n}\n"
        );
    }
}
