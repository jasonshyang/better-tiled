//! JSON laid out as Tiled's `JsonWriter` writes maps, tilesets and templates
//! (`plugins/json/qjsonparser/json.cpp`, Copyright 2010 Girish Ramakrishnan: "Use, modification
//! and distribution is allowed without limitation, warranty, liability or support of any
//! kind."): keys sorted, nested objects opening on their own line, `"key":value`, array items
//! wrapped after every `wrap` items, `/` and everything beyond ASCII escaped, reals with 15
//! significant digits, no line break at the end.

use std::fmt::Write;

use serde_json::Value as Json;

use crate::numbers::real_g15;

/// `value` with auto-formatting; arrays break after every `wrap` items (0: never).
pub(crate) fn tiled_layout(value: &Json, wrap: usize) -> String {
    let mut out = String::new();
    write_formatted(&mut out, value, 0, wrap);
    out
}

/// `value` without any whitespace (Tiled's "Minimize output").
pub(crate) fn minimized_layout(value: &Json) -> String {
    let mut out = String::new();
    write_minimized(&mut out, value);
    out
}

fn write_formatted(out: &mut String, value: &Json, depth: usize, wrap: usize) {
    match value {
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                    if wrap != 0 && i % wrap == 0 {
                        out.push('\n');
                        push_indent(out, depth);
                    } else {
                        out.push(' ');
                    }
                }
                write_formatted(out, item, depth + 1, wrap);
            }
            out.push(']');
        }
        Json::Object(map) => {
            if depth != 0 {
                out.push('\n');
                push_indent(out, depth);
                out.push_str("{\n");
            } else {
                out.push('{');
            }
            for (i, (key, value)) in map.iter().enumerate() {
                if i != 0 {
                    out.push_str(",\n");
                }
                push_indent(out, depth);
                out.push_str(" \"");
                escape_into(out, key);
                out.push_str("\":");
                write_formatted(out, value, depth + 1, wrap);
            }
            out.push('\n');
            push_indent(out, depth);
            out.push('}');
        }
        scalar => write_scalar(out, scalar),
    }
}

fn write_minimized(out: &mut String, value: &Json) {
    match value {
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i != 0 {
                    out.push(',');
                }
                write_minimized(out, item);
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
                write_minimized(out, value);
            }
            out.push('}');
        }
        scalar => write_scalar(out, scalar),
    }
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth * 4 {
        out.push(' ');
    }
}

fn write_scalar(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Json::Number(number) => match (number.as_i64(), number.as_u64(), number.as_f64()) {
            (Some(int), _, _) => {
                let _ = write!(out, "{int}");
            }
            (None, Some(int), _) => {
                let _ = write!(out, "{int}");
            }
            (None, None, Some(real)) if real.is_finite() => out.push_str(&real_g15(real)),
            _ => out.push_str("null"),
        },
        Json::String(text) => {
            out.push('"');
            escape_into(out, text);
            out.push('"');
        }
        Json::Array(_) | Json::Object(_) => out.push_str("null"),
    }
}

/// `JsonWriter`'s escaping: `/` is escaped and everything beyond ASCII becomes `\uXXXX` (UTF-16
/// units), so files are ASCII. Other control characters are escaped too, where Tiled writes them
/// raw, which is not valid JSON.
fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            c if u32::from(c) < 0x20 || u32::from(c) > 127 => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
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
    fn layout_follows_tileds_json_writer() {
        let value = json!({
            "b": "x/é\n",
            "a": [1, 2, 3],
            "list": [{"k": 0.5}, {}],
            "empty": [],
            "real": 2.0 / 3.0,
            "whole": 32.0,
        });
        assert_eq!(
            tiled_layout(&value, 2),
            "{ \"a\":[1, 2,\n    3],\n \"b\":\"x\\/\\u00e9\\n\",\n \"empty\":[],\n \"list\":[\n        {\n         \"k\":0.5\n        }, \n        {\n\n        }],\n \"real\":0.666666666666667,\n \"whole\":32\n}"
        );
        assert_eq!(tiled_layout(&json!("😀"), 0), "\"\\ud83d\\ude00\"");
        assert_eq!(
            minimized_layout(&json!({"b": [1, 2], "a": {"c": true}})),
            "{\"a\":{\"c\":true},\"b\":[1,2]}"
        );
    }
}
