//! Dragging objects: a move or a turn of the selected objects, applied live from where they
//! were when the drag started (so snapping and the pointer's path never pile up) and recorded
//! as one step. Objects on locked layers stay, and are named when the drag ends.

use std::sync::Arc;

use bt_core::{LayerId, Map, ObjectId};
use bt_edit::{Pivot, move_anchor, move_objects, rotate_objects, snap_delta};
use glam::DVec2;

use super::snapping::SnapToggle;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;

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
    /// The map as the drag found it.
    start: Arc<Map>,
    /// The objects that move: the selected ones on unlocked layers.
    ids: Vec<ObjectId>,
    /// The locked layers holding selected objects that stay.
    locked: Vec<LayerId>,
}

impl Editor {
    /// Takes a step of a drag of objects.
    pub(crate) fn object_stroke(&mut self, step: ObjectStroke) -> Vec<Effect> {
        match step {
            ObjectStroke::Begin { doc, kind } => {
                let Some(start) = self.map_of(doc).cloned() else {
                    return Vec::new();
                };
                let selected = self
                    .documents
                    .get(doc)
                    .map(|d| d.selection().objects.clone())
                    .unwrap_or_default();
                let split = unlocked_objects(&start, &selected);
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
                self.reapply(|map, ids| {
                    let geometry = map.geometry();
                    let pixel_delta =
                        geometry.screen_to_pixel(delta) - geometry.screen_to_pixel(DVec2::ZERO);
                    let anchor = move_anchor(map, ids)?.unwrap_or(DVec2::ZERO);
                    let snapped = snap_delta(&geometry, anchor, pixel_delta, snapping);
                    move_objects(map, ids, snapped)?;
                    Ok(())
                })
            }
            ObjectStroke::Rotate { degrees, center } => self.reapply(|map, ids| {
                rotate_objects(map, ids, degrees, Pivot::Point(center))?;
                Ok(())
            }),
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

    /// Puts the map back as the drag found it and applies `change` to the moving objects.
    fn reapply(
        &mut self,
        change: impl FnOnce(&mut Map, &[ObjectId]) -> Result<(), bt_edit::EditError>,
    ) -> Vec<Effect> {
        let Some(stroke) = self.object_stroke.clone() else {
            return Vec::new();
        };
        let Some(asset) = self.documents.get(stroke.doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let result = self.gesture_apply(|tx| {
            let map = tx.map_mut(asset)?;
            *map = (*stroke.start).clone();
            change(map, &stroke.ids).map_err(EditError::from)
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
