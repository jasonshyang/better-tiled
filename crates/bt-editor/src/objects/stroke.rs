//! Dragging on the map: a move, a turn or a resize of the selected objects (Select Objects'
//! body and handles), a move of the selected nodes (Edit Polygons), or the Offset Layers tool's
//! move of the selected layers' offsets (Tiled's `LayerOffsetTool`,
//! `reference/tiled/src/tiled/layeroffsettool.cpp`), applied live from where they were when the
//! drag started (so snapping and the pointer's path never pile up) and recorded as one step.
//! Objects on locked layers, and locked layers, stay where they are and are named when the drag
//! ends.

use std::sync::Arc;

use bt_core::{LayerId, Map, ObjectId};
use bt_edit::{
    Node, Pivot, Resize, ResizeHandle, SnapMode, drag_angle, move_anchor, move_anchor_in,
    move_nodes, move_nodes_in, move_objects, move_objects_in, nodes_anchor, nodes_anchor_in,
    offset_layers, resize_objects, resize_objects_in, rotate_objects, rotate_objects_in, snap,
    snap_delta,
};
use glam::DVec2;

use super::handles::SelectionSpace;
use super::host::{ObjectHost, TileShapes, change_tile_shapes};
use super::snapping::SnapToggle;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::layer_commands::locked_layers_notice;
use crate::store::TilesetHome;

/// A step of a drag of objects.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectStroke {
    /// A drag of the selected objects starts.
    Begin {
        /// The document.
        doc: DocumentId,
        /// What it does.
        kind: ObjectStrokeKind,
    },
    /// The objects, or the layers' offsets, are `delta` (the map's screen space) from where they
    /// started, snapped as the preference, changed by `snap`, says. A layer's offset snaps as a
    /// position does, as in Tiled.
    Move {
        /// How far.
        delta: DVec2,
        /// How the modifiers held change snapping.
        snap: SnapToggle,
    },
    /// A rotate handle dragged from `from` to `to` (the map's screen space) turns the objects
    /// around the middle of their handles by the angle between the two, seen from there, in
    /// steps of 15° with `snap` (Tiled's `updateRotatingItems`).
    Rotate {
        /// Where the drag started.
        from: DVec2,
        /// Where it is now.
        to: DVec2,
        /// Whether the angle snaps.
        snap: bool,
    },
    /// A resize handle dragged to `to` (the map's screen space), snapped as the preference says:
    /// the side or corner follows it while the opposite one stays, or with `from_centre` the
    /// middle stays; `keep_proportions` keeps the selection's shape.
    Resize {
        /// The handle.
        handle: ResizeHandle,
        /// Where it is now.
        to: DVec2,
        /// The middle stays rather than the opposite side.
        from_centre: bool,
        /// The proportions stay.
        keep_proportions: bool,
    },
    /// The selected nodes are `delta` (the map's screen space) from where they started, the
    /// node nearest the top-left snapped as the preference, changed by `snap`, says.
    MoveNodes {
        /// How far.
        delta: DVec2,
        /// How the modifiers held change snapping.
        snap: SnapToggle,
    },
    /// The drag is done: one step of the history.
    End,
    /// The drag is dropped: the objects go back.
    Cancel,
}

/// What a drag of objects does, which names its step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ObjectStrokeKind {
    /// Moves them.
    Move,
    /// Turns them.
    Rotate,
    /// Resizes them.
    Resize,
    /// Moves the selected nodes of polygons and polylines.
    MoveNodes,
    /// Moves the selected layers' offsets (the Offset Layers tool).
    OffsetLayers,
}

/// A drag of objects under way.
#[derive(Clone, Debug)]
pub(crate) struct ObjectStrokeState {
    doc: DocumentId,
    kind: ObjectStrokeKind,
    /// The layers whose offsets move: the selected ones (only when layers move).
    layers: Vec<LayerId>,
    /// The objects as the drag found them.
    start: StrokeStart,
    /// The objects that move: the selected ones on unlocked layers.
    ids: Vec<ObjectId>,
    /// The locked layers holding selected objects that stay.
    locked: Vec<LayerId>,
    /// The space the drag is measured in.
    space: SelectionSpace,
    /// The middle of the handles when the drag began, in the objects' pixels.
    centre: DVec2,
    /// The nodes that move: the selected ones of objects on unlocked layers.
    nodes: Vec<Node>,
}

/// What a drag of objects starts from.
#[derive(Clone, Debug)]
enum StrokeStart {
    /// The map, for objects of its layers.
    Map(Arc<Map>),
    /// A tile's collision shapes.
    Tile {
        /// Where the tileset lives.
        home: TilesetHome,
        /// The tile's shapes.
        shapes: TileShapes,
    },
}

