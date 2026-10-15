//! Builds a scene from the map, collecting diagnostics before writing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bt_core::{Map, Properties, Property, Value};

use crate::{Element, Note};
use crate::{
    contract, conventions, grid, images, names, objects, report, scene, text, tileset, values,
};
use bt_project::GodotSetup;
use grid::Grid;
use images::Images;
use names::Names;
use scene::Scene;
use tileset::TileSetBuilder;

/// A scene being built from a map: its nodes and resources, the TileSet its layers share, and the
/// images it uses. Each part of the map is built by its own module (`impl Build` blocks in
/// layers.rs, objects.rs, tileset.rs).
pub(crate) struct Build<'m> {
    pub(crate) failures: Vec<String>,
    pub(crate) map: &'m Map,
    pub(crate) grid: Grid,
    pub(crate) project: PathBuf,
    pub(crate) map_dir: PathBuf,
    pub(crate) images: Images,
    pub(crate) scene: Scene,
    pub(crate) tileset: TileSetBuilder,
    pub(crate) names: Names,
    pub(crate) object_paths: HashMap<u32, String>,
    pub(crate) frames: HashMap<(usize, u32), String>,
    pub(crate) atlas_textures: HashMap<(String, [i32; 4]), String>,
    pub(crate) notes: Vec<Note>,
    pub(crate) warned: HashSet<String>,
    /// Which tilesets are AutoMap's rule tiles, left out.
    pub(crate) rule_slots: Vec<bool>,
    /// Layers, cells and objects written so far.
    pub(crate) counts: report::Counts,
    /// The project's Godot settings (better-tiled.json).
    pub(crate) setup: GodotSetup,
    /// The property names the export gave a meaning to (kept out of metadata).
    pub(crate) consumed: conventions::Consumed,
    /// What objects with a `resPath` leave until every node is placed.
    pub(crate) instances: objects::Instances,
}

