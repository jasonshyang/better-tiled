//! What the Objects panel lists (Tiled's `MapObjectModel`, `reference/tiled/src/tiled/
//! mapobjectmodel.cpp`, upside down as its `ObjectsView` shows it): the object layers inside their
//! groups, topmost first, each followed by its objects topmost first; and the filter that keeps
//! the objects whose name, class, kind or id holds a text, with the layers holding them.

use std::collections::HashMap;

use bt_core::{Layer, LayerKind, Map, MapObject, ObjectId};
use bt_edit::render_order;

use crate::document::DocumentId;
use crate::editor::Editor;

/// A row of the Objects panel.
#[derive(Copy, Clone, Debug)]
pub enum ObjectRow<'m> {
    /// An object layer, or a group holding one.
    Layer {
        /// The layer.
        layer: &'m Layer,
        /// How many groups hold it.
        depth: usize,
    },
    /// An object.
    Object {
        /// The object.
        object: &'m MapObject,
        /// Its object layer.
        layer: &'m Layer,
        /// Its place in the layer's list, counted from the bottom.
        index: usize,
        /// One more than its layer's depth.
        depth: usize,
    },
}

impl ObjectRow<'_> {
    /// How deep in the tree the row is.
    pub fn depth(&self) -> usize {
        match self {
            ObjectRow::Layer { depth, .. } | ObjectRow::Object { depth, .. } => *depth,
        }
    }
}

impl Editor {
    /// The Objects panel's rows for `doc`: the object layers and the groups holding them,
    /// topmost first, each object layer followed by its objects in the order they are drawn,
    /// topmost first. With a `filter`, only the objects whose name, class, kind or id holds it
    /// (ignoring case), and the layers holding them.
    pub fn object_rows(&self, doc: DocumentId, filter: &str) -> Vec<ObjectRow<'_>> {
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let filter = filter.trim().to_lowercase();
        let mut rows = Vec::new();
        list_layers(map, &map.layers, 0, &filter, &mut rows);
        rows
    }
}

/// Adds the rows of `layers` (bottom first in the map) topmost first; whether any was added.
fn list_layers<'m>(
    map: &'m Map,
    layers: &'m [Layer],
    depth: usize,
    filter: &str,
    rows: &mut Vec<ObjectRow<'m>>,
) -> bool {
    let mut any = false;
    for layer in layers.iter().rev() {
        let start = rows.len();
        rows.push(ObjectRow::Layer { layer, depth });
        let listed = match &layer.kind {
            LayerKind::Group(children) => list_layers(map, children, depth + 1, filter, rows),
            LayerKind::Object(objects) => {
                let mut places: HashMap<ObjectId, usize> = HashMap::new();
                for (index, object) in objects.objects.iter().enumerate() {
                    places.entry(object.id).or_insert(index);
                }
                let mut matched = filter.is_empty();
                for object in render_order(objects, &map.geometry()).into_iter().rev() {
                    let Some(&index) = places.get(&object.id) else {
                        continue;
                    };
                    if filter.is_empty() || matches(object, filter) {
                        rows.push(ObjectRow::Object {
                            object,
                            layer,
                            index,
                            depth: depth + 1,
                        });
                        matched = true;
                    }
                }
                matched
            }
            LayerKind::Tile(_) | LayerKind::Image(_) => false,
        };
        if listed {
            any = true;
        } else {
            rows.truncate(start);
        }
    }
    any
}

/// Whether the object's name, class, kind or id holds `filter`, which is in lowercase.
fn matches(object: &MapObject, filter: &str) -> bool {
    [
        object.name.as_str(),
        object.class.as_str(),
        object.kind_name(),
    ]
    .iter()
    .any(|text| text.to_lowercase().contains(filter))
        || object.id.0.to_string().contains(filter)
}