/// How a drag changes the moving objects from where they started.
#[derive(Copy, Clone, Debug)]
enum StrokeChange {
    /// Moved by a screen delta, snapped as `snapping` says.
    Move { delta: DVec2, snapping: SnapMode },
    /// Turned around a pixel point.
    Rotate { degrees: f64, center: DVec2 },
    /// Resized by a handle.
    Resize(Resize),
    /// The nodes moved by a pixel delta, snapped as `snapping` says.
    MoveNodes { delta: DVec2, snapping: SnapMode },
    /// The layers' offsets moved by a screen delta, each snapped as a position.
    OffsetLayers { delta: DVec2, snapping: SnapMode },
}

impl Editor {
    /// Takes a step of a drag of objects.
    pub(crate) fn object_stroke(&mut self, step: ObjectStroke) -> Vec<Effect> {
        match step {
            ObjectStroke::Begin { doc, kind } => {
                let selected = self
                    .documents
                    .get(doc)
                    .map(|d| d.selection().objects.clone())
                    .unwrap_or_default();
                let (start, split) = match self.object_host(doc) {
                    Some(ObjectHost::Map(_)) => {
                        let Some(map) = self.map_of(doc).cloned() else {
                            return Vec::new();
                        };
                        let split = unlocked_objects(&map, &selected);
                        (StrokeStart::Map(map), split)
                    }
                    Some(ObjectHost::Tile { home, tile }) => {
                        let Some(shapes) = self.tile_shapes(home, tile) else {
                            return Vec::new();
                        };
                        let split = SplitByLock {
                            unlocked: selected,
                            locked_layers: Vec::new(),
                        };
                        (StrokeStart::Tile { home, shapes }, split)
                    }
                    None => return Vec::new(),
                };
                let label = match kind {
                    ObjectStrokeKind::Move => "Move Objects",
                    ObjectStrokeKind::Rotate => "Rotate Objects",
                    ObjectStrokeKind::Resize => "Resize Objects",
                    ObjectStrokeKind::MoveNodes => "Move Nodes",
                    ObjectStrokeKind::OffsetLayers => "Offset Layers",
                };
                let Some(space) = self.selection_space(doc) else {
                    return Vec::new();
                };
                let centre = self
                    .selection_handles(doc)
                    .map_or(DVec2::ZERO, |frame| space.to_pixel(frame.centre()));
                if let Err(error) = self.begin_gesture(doc, label) {
                    return vec![warn(error)];
                }
                let nodes = self
                    .documents
                    .get(doc)
                    .map(|d| d.selection().nodes.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|node| split.unlocked.contains(&node.object))
                    .collect();
                let (ids, layers, locked) = if kind == ObjectStrokeKind::OffsetLayers {
                    let StrokeStart::Map(map) = &start else {
                        return Vec::new();
                    };
                    let layers = self
                        .documents
                        .get(doc)
                        .map(|d| d.selection().layers.clone())
                        .unwrap_or_default();
                    let locked = layers
                        .iter()
                        .copied()
                        .filter(|id| map.effective_layer(*id).is_some_and(|l| l.locked))
                        .collect();
                    (Vec::new(), layers, locked)
                } else {
                    (split.unlocked, Vec::new(), split.locked_layers)
                };
                self.object_stroke = Some(ObjectStrokeState {
                    doc,
                    kind,
                    layers,
                    start,
                    ids,
                    locked,
                    space,
                    centre,
                    nodes,
                });
                Vec::new()
            }
            ObjectStroke::Move { delta, snap } => {
                let snapping = self.snap_mode(snap);
                let layers = self
                    .object_stroke
                    .as_ref()
                    .is_some_and(|stroke| stroke.kind == ObjectStrokeKind::OffsetLayers);
                if layers {
                    self.reapply(StrokeChange::OffsetLayers { delta, snapping })
                } else {
                    self.reapply(StrokeChange::Move { delta, snapping })
                }
            }
            ObjectStroke::Rotate { from, to, snap } => {
                let Some(stroke) = &self.object_stroke else {
                    return Vec::new();
                };
                let space = stroke.space;
                let center = stroke.centre;
                let degrees = drag_angle(
                    &space.geometry,
                    center,
                    space.to_pixel(from),
                    space.to_pixel(to),
                    snap,
                );
                self.reapply(StrokeChange::Rotate { degrees, center })
            }
            ObjectStroke::Resize {
                handle,
                to,
                from_centre,
                keep_proportions,
            } => {
                let Some(stroke) = &self.object_stroke else {
                    return Vec::new();
                };
                let space = stroke.space;
                let at = snap(
                    &space.geometry,
                    space.to_pixel(to),
                    self.snap_mode(SnapToggle::Keep),
                );
                let mut resize = Resize::handle_to(handle, at);
                if from_centre {
                    resize = resize.from_centre();
                }
                if keep_proportions {
                    resize = resize.keeping_proportions();
                }
                self.reapply(StrokeChange::Resize(resize))
            }
            ObjectStroke::MoveNodes { delta, snap } => {
                let snapping = self.snap_mode(snap);
                self.reapply(StrokeChange::MoveNodes { delta, snapping })
            }
            ObjectStroke::End => {
                let Some(stroke) = self.object_stroke.take() else {
                    return Vec::new();
                };
                if let Err(error) = self.end_gesture() {
                    return vec![warn(error)];
                }
                if stroke.kind == ObjectStrokeKind::OffsetLayers {
                    let StrokeStart::Map(map) = &stroke.start else {
                        return Vec::new();
                    };
                    return locked_layers_notice(
                        map,
                        &stroke.locked,
                        "it stayed where it was",
                        "they stayed where they were",
                    )
                    .into_iter()
                    .collect();
                }
                self.locked_objects_notice(stroke.doc, &stroke.locked)
                    .into_iter()
                    .collect()
            }
            ObjectStroke::Cancel => {
                self.object_stroke = None;
                match self.cancel_gesture() {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![warn(error)],
                }
            }
        }
    }

