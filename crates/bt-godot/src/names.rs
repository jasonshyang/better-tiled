//! Node identity (docs/design/12-godot-export.md): what each layer and object is called in the scene, and the
//! `unique_id` Godot 4.6 and later find it by, both from its Tiled id rather than the order
//! nodes are visited, so that adding an object never renames another. Also the rename report:
//! the nodes whose path changed since the scene on disk; and the metadata keys of properties,
//! which leave the export's own (`metadata/id`, `layer_id`, `class`, `bt_source`) to it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use bt_core::{Layer, MapObject, Properties, Shape};

use super::report::{Element, Note, Renamed, Severity};
use super::{read, text};
use crate::build::Build;

/// Objects' ids are 2^29 + their Tiled id, layers' 2^30 + theirs, and nodes standing for no
/// layer or object of their own get hashed ids from 3·2^29 up (the root is 1).
const OBJECT_IDS: i32 = 1 << 29;
const LAYER_IDS: i32 = 1 << 30;
const HASHED_IDS: i32 = 3 << 29;
/// Tiled ids from here on would run into the next range, so they are hashed.
const OWN_RANGE: u32 = 1 << 28;

/// The layer or object whose node has the `unique_id` `id`, for ids from its Tiled id (not hashed
/// ones).
pub(super) fn element(id: i32) -> Option<Element> {
    let own = |from: i32| u32::try_from(id - from).ok().filter(|&id| id < OWN_RANGE);
    if id >= LAYER_IDS {
        own(LAYER_IDS).map(Element::Layer)
    } else {
        own(OBJECT_IDS).map(Element::Object)
    }
}

/// The names and ids given so far.
#[derive(Default)]
pub(super) struct Names {
    /// Whether nodes get `unique_id`s: projects for Godot 4.6 and later (version.rs).
    pub unique_ids: bool,
    /// The names taken under each parent, so that no two siblings share one.
    siblings: HashMap<String, HashSet<String>>,
    /// Hashed ids given so far: a clash is hashed again, salted.
    hashed: HashSet<i32>,
    /// Where each layer and object went, by Tiled id, for the rename report.
    layers: BTreeMap<u32, String>,
    objects: BTreeMap<u32, String>,
    /// Properties whose metadata key isn't their name, for the report: (name, key) → the layers
    /// and objects having them (`None`: the map). Those whose name the export's own metadata
    /// has, apart.
    keys: BTreeMap<(String, String), Vec<Option<Element>>>,
    reserved: BTreeMap<(String, String), Vec<Option<Element>>>,
}

impl Names {
    /// `name` as a node name under `parent` (`None` for the root): Godot's characters only, the
    /// node's type when that leaves nothing. Layers and objects come with names unique among
    /// their siblings ([`layer_names`], [`object_names`]); anything else that clashes is
    /// numbered.
    pub fn unique(&mut self, parent: Option<&str>, name: &str, kind: &str) -> String {
        let base = text::node_name(name);
        let base = if base.is_empty() {
            kind.to_owned()
        } else {
            base
        };
        let siblings = self
            .siblings
            .entry(parent.unwrap_or("").to_owned())
            .or_default();
        let mut unique = base.clone();
        let mut n = 2;
        while siblings.contains(&unique) {
            unique = format!("{base}{n}");
            n += 1;
        }
        siblings.insert(unique.clone());
        unique
    }

    /// The root's `unique_id`.
    pub fn root(&self) -> Option<i32> {
        self.unique_ids.then_some(1)
    }

    /// The `unique_id` of the node of layer `id`.
    pub fn layer(&mut self, id: u32) -> Option<i32> {
        if id < OWN_RANGE {
            self.unique_ids.then_some(LAYER_IDS + id as i32)
        } else {
            self.helper(self.root(), "layer", id)
        }
    }

    /// The `unique_id` of the node of object `id`.
    pub fn object(&mut self, id: u32) -> Option<i32> {
        if id < OWN_RANGE {
            self.unique_ids.then_some(OBJECT_IDS + id as i32)
        } else {
            self.helper(self.root(), "object", id)
        }
    }

    /// The `unique_id` of the `n`th node of a `role` (`parallax`, `shape`, …) that the node with
    /// id `owner` has: a hash of the three, the same in every export while the owner keeps its id.
    pub fn helper(&mut self, owner: Option<i32>, role: &str, n: u32) -> Option<i32> {
        let owner = owner.filter(|_| self.unique_ids)?;
        let seed = format!("{owner}/{role}/{n}");
        let mut key = seed.clone();
        let mut salt = 0;
        loop {
            let id = HASHED_IDS + (read::fnv1a32(key.as_bytes()) % (1 << 29)) as i32;
            if self.hashed.insert(id) {
                return Some(id);
            }
            salt += 1;
            key = format!("{seed}/{salt}");
        }
    }