impl<'m> Build<'m> {
    pub(crate) fn new(map: &'m Map, grid: Grid, images: Images) -> Build<'m> {
        let project = images.project().to_path_buf();
        let map_dir = images.map_folder().to_path_buf();
        Build {
            failures: Vec::new(),
            map,
            grid,
            project,
            map_dir,
            images,
            scene: Scene::default(),
            tileset: TileSetBuilder::default(),
            names: Names::default(),
            object_paths: HashMap::new(),
            frames: HashMap::new(),
            atlas_textures: HashMap::new(),
            notes: Vec::new(),
            warned: HashSet::new(),
            rule_slots: Vec::new(),
            counts: report::Counts::default(),
            setup: GodotSetup::default(),
            consumed: conventions::Consumed::default(),
            instances: objects::Instances::default(),
        }
    }

    /// A warning, once however often it comes up.
    pub(crate) fn warn_once(&mut self, code: crate::Code, message: String, elements: Vec<Element>) {
        if self.warned.insert(message.clone()) {
            self.notes.push(Note::warning(code, message, elements));
        }
    }

    pub(crate) fn is_rule_slot(&self, slot: usize) -> bool {
        self.rule_slots.get(slot).copied().unwrap_or(false)
    }

    /// `ExtResource("…")` of an image.
    pub(crate) fn texture(&mut self, image: &bt_core::ImageRef) -> String {
        let res = self.images.res_image(image);
        self.scene.ext_resource("Texture2D", res)
    }

    /// Adds a sub-resource; returns `SubResource("…")`.
    pub(crate) fn sub(&mut self, kind: &str, props: Vec<(String, String)>) -> String {
        self.scene.sub_resource(kind, props)
    }

    /// Adds a node under `parent` (`None` for the root) with a name unique among its siblings.
    pub(crate) fn node(&mut self, parent: Option<&str>, name: &str, kind: &'static str) -> usize {
        let unique = self.names.unique(parent, name, kind);
        self.scene.add_node(parent, unique, kind)
    }

    pub(crate) fn prop(&mut self, node: usize, key: &str, value: String) {
        if let Some(node) = self.node_state(node) {
            node.props.push((key.to_owned(), value));
        }
    }

    /// The scene, its root named `name` and marked as made from the map `marker` names.
    pub(crate) fn build(&mut self, name: &str, marker: &str) -> Result<(), String> {
        self.notes.extend(self.grid.notes());
        let root = self.node(None, name, "Node2D");
        let uid = self.names.root();
        self.set_node_id(root, uid);
        // Pixel art: nearest-neighbor filtering, inherited by every node.
        self.prop(root, "texture_filter", "1".to_owned());
        self.prop(root, contract::MARKER, text::string(marker));
        self.map_layers(root)?;
        self.finish_objects();
        self.notes.extend(self.names.key_notes());
        if !self.failures.is_empty() {
            return Err(self.failures.join("; "));
        }
        Ok(())
    }

    /// A node's class (its group and `metadata/class`) and properties, each under a key of its
    /// own that Godot takes (names.rs). The keys the node's own metadata has (a layer's or
    /// object's id, pushed before; the root's marker; the class) stay the export's.
    pub(crate) fn meta(&mut self, node: usize, class: &str, properties: &Properties) {
        let Some(node) = self.scene.nodes.get_mut(node) else {
            self.failures
                .push("the scene lost a node while assigning metadata".to_owned());
            return;
        };
        let of = node
            .meta
            .iter()
            .find_map(|(key, p)| match (key.as_str(), &p.value) {
                ("layer_id", Value::Int(id)) => Some(Element::Layer(*id as u32)),
                ("id", Value::Int(id)) => Some(Element::Object(*id as u32)),
                _ => None,
            });
        let mut own: Vec<String> = node
            .meta
            .iter()
            .map(|(key, _)| key.clone())
            .chain(
                node.props
                    .iter()
                    .filter_map(|(key, _)| Some(key.strip_prefix("metadata/")?.to_owned())),
            )
            .collect();
        if !class.is_empty() {
            node.groups.push(class.to_owned());
            node.meta.push((
                "class".to_owned(),
                Property::new(Value::String(class.to_owned())),
            ));
            own.push("class".to_owned());
        }
        let keys = self.names.metadata_keys(of, &own, properties);
        for (property, key) in properties.values().zip(keys) {
            node.meta.push((key, property.clone()));
        }
    }

    /// The scene's text.
    pub(crate) fn write(&mut self) -> String {
        // The TileSet only when a layer uses it, and isn't shared.
        if self.tileset.shared.is_none()
            && self.scene.nodes.iter().any(|n| n.kind == "TileMapLayer")
        {
            self.scene.tileset = Some(self.tileset_sections());
        }
        let values = values::Values {
            project: &self.project,
            map_dir: &self.map_dir,
            object_paths: &self.object_paths,
        };
        self.scene
            .write(|property, from| values.value(property, from))
    }

    pub(crate) fn counts(&self) -> report::Counts {
        let tiles = self.tileset.sources.iter().map(|s| s.tiles.len()).sum();
        report::Counts {
            tiles,
            sources: self.tileset.sources.len(),
            images: self.images.copied(),
            ..self.counts.clone()
        }
    }
}

impl Build<'_> {
    pub(crate) fn node_state(&mut self, id: usize) -> Option<&mut crate::scene::Node> {
        if id >= self.scene.nodes.len() {
            self.failures.push(format!("scene node {id} is missing"));
            return None;
        }
        self.scene.nodes.get_mut(id)
    }
    pub(crate) fn node_path(&mut self, id: usize) -> String {
        self.node_state(id)
            .map(|n| n.path.clone())
            .unwrap_or_default()
    }
    pub(crate) fn node_id(&mut self, id: usize) -> Option<i32> {
        self.node_state(id).and_then(|n| n.unique_id)
    }
    pub(crate) fn set_node_id(&mut self, id: usize, uid: Option<i32>) {
        if let Some(node) = self.node_state(id) {
            node.unique_id = uid;
        }
    }
    pub(crate) fn push_node_meta(&mut self, id: usize, key: String, value: Property) {
        if let Some(node) = self.node_state(id) {
            node.meta.push((key, value));
        }
    }
}