    /// Puts the objects back as the drag found them and applies `change` to the moving ones.
    fn reapply(&mut self, change: StrokeChange) -> Vec<Effect> {
        let Some(stroke) = self.object_stroke.clone() else {
            return Vec::new();
        };
        let Some(asset) = self.documents.get(stroke.doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let ids = &stroke.ids;
        let nodes = &stroke.nodes;
        let layers = &stroke.layers;
        let result = self.gesture_apply(|tx| match &stroke.start {
            StrokeStart::Map(start) => {
                let map = tx.map_mut(asset)?;
                *map = (**start).clone();
                let geometry = map.geometry();
                match change {
                    StrokeChange::Move { delta, snapping } => {
                        let pixel_delta =
                            geometry.screen_to_pixel(delta) - geometry.screen_to_pixel(DVec2::ZERO);
                        let anchor = move_anchor(map, ids)?.unwrap_or(DVec2::ZERO);
                        let snapped = snap_delta(&geometry, anchor, pixel_delta, snapping);
                        move_objects(map, ids, snapped)?;
                    }
                    StrokeChange::Rotate { degrees, center } => {
                        rotate_objects(map, ids, degrees, Pivot::Point(center))?;
                    }
                    StrokeChange::Resize(resize) => {
                        resize_objects(map, ids, resize)?;
                    }
                    StrokeChange::MoveNodes { delta, snapping } => {
                        let pixel_delta =
                            geometry.screen_to_pixel(delta) - geometry.screen_to_pixel(DVec2::ZERO);
                        let anchor = nodes_anchor(map, nodes)?.unwrap_or(DVec2::ZERO);
                        let snapped = snap_delta(&geometry, anchor, pixel_delta, snapping);
                        move_nodes(map, nodes, snapped)?;
                    }
                    StrokeChange::OffsetLayers { delta, snapping } => {
                        offset_layers(map, layers, delta, |offset| {
                            snap(&geometry, offset, snapping)
                        })?;
                    }
                }
                Ok::<_, EditError>(())
            }
            StrokeStart::Tile { home, shapes } => {
                change_tile_shapes(tx, *home, shapes.tile, |objects, space| {
                    *objects = shapes.shapes.clone();
                    let geometry = space.geometry();
                    match change {
                        StrokeChange::Move { delta, snapping } => {
                            let pixel_delta = geometry.screen_to_pixel(delta)
                                - geometry.screen_to_pixel(DVec2::ZERO);
                            let anchor = move_anchor_in(objects, ids)?.unwrap_or(DVec2::ZERO);
                            let snapped = snap_delta(geometry, anchor, pixel_delta, snapping);
                            move_objects_in(objects, ids, snapped)?;
                        }
                        StrokeChange::Rotate { degrees, center } => {
                            rotate_objects_in(objects, space, ids, degrees, Pivot::Point(center))?;
                        }
                        StrokeChange::Resize(resize) => {
                            resize_objects_in(objects, space, ids, resize)?;
                        }
                        StrokeChange::MoveNodes { delta, snapping } => {
                            let pixel_delta = geometry.screen_to_pixel(delta)
                                - geometry.screen_to_pixel(DVec2::ZERO);
                            let anchor =
                                nodes_anchor_in(objects, space, nodes)?.unwrap_or(DVec2::ZERO);
                            let snapped = snap_delta(geometry, anchor, pixel_delta, snapping);
                            move_nodes_in(objects, space, nodes, snapped)?;
                        }
                        // A tile's shapes have no layers to offset.
                        StrokeChange::OffsetLayers { .. } => {}
                    }
                    Ok(())
                })
            }
        });
        result.err().map(warn).into_iter().collect()
    }
}

/// Objects split by whether their layer is locked.
pub(crate) struct SplitByLock {
    /// The objects on unlocked layers.
    pub(crate) unlocked: Vec<ObjectId>,
    /// The locked layers holding the others, each once.
    pub(crate) locked_layers: Vec<LayerId>,
}

/// The objects of `ids` on unlocked layers, and the locked layers holding the rest.
pub(crate) fn unlocked_objects(map: &Map, ids: &[ObjectId]) -> SplitByLock {
    let mut split = SplitByLock {
        unlocked: Vec::new(),
        locked_layers: Vec::new(),
    };
    for &id in ids {
        let Some(layer) = map.object_layer_of(id) else {
            continue;
        };
        if map.effective_layer(layer).is_some_and(|l| l.locked) {
            if !split.locked_layers.contains(&layer) {
                split.locked_layers.push(layer);
            }
        } else {
            split.unlocked.push(id);
        }
    }
    split
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;
    use crate::actions::ActionId;
    use crate::intent::Intent;

    /// A 10×10 map of 16-pixel cells with the object layers "a" (1), "b" (2, locked) and "c"
    /// (3); "a" and "b" are selected.
    fn editor_with_layers() -> (Editor, DocumentId) {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(10))
            .object_layer("a", Vec::new())
            .object_layer("b", Vec::new())
            .object_layer("c", Vec::new())
            .build()
            .unwrap();
        map.layers[1].locked = true;
        let mut editor = Editor::new();
        let doc = editor.open_new_map(map);
        if let Some(document) = editor.documents.get_mut(doc) {
            document.selection.current_layer = Some(LayerId(1));
            document.selection.layers = vec![LayerId(1), LayerId(2)];
        }
        (editor, doc)
    }