    /// Notes that the node of a layer or object is at `path`.
    pub fn placed(&mut self, element: Element, path: &str) {
        match element {
            Element::Layer(id) => self.layers.insert(id, path.to_owned()),
            Element::Object(id) => self.objects.insert(id, path.to_owned()),
            Element::Tileset(_) | Element::Tile { .. } => None,
        };
    }

    /// The metadata keys of `properties`, in their order, on the node of the layer or object
    /// `of` (`None`: the root) whose own metadata has the keys `own`. A name Godot takes as a key
    /// stays the key; any other gets the nearest one Godot takes (`état` → `etat`, `max hp` →
    /// `max_hp`), and a name the export's own metadata has gets `_` after it (`id` → `id_`), so
    /// that no key is there twice and the export's keep their meaning. Both `id` and `layer_id`
    /// are a layer's and an object's own: which one a node has tells readers (`renamed`,
    /// inheritors.rs) which it is.
    pub fn metadata_keys(
        &mut self,
        of: Option<Element>,
        own: &[String],
        properties: &Properties,
    ) -> Vec<String> {
        let mut own: HashSet<&str> = own.iter().map(String::as_str).collect();
        if of.is_some() {
            own.extend(["id", "layer_id"]);
        }
        let as_named = |name: &str| !own.contains(name) && text::identifier(name) == name;
        let mut taken: HashSet<String> = own
            .iter()
            .map(|k| k.to_string())
            .chain(properties.keys().filter(|n| as_named(n)).cloned())
            .collect();
        let mut keys = Vec::with_capacity(properties.len());
        for name in properties.keys() {
            if as_named(name) {
                keys.push(name.clone());
                continue;
            }
            let base = text::identifier(name);
            let (mut key, mut n) = (base.clone(), 1);
            while taken.contains(&key) {
                key = if n == 1 {
                    format!("{base}_")
                } else {
                    format!("{base}_{n}")
                };
                n += 1;
            }
            taken.insert(key.clone());
            let noted = if own.contains(base.as_str()) {
                &mut self.reserved
            } else {
                &mut self.keys
            };
            noted
                .entry((name.clone(), key.clone()))
                .or_default()
                .push(of);
            keys.push(key);
        }
        keys
    }

    /// The report's notes on properties whose metadata key isn't their name: a warning for
    /// those named like the export's own metadata, and a note for the others.
    pub fn key_notes(&mut self) -> Vec<Note> {
        let mut notes = Vec::new();
        let reserved = std::mem::take(&mut self.reserved);
        if !reserved.is_empty() {
            let message = format!(
                "metadata/id, layer_id, class and bt_source are the export's own (a Tiled id, the class, the map the scene is made from), so properties of those names go under another key: {}",
                rekeyed(&reserved)
            );
            notes.push(Note::warning(
                crate::Code::ReservedName,
                message,
                key_elements(&reserved),
            ));
        }
        let keys = std::mem::take(&mut self.keys);
        if !keys.is_empty() {
            let message = format!(
                "Godot's metadata keys are ASCII letters, digits and _, not starting with a digit, so these properties go under another key: {}",
                rekeyed(&keys)
            );
            notes.push(Note {
                code: crate::Code::MetadataKey,
                severity: Severity::Info,
                message,
                elements: key_elements(&keys),
            });
        }
        notes
    }

