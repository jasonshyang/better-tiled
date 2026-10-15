//! Tiled property values as Godot values, for metadata.

use std::collections::HashMap;
use std::path::Path;

use bt_core::{Property, Value};

use super::{res_path, text};

/// What a value needs to know: files inside the project become `res://` paths, object references
/// the path to the object's node.
pub(super) struct Values<'a> {
    pub project: &'a Path,
    pub map_dir: &'a Path,
    /// Object id → its node's path from the root.
    pub object_paths: &'a HashMap<u32, String>,
}

impl Values<'_> {
    /// A property as a Godot value, for the node at `from`.
    pub fn value(&self, p: &Property, from: &str) -> String {
        match &p.value {
            Value::String(s) => text::string(s),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => text::float(*f),
            Value::Bool(b) => b.to_string(),
            Value::Color(c) => text::color(c.unwrap_or(bt_core::Color::rgba(0, 0, 0, 0))),
            Value::File(f) => {
                let file = self.map_dir.join(f);
                text::string(
                    &res_path(self.project, &file)
                        .filter(|_| !f.is_empty())
                        .unwrap_or_else(|| f.clone()),
                )
            }
            Value::Object(id) => match self.object_paths.get(&id.0) {
                Some(to) => format!("NodePath({})", text::string(&relative_node_path(from, to))),
                None => id.to_string(),
            },
            Value::Class(members) => {
                let fields: Vec<String> = members
                    .iter()
                    .map(|(k, v)| format!("{}: {}", text::string(k), self.value(v, from)))
                    .collect();
                format!("{{{}}}", fields.join(", "))
            }
            Value::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|v| self.value(v, from))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The path from the node at `from` to the node at `to` (both from the root, `.` for the root).
pub(super) fn relative_node_path(from: &str, to: &str) -> String {
    let split = |p: &str| -> Vec<String> {
        if p == "." {
            Vec::new()
        } else {
            p.split('/').map(str::to_owned).collect()
        }
    };
    let (from, to) = (split(from), split(to));
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len() - common];
    parts.extend(to.iter().skip(common).cloned());
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}