    fn offset(editor: &Editor, doc: DocumentId, layer: u32) -> DVec2 {
        editor
            .map_of(doc)
            .unwrap()
            .layer(LayerId(layer))
            .unwrap()
            .offset
    }

    fn drag(editor: &mut Editor, step: ObjectStroke) -> Vec<Effect> {
        editor.apply(Intent::ObjectStroke(step))
    }

    #[test]
    fn offsetting_layers_moves_the_unlocked_selected_ones_live_in_one_step() {
        let (mut editor, doc) = editor_with_layers();
        drag(
            &mut editor,
            ObjectStroke::Begin {
                doc,
                kind: ObjectStrokeKind::OffsetLayers,
            },
        );
        for x in [5.0, 21.0] {
            drag(
                &mut editor,
                ObjectStroke::Move {
                    delta: DVec2::new(x, 5.0),
                    snap: SnapToggle::Keep,
                },
            );
            assert_eq!(offset(&editor, doc, 1), DVec2::new(x, 5.0), "live");
        }
        // Command snaps the offset to the grid, as a position.
        drag(
            &mut editor,
            ObjectStroke::Move {
                delta: DVec2::new(21.0, 5.0),
                snap: SnapToggle::Grid,
            },
        );
        assert_eq!(offset(&editor, doc, 1), DVec2::new(16.0, 0.0));
        let effects = drag(&mut editor, ObjectStroke::End);
        assert!(
            matches!(&effects[..], [Effect::Notify(notice)]
                if notice.text == "“b” is locked: it stayed where it was"),
            "{effects:?}"
        );
        assert_eq!(offset(&editor, doc, 2), DVec2::ZERO);
        assert_eq!(offset(&editor, doc, 3), DVec2::ZERO, "not selected");
        let history = editor.document(doc).unwrap().history();
        assert_eq!(history.undoable().unwrap().label, "Offset Layers");
        editor.apply(Intent::Run(ActionId::Undo));
        assert_eq!(offset(&editor, doc, 1), DVec2::ZERO, "one step");
    }

    #[test]
    fn a_cancelled_offset_puts_the_layers_back_and_leaves_no_step() {
        let (mut editor, doc) = editor_with_layers();
        drag(
            &mut editor,
            ObjectStroke::Begin {
                doc,
                kind: ObjectStrokeKind::OffsetLayers,
            },
        );
        drag(
            &mut editor,
            ObjectStroke::Move {
                delta: DVec2::new(30.0, -7.0),
                snap: SnapToggle::Keep,
            },
        );
        drag(&mut editor, ObjectStroke::Cancel);
        assert_eq!(offset(&editor, doc, 1), DVec2::ZERO);
        assert!(editor.document(doc).unwrap().history().undoable().is_none());
    }
}