    /// The layers and objects whose node path differs from the one in `old`, a scene the export
    /// wrote before, found by the `metadata/layer_id` and `metadata/id` it gave them
    /// ([`identity`]). Text that isn't a scene, or has neither, gives nothing; so does a node
    /// gone from either side.
    pub fn renamed(&self, old: &str) -> Vec<Renamed> {
        let Ok(old) = read::parse(old) else {
            return Vec::new();
        };
        // An id found on two nodes says nothing; on the root it is the map's property.
        let (mut layers, mut objects) = (HashMap::new(), HashMap::new());
        for (node, section) in old.nodes().filter(|(node, _)| node.parent.is_some()) {
            let (id, paths) = match identity(section) {
                Some(Element::Layer(id)) => (id, &mut layers),
                Some(Element::Object(id)) => (id, &mut objects),
                _ => continue,
            };
            paths
                .entry(id)
                .and_modify(|path| *path = None)
                .or_insert(Some(node.path()));
        }
        let changed = |new: &BTreeMap<u32, String>,
                       old: &HashMap<u32, Option<String>>,
                       element: fn(u32) -> Element| {
            new.iter()
                .filter_map(|(&id, to)| {
                    let from = old.get(&id)?.as_ref()?;
                    (from != to).then(|| Renamed {
                        element: element(id),
                        from: from.clone(),
                        to: to.clone(),
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut renamed = changed(&self.layers, &layers, Element::Layer);
        renamed.extend(changed(&self.objects, &objects, Element::Object));
        renamed
    }
}

impl Build<'_> {
    /// What the report says about node identity: a warning when the map file at `map_path`
    /// leaves its layers' ids to be made up as it loads, and the nodes whose path changed since
    /// the scene at `target`.
    pub(super) fn identity(&mut self, map_path: &Path, target: &Path) -> Vec<Renamed> {
        if std::fs::read_to_string(map_path).is_ok_and(|text| layer_ids_unsaved(&text)) {
            let message = format!(
                "{}: the map file gives its layers no ids, so they are numbered as it loads and can change; save the map once so its layer ids are kept, and scenes that inherit this one keep their overrides",
                map_path.display()
            );
            self.warn_once(crate::Code::LayerIdsUnsaved, message, Vec::new());
        }
        std::fs::read_to_string(target)
            .map(|old| self.names.renamed(&old))
            .unwrap_or_default()
    }
}

/// The layer or object a node of a scene the export wrote stands for: its `metadata/layer_id`
/// or `metadata/id`, whichever comes first. The export writes its own before the node's
/// properties, which in scenes exported before they got keys of their own could repeat it or
/// have the other name.
pub(super) fn identity(section: &read::Section) -> Option<Element> {
    let id = |raw: &str| read::int(raw).and_then(|id| u32::try_from(id).ok());
    section
        .props
        .iter()
        .find_map(|(key, raw)| match key.as_ref() {
            "metadata/layer_id" => Some(id(raw).map(Element::Layer)),
            "metadata/id" => Some(id(raw).map(Element::Object)),
            _ => None,
        })
        .flatten()
}

/// The names of sibling layers' nodes, in their order.
pub(super) fn layer_names<'l>(layers: impl IntoIterator<Item = &'l Layer>) -> Vec<String> {
    let wanted: Vec<(u32, String)> = layers
        .into_iter()
        .map(|layer| {
            let name = text::node_name(&layer.name);
            (
                layer.id.0,
                if name.is_empty() {
                    format!("layer_{}", layer.id.0)
                } else {
                    name
                },
            )
        })
        .collect();
    siblings(&wanted)
}

/// The names of sibling objects' nodes (an object layer's), in their order: an object's name,
/// else its class or kind and its id (`Chest_195`, `rectangle_197`).
pub(super) fn object_names(objects: &[&MapObject]) -> Vec<String> {
    let wanted: Vec<(u32, String)> = objects
        .iter()
        .map(|o| {
            let name = text::node_name(&o.name);
            if !name.is_empty() {
                return (o.id.0, name);
            }
            let class = text::node_name(&o.class);
            (
                o.id.0,
                format!(
                    "{}_{}",
                    if class.is_empty() { kind(o) } else { &class },
                    o.id.0
                ),
            )
        })
        .collect();
    siblings(&wanted)
}

/// Names for siblings, each wanting one (`(id, name)`): in id order, each takes its name if no
/// lower id has, else adds `_<id>` until it is free. So the lowest id keeps a shared name, a new
/// sibling (the highest id) never takes another's, and a deletion can only shorten a name.
pub(super) fn siblings(wanted: &[(u32, String)]) -> Vec<String> {
    let mut order: Vec<_> = wanted.iter().enumerate().collect();
    order.sort_by_key(|(_, item)| item.0);
    let mut taken = HashSet::new();
    let mut names = vec![String::new(); wanted.len()];
    for (i, (id, wanted)) in order {
        let mut name = wanted.clone();
        while taken.contains(&name) {
            name = format!("{name}_{id}");
        }
        taken.insert(name.clone());
        if let Some(target) = names.get_mut(i) {
            *target = name;
        }
    }
    names
}

/// Properties under another key, as a list of at most a few: "id is metadata/id_ on objects 1
/// and 2; …".
fn rekeyed(keys: &BTreeMap<(String, String), Vec<Option<Element>>>) -> String {
    const SHOWN: usize = 8;
    let mut items: Vec<String> = keys
        .iter()
        .take(SHOWN)
        .map(|((name, key), of)| format!("{name} is metadata/{key} on {}", having(of)))
        .collect();
    if keys.len() > SHOWN {
        items.push(format!("and {} more", keys.len() - SHOWN));
    }
    items.join("; ")
}

/// Where properties are: "the map, layer 2 and objects 1 and 3".
fn having(of: &[Option<Element>]) -> String {
    const SHOWN: usize = 8;
    let mut parts = Vec::new();
    if of.contains(&None) {
        parts.push("the map".to_owned());
    }
    for (layers, one, many) in [(true, "layer", "layers"), (false, "object", "objects")] {
        let ids: Vec<String> = of
            .iter()
            .filter_map(|e| match e {
                Some(Element::Layer(id)) if layers => Some(id.to_string()),
                Some(Element::Object(id)) if !layers => Some(id.to_string()),
                _ => None,
            })
            .collect();
        match ids.len() {
            0 => {}
            1 => parts.push(format!(
                "{one} {}",
                ids.first().map(String::as_str).unwrap_or_default()
            )),
            n if n <= SHOWN => parts.push(format!(
                "{many} {} and {}",
                ids.iter()
                    .take(n - 1)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                ids.last().map(String::as_str).unwrap_or_default()
            )),
            n => parts.push(format!(
                "{many} {} and {} more",
                ids.iter()
                    .take(SHOWN)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                n - SHOWN
            )),
        }
    }
    match parts.as_slice() {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => parts.concat(),
    }
}

/// The layers and objects having properties under another key.
fn key_elements(keys: &BTreeMap<(String, String), Vec<Option<Element>>>) -> Vec<Element> {
    let mut elements: Vec<Element> = Vec::new();
    for e in keys.values().flatten().flatten() {
        if !elements.contains(e) {
            elements.push(*e);
        }
    }
    elements
}

/// What an unnamed object without a class is called after.
fn kind(o: &MapObject) -> &'static str {
    if o.tile.is_some() {
        return "sprite";
    }
    match &o.shape {
        Shape::Rectangle => "rectangle",
        Shape::Ellipse => "ellipse",
        Shape::Point => "point",
        Shape::Capsule => "capsule",
        Shape::Polygon(_) => "polygon",
        Shape::Polyline(_) => "polyline",
        Shape::Text(_) => "text",
    }
}

/// Whether a map file (TMX or JSON) has layers without ids. bt_tmx numbers such layers as the
/// file loads, so their ids, and with them names and `unique_id`s, can change between exports.
pub(super) fn layer_ids_unsaved(map_file: &str) -> bool {
    if map_file.trim_start().starts_with('{') {
        fn missing(layers: &serde_json::Value) -> bool {
            let unsaved = |layer: &serde_json::Value| {
                layer
                    .get("id")
                    .and_then(serde_json::Value::as_u64)
                    .is_none_or(|id| id == 0)
            };
            layers.as_array().is_some_and(|layers| {
                layers
                    .iter()
                    .any(|layer| unsaved(layer) || missing(&layer["layers"]))
            })
        }
        return serde_json::from_str::<serde_json::Value>(map_file)
            .is_ok_and(|map| map.get("layers").is_some_and(missing));
    }
    // XML can't have `<` inside a value, so every `<` opens a tag. Tilesets are skipped: their
    // tiles' collision shapes are object groups too, without ids.
    let mut in_tileset = false;
    for tag in map_file.split('<').skip(1) {
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        let (closing, tag) = match tag.strip_prefix('/') {
            Some(tag) => (true, tag),
            None => (false, tag),
        };
        let end = tag
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let (name, attrs) = tag.split_at(end);
        match name {
            "tileset" => in_tileset = !closing && !attrs.trim_end().ends_with('/'),
            "layer" | "objectgroup" | "imagelayer" | "group" if !closing && !in_tileset => {
                let id = xml_attrs(attrs)
                    .find(|(key, _)| *key == "id")
                    .and_then(|(_, id)| id.parse::<u32>().ok());
                if id.is_none_or(|id| id == 0) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// The `key="value"` pairs of an XML tag, values as written.
fn xml_attrs(mut attrs: &str) -> impl Iterator<Item = (&str, &str)> {
    std::iter::from_fn(move || {
        let (key, rest) = attrs.split_once('=')?;
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let (value, rest) = rest[1..].split_once(quote)?;
        attrs = rest;
        Some((key.trim(), value))
    })
}
