//! Dragging objects: a move or a turn of the selected objects, applied live from where they
//! were when the drag started (so snapping and the pointer's path never pile up) and recorded
//! as one step. Objects on locked layers stay, and are named when the drag ends.

use std::sync::Arc;

use bt_core::{LayerId, Map, ObjectId};
use bt_edit::{
    Pivot, SnapMode, move_anchor, move_anchor_in, move_objects, move_objects_in, rotate_objects,
    rotate_objects_in, snap_delta,
};
use glam::DVec2;

use super::host::{ObjectHost, TileShapes, change_tile_shapes};
use super::snapping::SnapToggle;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
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
    /// The objects are `delta` (the map's screen space) from where they started, snapped as the
    /// preference, changed by `snap`, says.
    Move {
        /// How far.
        delta: DVec2,
        /// How the modifiers held change snapping.
        snap: SnapToggle,
    },
    /// The objects are turned by `degrees` (clockwise) around `center` (pixel space) from how
    /// they started.
    Rotate {
        /// How far.
        degrees: f64,
        /// Around what.
        center: DVec2,
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
}

/// A drag of objects under way.
#[derive(Clone, Debug)]
pub(crate) struct ObjectStrokeState {
    doc: DocumentId,
    /// The objects as the drag found them.
    start: StrokeStart,
    /// The objects that move: the selected ones on unlocked layers.
    ids: Vec<ObjectId>,
    /// The locked layers holding selected objects that stay.
    locked: Vec<LayerId>,
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
                };
                if let Err(error) = self.begin_gesture(doc, label) {
                    return vec![warn(error)];
                }
                self.object_stroke = Some(ObjectStrokeState {
                    doc,
                    start,
                    ids: split.unlocked,
                    locked: split.locked_layers,
                });
                Vec::new()
            }
            ObjectStroke::Move { delta, snap } => {
                let snapping = self.snap_mode(snap);
                self.reapply(StrokeChange::Move { delta, snapping })
            }
            ObjectStroke::Rotate { degrees, center } => {
                self.reapply(StrokeChange::Rotate { degrees, center })
            }
            ObjectStroke::End => {
                let Some(stroke) = self.object_stroke.take() else {
                    return Vec::new();
                };
                if let Err(error) = self.end_gesture() {
                    return vec![warn(error)];
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
