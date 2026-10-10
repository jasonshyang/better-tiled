//! The object commands: selecting objects, inserting one, and the Objects menu with Delete,
//! Cut, Copy and Paste on objects. Each change is one step; objects on locked layers are left
//! where they are and the locks are named.

use bt_core::{DrawOrder, LayerId, MapObject, ObjectId};
use bt_edit::{
    Axis, ObjectClip, Pivot, SelectionMode, add_object, copy_objects, cut_objects,
    duplicate_objects, flip_objects, flip_origin, lower_objects, lower_objects_to_bottom,
    move_objects_to_layer, paste_objects, paste_offset, raise_objects, raise_objects_to_top,
    remove_objects, rotate_objects,
};
use glam::DVec2;

use super::stroke::unlocked_objects;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// Where Raise and Lower move objects.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Restack {
    /// A step up.
    Raise,
    /// A step down.
    Lower,
    /// To the top.
    ToTop,
    /// To the bottom.
    ToBottom,
}

impl Editor {
    /// Changes the selected objects: `ids` combined with them as `mode` says.
    pub(crate) fn select_objects(
        &mut self,
        doc: DocumentId,
        ids: &[ObjectId],
        mode: SelectionMode,
    ) {
        let Some(document) = self.documents.get_mut(doc) else {
            return;
        };
        let current = &mut document.selection.objects;
        match mode {
            SelectionMode::Replace => *current = ids.to_vec(),
            SelectionMode::Add => {
                for id in ids {
                    if !current.contains(id) {
                        current.push(*id);
                    }
                }
            }
            SelectionMode::Subtract => current.retain(|id| !ids.contains(id)),
            SelectionMode::Intersect => current.retain(|id| ids.contains(id)),
        }
    }

