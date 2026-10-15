//! A Godot scene as the export writes it: its resources and nodes, and their text (`.tscn`,
//! format 3, with Godot's own order of section attributes).

use std::collections::HashMap;
use std::fmt::Write;

use bt_core::Property;

use super::text;

/// `;` comment lines before the `[gd_scene …]` line. Godot loads a file that has them and drops
/// them when it saves the file again.
pub(super) struct Header {
    pub lines: Vec<String>,
}

/// An `[ext_resource]`: a file the scene uses.
pub(super) struct ExtResource {
    pub kind: &'static str,
    pub path: String,
    /// The file's `uid://…`, by which Godot still finds it once it moved.
    pub uid: Option<String>,
    pub id: String,
}

/// A `[sub_resource]`: its type, id and properties.
pub(super) struct SubResource {
    pub kind: String,
    pub id: String,
    pub props: Vec<(String, String)>,
}

/// A node of the scene; `path` is from the root (`.` for the root).
pub(super) struct Node {
    pub name: String,
    pub kind: &'static str,
    pub parent: Option<String>,
    pub path: String,
    /// Godot's id of the node (`unique_id=`), which survives renames in scenes that inherit this
    /// one.
    pub unique_id: Option<i32>,
    pub groups: Vec<String>,
    /// The scene the node is an instance of (`instance=ExtResource("…")`), written in place of
    /// its type.
    pub instance: Option<String>,
    /// Properties whose `NodePath` value Godot turns into the node it leads to (`node_paths=`):
    /// a script's exported `Node` variables.
    pub node_paths: Vec<String>,
    pub props: Vec<(String, String)>,
    /// Properties, written when every node's path is known (object references).
    pub meta: Vec<(String, Property)>,
}

#[derive(Default)]
pub(super) struct Scene {
    pub header: Option<Header>,
    /// The scene's `uid://…`.
    pub uid: Option<String>,
    pub ext: Vec<ExtResource>,
    /// Path → id of each `ext`.
    ext_ids: HashMap<String, String>,
    /// The sections of the TileSet kept in the scene, written before the other sub-resources.
    pub tileset: Option<String>,
    pub subs: Vec<SubResource>,
    sub_count: usize,
    pub nodes: Vec<Node>,
}

impl Scene {
    /// `ExtResource("…")` of the file at the `res://` path `path`, added once.
    pub fn ext_resource(&mut self, kind: &'static str, path: String) -> String {
        let id = match self.ext_ids.get(&path) {
            Some(id) => id.clone(),
            None => {
                let id = (self.ext.len() + 1).to_string();
                self.ext.push(ExtResource {
                    kind,
                    path: path.clone(),
                    uid: None,
                    id: id.clone(),
                });
                self.ext_ids.insert(path, id.clone());
                id
            }
        };
        format!("ExtResource(\"{id}\")")
    }

    /// Adds a sub-resource; returns `SubResource("…")`.
    pub fn sub_resource(&mut self, kind: &str, props: Vec<(String, String)>) -> String {
        self.sub_count += 1;
        let id = format!("{kind}_{}", self.sub_count);
        self.subs.push(SubResource {
            kind: kind.to_owned(),
            id: id.clone(),
            props,
        });
        format!("SubResource(\"{id}\")")
    }

    /// Adds a node named `name` (unique among its siblings) under `parent` (`None` for the root).
    pub fn add_node(&mut self, parent: Option<&str>, name: String, kind: &'static str) -> usize {
        let path = match parent {
            None => ".".to_owned(),
            Some(".") => name.clone(),
            Some(parent) => format!("{parent}/{name}"),
        };
        let parent = parent.map(str::to_owned);
        self.nodes.push(Node {
            name,
            kind,
            parent,
            path,
            unique_id: None,
            groups: Vec::new(),
            instance: None,
            node_paths: Vec::new(),
            props: Vec::new(),
            meta: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// The scene's text; `value` writes a metadata property for the node at a path.
    pub fn write(&self, value: impl Fn(&Property, &str) -> String) -> String {
        let mut out = String::new();
        if let Some(header) = &self.header {
            for line in &header.lines {
                let _ = writeln!(out, "; {line}");
            }
        }
        out.push_str("[gd_scene format=3");
        if let Some(uid) = &self.uid {
            let _ = write!(out, " uid={}", text::string(uid));
        }
        out.push_str("]\n");
        for ExtResource {
            kind,
            path,
            uid,
            id,
        } in &self.ext
        {
            let uid = uid
                .as_ref()
                .map_or_else(String::new, |uid| format!(" uid={}", text::string(uid)));
            let _ = write!(
                out,
                "\n[ext_resource type=\"{kind}\"{uid} path={} id=\"{id}\"]",
                text::string(path)
            );
        }
        if !self.ext.is_empty() {
            out.push('\n');
        }
        if let Some(tileset) = &self.tileset {
            out.push_str(tileset);
        }
        for SubResource { kind, id, props } in &self.subs {
            let _ = write!(out, "\n[sub_resource type=\"{kind}\" id=\"{id}\"]\n");
            for (key, value) in props {
                let _ = writeln!(out, "{key} = {value}");
            }
        }
        for node in &self.nodes {
            let _ = write!(out, "\n[node name={}", text::string(&node.name));
            if node.instance.is_none() {
                let _ = write!(out, " type=\"{}\"", node.kind);
            }
            if let Some(parent) = &node.parent {
                let _ = write!(out, " parent={}", text::string(parent));
            }
            if let Some(id) = node.unique_id {
                let _ = write!(out, " unique_id={id}");
            }
            if !node.node_paths.is_empty() {
                let paths: Vec<String> = node.node_paths.iter().map(|p| text::string(p)).collect();
                let _ = write!(out, " node_paths=PackedStringArray({})", paths.join(", "));
            }
            if !node.groups.is_empty() {
                let groups: Vec<String> = node.groups.iter().map(|g| text::string(g)).collect();
                let _ = write!(out, " groups=[{}]", groups.join(", "));
            }
            if let Some(instance) = &node.instance {
                let _ = write!(out, " instance={instance}");
            }
            out.push_str("]\n");
            for (key, value) in &node.props {
                let _ = writeln!(out, "{key} = {value}");
            }
            for (key, property) in &node.meta {
                let _ = writeln!(
                    out,
                    "metadata/{} = {}",
                    text::identifier(key),
                    value(property, &node.path)
                );
            }
        }
        out
    }
}
