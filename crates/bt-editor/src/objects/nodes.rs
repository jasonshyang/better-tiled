//! Edit Polygons (Tiled's `EditPolygonTool`, `reference/tiled/src/tiled/editpolygontool.cpp`):
//! the nodes of the selected polygons and polylines, where they are on screen and which one the
//! pointer picks, and the node commands: select, insert on a segment, delete, join and split,
//! each one step. A map's objects are measured on the layer holding them (its offset taken
//! off), a tile's collision shapes in the tile's own space. Objects on locked layers keep their
//! nodes, and the locks are named.

use bt_core::{LayerId, ObjectFrame as Placement, ObjectId, ObjectLayer, Shape};
use bt_edit::{
    Node, ObjectSpace, SelectionMode, insert_node, insert_node_in, join_nodes, join_nodes_in,
    remove_nodes, remove_nodes_in, segment_at, split_segments, split_segments_in,
};
use glam::DVec2;

use super::host::{ObjectHost, change_tile_shapes};
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;

/// A node of a selected polygon or polyline where it is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct NodeHandle {
    /// The node.
    pub node: Node,
    /// Where it is, in the map's screen space.
    pub at: DVec2,
}

/// The node commands that change nodes in place.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum NodeCommand {
    /// Delete the selected nodes.
    Delete,
    /// Join the selected nodes (Tiled's Join Nodes).
    Join,
    /// Split the segments between the selected nodes (Tiled's Split Segments).
    Split,
}

impl Editor {
    /// The nodes of `doc`'s selected polygons and polylines, where they are drawn.
    pub fn node_handles(&self, doc: DocumentId) -> Vec<NodeHandle> {
        let mut handles = Vec::new();
        self.visit_selected_lists(doc, |list| {
            for object in list.objects.objects.iter() {
                if !list.ids.contains(&object.id) || object.tile.is_some() {
                    continue;
                }
                let (Shape::Polygon(points) | Shape::Polyline(points)) = &object.shape else {
                    continue;
                };
                let placement = Placement::of(list.space.geometry(), object);
                handles.extend(points.iter().enumerate().map(|(index, point)| NodeHandle {
                    node: Node {
                        object: object.id,
                        index,
                    },
                    at: placement.to_screen(*point) + list.offset,
                }));
            }
        });
        handles
    }

    /// The node of the selected polygons and polylines drawn nearest `world`, within
    /// `tolerance` world pixels.
    pub fn node_at(&self, doc: DocumentId, world: DVec2, tolerance: f64) -> Option<Node> {
        self.node_handles(doc)
            .into_iter()
            .filter(|handle| handle.at.distance(world) <= tolerance)
            .min_by(|a, b| a.at.distance(world).total_cmp(&b.at.distance(world)))
            .map(|handle| handle.node)
    }

    /// The segment of the selected polygons and polylines drawn nearest `world`, within
    /// `tolerance` world pixels: the node it starts at.
    pub fn segment_at(&self, doc: DocumentId, world: DVec2, tolerance: f64) -> Option<Node> {
        let mut found = None;
        self.visit_selected_lists(doc, |list| {
            if found.is_some() {
                return;
            }
            let pixel = list.space.geometry().screen_to_pixel(world - list.offset);
            found = segment_at(list.objects, &list.space, &list.ids, pixel, tolerance)
                .ok()
                .flatten();
        });
        found
    }

    /// Changes the selected nodes: `nodes` combined with them as `mode` says.
    pub(crate) fn select_nodes(&mut self, doc: DocumentId, nodes: &[Node], mode: SelectionMode) {
        let Some(document) = self.documents.get_mut(doc) else {
            return;
        };
        let current = &mut document.selection.nodes;
        match mode {
            SelectionMode::Replace => *current = nodes.to_vec(),
            SelectionMode::Add => {
                for node in nodes {
                    if !current.contains(node) {
                        current.push(*node);
                    }
                }
            }
            SelectionMode::Subtract => current.retain(|node| !nodes.contains(node)),
            SelectionMode::Intersect => current.retain(|node| nodes.contains(node)),
        }
    }

    /// Inserts a node on `segment` at the point of it nearest `near` (the map's screen space),
    /// as one step; the new node is selected.
    pub(crate) fn insert_node_near(
        &mut self,
        doc: DocumentId,
        segment: Node,
        near: DVec2,
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let result = match self.object_host(doc) {
            Some(ObjectHost::Tile { home, tile }) => self.transact(doc, "Insert Node", |tx| {
                let node = change_tile_shapes(tx, home, tile, |objects, space| {
                    let pixel = space.geometry().screen_to_pixel(near);
                    insert_node_in(objects, space, segment, pixel)
                })?;
                tx.selection.nodes = vec![node];
                Ok::<_, EditError>(())
            }),
            Some(ObjectHost::Map(_)) => {
                let Some(map) = self.map_of(doc).cloned() else {
                    return Vec::new();
                };
                let Some(layer) = map.object_layer_of(segment.object) else {
                    return Vec::new();
                };
                let state = map.effective_layer(layer);
                if state.is_some_and(|s| s.locked) {
                    return self
                        .locked_objects_notice(doc, &[layer])
                        .into_iter()
                        .collect();
                }
                let offset = state.map_or(DVec2::ZERO, |s| s.offset);
                let pixel = map.geometry().screen_to_pixel(near - offset);
                self.transact(doc, "Insert Node", |tx| {
                    let node = insert_node(tx.map_mut(asset)?, segment, pixel)?;
                    tx.selection.nodes = vec![node];
                    Ok::<_, EditError>(())
                })
            }
            None => return Vec::new(),
        };
        result.err().map(warn).into_iter().collect()
    }