    /// Inserts an object into the current object layer as one step; it becomes the selection.
    pub(crate) fn insert_object(&mut self, doc: DocumentId, object: MapObject) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let current = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().current_layer);
        let Some(layer) = current.filter(|id| {
            map.layer(*id)
                .is_some_and(|l| l.as_object_layer().is_some())
        }) else {
            return vec![Effect::Notify(Notice::info(
                "Objects go on object layers: make one current first",
            ))];
        };
        if map.effective_layer(layer).is_some_and(|l| l.locked) {
            return self
                .locked_objects_notice(doc, &[layer])
                .into_iter()
                .collect();
        }
        let result = self.transact(doc, "Insert Object", |tx| {
            let id =
                add_object(tx.map_mut(asset)?, layer, object, None).map_err(EditError::from)?;
            tx.selection.objects = vec![id];
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Runs `change` on the selected objects of unlocked layers as one step labelled `label`;
    /// the locked layers holding the others are named.
    fn on_selected_objects(
        &mut self,
        doc: DocumentId,
        label: &str,
        change: impl FnOnce(&mut bt_core::Map, &[ObjectId]) -> Result<(), bt_edit::EditError>,
    ) -> Vec<Effect> {
        let (Some(asset), Some(map)) = (
            self.documents.get(doc).map(|d| d.asset()),
            self.map_of(doc).cloned(),
        ) else {
            return Vec::new();
        };
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        let split = unlocked_objects(&map, &selected);
        let mut effects: Vec<Effect> = self
            .locked_objects_notice(doc, &split.locked_layers)
            .into_iter()
            .collect();
        if split.unlocked.is_empty() {
            return effects;
        }
        let ids = split.unlocked;
        let result = self.transact(doc, label, |tx| {
            change(tx.map_mut(asset)?, &ids).map_err(EditError::from)
        });
        effects.extend(result.err().map(warn));
        effects
    }

    /// Delete on objects.
    pub(crate) fn delete_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.on_selected_objects(doc, "Delete Objects", |map, ids| {
            remove_objects(map, ids).map(|_| ())
        })
    }

    /// Copy on objects.
    pub(crate) fn copy_selected_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(map) = self.map_of(doc).cloned() else {
            return Vec::new();
        };
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        match copy_objects(&map, &selected) {
            Ok(clip) => {
                self.clipboard.objects = Some(clip);
                Vec::new()
            }
            Err(error) => vec![warn(error.into())],
        }
    }

    /// Cut on objects: Copy, then the unlocked ones are removed.
    pub(crate) fn cut_selected_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        let mut copied: Option<ObjectClip> = None;
        let effects = self.on_selected_objects(doc, "Cut Objects", |map, ids| {
            copied = Some(cut_objects(map, ids)?);
            Ok(())
        });
        if let Some(clip) = copied {
            self.clipboard.objects = Some(clip);
        }
        effects
    }

    /// Paste on objects: into the current object layer, centred on `at` (pixel space) and
    /// snapped when given, else where they were copied from; they become the selection.
    pub(crate) fn paste_copied_objects(
        &mut self,
        doc: DocumentId,
        at: Option<DVec2>,
    ) -> Vec<Effect> {
        let (Some(clip), Some(asset), Some(map)) = (
            self.clipboard.objects.clone(),
            self.documents.get(doc).map(|d| d.asset()),
            self.map_of(doc).cloned(),
        ) else {
            return Vec::new();
        };
        let current = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().current_layer);
        let Some(layer) = current.filter(|id| {
            map.layer(*id)
                .is_some_and(|l| l.as_object_layer().is_some())
        }) else {
            return vec![Effect::Notify(Notice::info(
                "Objects are pasted into an object layer: make one current first",
            ))];
        };
        if map.effective_layer(layer).is_some_and(|l| l.locked) {
            return self
                .locked_objects_notice(doc, &[layer])
                .into_iter()
                .collect();
        }
        let offset = match at {
            Some(cursor) => paste_offset(&map.geometry(), &clip, cursor, self.snap_mode()),
            None => DVec2::ZERO,
        };
        let result = self.transact(doc, "Paste Objects", |tx| {
            let report =
                paste_objects(tx.map_mut(asset)?, layer, &clip, offset).map_err(EditError::from)?;
            tx.selection.objects = report.changed;
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Duplicate Objects: copies in place, with their names as they were (as Tiled's default);
    /// the copies become the selection.
    pub(crate) fn duplicate_selected_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        let (Some(asset), Some(map)) = (
            self.documents.get(doc).map(|d| d.asset()),
            self.map_of(doc).cloned(),
        ) else {
            return Vec::new();
        };
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        let split = unlocked_objects(&map, &selected);
        let mut effects: Vec<Effect> = self
            .locked_objects_notice(doc, &split.locked_layers)
            .into_iter()
            .collect();
        if split.unlocked.is_empty() {
            return effects;
        }
        let result = self.transact(doc, "Duplicate Objects", |tx| {
            let copies = duplicate_objects(tx.map_mut(asset)?, &split.unlocked, DVec2::ZERO, false)
                .map_err(EditError::from)?
                .changed;
            tx.selection.objects = copies;
            Ok::<_, EditError>(())
        });
        effects.extend(result.err().map(warn));
        effects
    }

    /// Raise, Lower, To Top, To Bottom.
    pub(crate) fn restack_objects(&mut self, doc: DocumentId, how: Restack) -> Vec<Effect> {
        let label = match how {
            Restack::Raise => "Raise Objects",
            Restack::Lower => "Lower Objects",
            Restack::ToTop => "Raise Objects to Top",
            Restack::ToBottom => "Lower Objects to Bottom",
        };
        self.on_selected_objects(doc, label, |map, ids| {
            match how {
                Restack::Raise => raise_objects(map, ids)?,
                Restack::Lower => lower_objects(map, ids)?,
                Restack::ToTop => raise_objects_to_top(map, ids)?,
                Restack::ToBottom => lower_objects_to_bottom(map, ids)?,
            };
            Ok(())
        })
    }

    /// Flips the selected objects around the middle of what they cover.
    pub(crate) fn flip_selected_objects(&mut self, doc: DocumentId, axis: Axis) -> Vec<Effect> {
        let label = match axis {
            Axis::Horizontal => "Flip Horizontally",
            Axis::Vertical => "Flip Vertically",
        };
        self.on_selected_objects(doc, label, |map, ids| {
            let Some(origin) = flip_origin(map, ids)? else {
                return Ok(());
            };
            flip_objects(map, ids, axis, origin)?;
            Ok(())
        })
    }

    /// Turns each selected object a quarter around its own position (Tiled's Rotate Left and
    /// Right).
    pub(crate) fn rotate_selected_objects(&mut self, doc: DocumentId, degrees: f64) -> Vec<Effect> {
        let label = if degrees > 0.0 {
            "Rotate Right"
        } else {
            "Rotate Left"
        };
        self.on_selected_objects(doc, label, |map, ids| {
            rotate_objects(map, ids, degrees, Pivot::OwnPosition).map(|_| ())
        })
    }

    /// Moves the selected objects to another object layer.
    pub(crate) fn move_selected_objects_to(
        &mut self,
        doc: DocumentId,
        layer: LayerId,
    ) -> Vec<Effect> {
        let locked = self
            .map_of(doc)
            .is_some_and(|map| map.effective_layer(layer).is_some_and(|l| l.locked));
        if locked {
            return self
                .locked_objects_notice(doc, &[layer])
                .into_iter()
                .collect();
        }
        self.on_selected_objects(doc, "Move Objects to Layer", |map, ids| {
            move_objects_to_layer(map, ids, layer).map(|_| ())
        })
    }

    /// Whether objects were copied.
    pub(crate) fn has_copied_objects(&self) -> bool {
        self.clipboard.objects.is_some()
    }

    /// Why Raise and Lower cannot move the selected objects: their layer draws top-down.
    pub(crate) fn restack_reason(&self, doc: DocumentId) -> Option<String> {
        let map = self.map_of(doc)?;
        let layer = self.documents.get(doc)?.selection().current_layer?;
        let objects = map.layer(layer)?.as_object_layer()?;
        (objects.draw_order == DrawOrder::TopDown).then(|| {
            "The objects of this layer are drawn top-down: set its draw order to Manual to order them by hand"
                .to_owned()
        })
    }

    /// What to say about locked layers whose objects an edit left alone.
    pub(crate) fn locked_objects_notice(
        &self,
        doc: DocumentId,
        layers: &[LayerId],
    ) -> Option<Effect> {
        let map = self.map_of(doc)?;
        let names: Vec<String> = layers
            .iter()
            .filter_map(|id| map.layer(*id).map(|l| l.name.clone()))
            .collect();
        let quoted = bt_utils::text::quoted_list(&names);
        match names.len() {
            0 => None,
            1 => Some(Effect::Notify(Notice::info(format!(
                "{quoted} is locked: its objects were left as they were"
            )))),
            _ => Some(Effect::Notify(Notice::info(format!(
                "{quoted} are locked: their objects were left as they were"
            )))),
        }
    }
}