    /// Deletes, joins or splits the selected nodes, as one step; the node selection is dropped.
    pub(crate) fn change_selected_nodes(
        &mut self,
        doc: DocumentId,
        command: NodeCommand,
    ) -> Vec<Effect> {
        let label = match command {
            NodeCommand::Delete => "Delete Nodes",
            NodeCommand::Join => "Join Nodes",
            NodeCommand::Split => "Split Segments",
        };
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().nodes.clone())
            .unwrap_or_default();
        if selected.is_empty() {
            return Vec::new();
        }
        let result = match self.object_host(doc) {
            Some(ObjectHost::Tile { home, tile }) => self.transact(doc, label, |tx| {
                change_tile_shapes(tx, home, tile, |objects, _| match command {
                    NodeCommand::Delete => remove_nodes_in(objects, &selected),
                    NodeCommand::Join => join_nodes_in(objects, &selected),
                    NodeCommand::Split => split_segments_in(objects, &selected),
                })?;
                tx.selection.nodes.clear();
                Ok::<_, EditError>(())
            }),
            Some(ObjectHost::Map(_)) => {
                let Some(map) = self.map_of(doc).cloned() else {
                    return Vec::new();
                };
                let mut locked: Vec<LayerId> = Vec::new();
                let nodes: Vec<Node> = selected
                    .iter()
                    .copied()
                    .filter(|node| {
                        let Some(layer) = map.object_layer_of(node.object) else {
                            return false;
                        };
                        let free = map.effective_layer(layer).is_some_and(|s| !s.locked);
                        if !free && !locked.contains(&layer) {
                            locked.push(layer);
                        }
                        free
                    })
                    .collect();
                let mut effects: Vec<Effect> = self
                    .locked_objects_notice(doc, &locked)
                    .into_iter()
                    .collect();
                if nodes.is_empty() {
                    return effects;
                }
                let result = self.transact(doc, label, |tx| {
                    let map = tx.map_mut(asset)?;
                    match command {
                        NodeCommand::Delete => remove_nodes(map, &nodes)?,
                        NodeCommand::Join => join_nodes(map, &nodes)?,
                        NodeCommand::Split => split_segments(map, &nodes)?,
                    };
                    tx.selection.nodes.clear();
                    Ok::<_, EditError>(())
                });
                effects.extend(result.err().map(warn));
                return effects;
            }
            None => return Vec::new(),
        };
        result.err().map(warn).into_iter().collect()
    }

    /// Visits each list of objects holding selected objects of `doc`: a map's object layers,
    /// topmost first, or the current tile's shapes.
    fn visit_selected_lists(&self, doc: DocumentId, mut visit: impl FnMut(&SelectedList<'_>)) {
        let Some(selected) = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
        else {
            return;
        };
        if selected.is_empty() {
            return;
        }
        match self.object_host(doc) {
            Some(ObjectHost::Tile { home, tile }) => {
                let Some(shapes) = self.tile_shapes(home, tile) else {
                    return;
                };
                visit(&SelectedList {
                    objects: &shapes.shapes,
                    space: shapes.space(),
                    offset: DVec2::ZERO,
                    ids: selected,
                });
            }
            Some(ObjectHost::Map(_)) => {
                let Some(map) = self.map_of(doc) else {
                    return;
                };
                let space = ObjectSpace::of_map(map);
                for layer in bt_edit::layer_order(map).into_iter().rev() {
                    let (Some(objects), Some(state)) = (
                        map.layer(layer).and_then(|l| l.as_object_layer()),
                        map.effective_layer(layer),
                    ) else {
                        continue;
                    };
                    let ids: Vec<ObjectId> = objects
                        .objects
                        .iter()
                        .map(|o| o.id)
                        .filter(|id| selected.contains(id))
                        .collect();
                    if !ids.is_empty() {
                        visit(&SelectedList {
                            objects,
                            space,
                            offset: state.offset,
                            ids,
                        });
                    }
                }
            }
            None => {}
        }
    }
}

/// Selected objects of one list, with the space they live in and where it is drawn.
struct SelectedList<'a> {
    objects: &'a ObjectLayer,
    space: ObjectSpace<'a>,
    offset: DVec2,
    ids: Vec<ObjectId>,
}
