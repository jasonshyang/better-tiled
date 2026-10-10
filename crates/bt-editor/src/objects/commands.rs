//! The object commands: selecting objects, inserting one, the Objects menu with Delete, Cut,
//! Copy and Paste on objects, and the Objects panel's eye, centring and drag and drop, on a
//! map's object layers or (most of them) on a tile's collision shapes (`host`). Each change is
//! one step; objects on locked layers are left where they are and the locks are named.

use std::collections::HashSet;

use bt_core::{DrawOrder, LayerId, Map, MapObject, ObjectId, ObjectLayer, ObjectOverrides};
use bt_edit::{
    Axis, NewIds, ObjectClip, ObjectSpace, Pivot, SelectionMode, add_object, add_object_in,
    copy_objects, copy_objects_in, cut_objects, cut_objects_in, duplicate_objects,
    duplicate_objects_in, flip_objects, flip_objects_in, flip_origin, flip_origin_in,
    lower_objects, lower_objects_in, lower_objects_to_bottom, lower_objects_to_bottom_in,
    move_objects_to_layer, object_mut, paste_objects, paste_objects_in, paste_offset,
    raise_objects, raise_objects_in, raise_objects_to_top, raise_objects_to_top_in, remove_objects,
    remove_objects_in, rotate_objects, rotate_objects_in,
};
use glam::DVec2;

use super::host::{ObjectHost, change_tile_shapes};
use super::snapping::SnapToggle;
use super::stroke::unlocked_objects;
use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::{Effect, ViewRequest};
use crate::error::EditError;
use crate::notice::Notice;

/// Why the objects of a layer drawn top-down cannot be ordered by hand.
const TOP_DOWN_ORDER: &str = "The objects of this layer are drawn top-down: set its draw order to Manual to order them by hand";

/// The object layer holding every one of `ids`, when one does.
fn single_layer_of(map: &Map, ids: &[ObjectId]) -> Option<LayerId> {
    let wanted: HashSet<ObjectId> = ids.iter().copied().collect();
    let mut holders = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer()
            && objects.objects.iter().any(|o| wanted.contains(&o.id))
        {
            holders.push(layer.id);
        }
    });
    match holders[..] {
        [only] => Some(only),
        _ => None,
    }
}

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
    /// Changes the selected objects: `ids` combined with them as `mode` says. When they all lie
    /// on one object layer, it becomes current, and the only selected layer unless it was
    /// selected (Tiled's `MapDocument::setSelectedObjects`).
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
        let selected = document.selection.objects.clone();
        document
            .selection
            .nodes
            .retain(|node| selected.contains(&node.object));
        // A map's objects all on one layer make it current; a tile's shapes have no layer.
        if !matches!(self.object_host(doc), Some(ObjectHost::Map(_))) {
            return;
        }
        let holder = self
            .map_of(doc)
            .and_then(|map| single_layer_of(map, &selected));
        let Some(layer) = holder else {
            return;
        };
        let Some(document) = self.documents.get_mut(doc) else {
            return;
        };
        let selection = &document.selection;
        if selection.current_layer == Some(layer) {
            return;
        }
        document.selection = if selection.layers.contains(&layer) {
            selection.with_layers(layer, &selection.layers)
        } else {
            selection.with_current_layer(layer)
        };
        let family = self.tool_family(doc);
        self.tools.follow_layer(family);
        self.session_changed();
    }

    /// Shows or hides an object as one step; a template instance keeps its visibility as its
    /// own.
    pub(crate) fn set_object_visible(
        &mut self,
        doc: DocumentId,
        object: ObjectId,
        visible: bool,
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let label = if visible {
            "Show Object"
        } else {
            "Hide Object"
        };
        let result = self.transact(doc, label, |tx| {
            let found = object_mut(tx.map_mut(asset)?, object)?;
            if found.visible != visible {
                found.visible = visible;
                if found.template.is_some() {
                    found.overrides |= ObjectOverrides::VISIBLE;
                }
            }
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }

    /// Asks the window to centre the view on an object: on the middle of what it covers.
    pub(crate) fn center_on_object(&self, doc: DocumentId, object: ObjectId) -> Vec<Effect> {
        match self.objects_screen_bounds(doc, &[object]) {
            Some(bounds) => vec![Effect::View(ViewRequest::CenterOn(
                (bounds.min + bounds.max) / 2.0,
            ))],
            None => vec![warn(bt_edit::EditError::NoSuchObject(object).into())],
        }
    }

    /// The Objects panel's drag: moves objects to the place `index` of the object layer
    /// `layer`'s list as one step, "Reorder Objects" within their layer and "Move Objects to
    /// Layer" from others. Objects on locked layers stay and their layers are named; a locked
    /// layer takes none; a layer drawn top-down is not reordered, since its list order draws
    /// nothing.
    pub(crate) fn reorder_objects(
        &mut self,
        doc: DocumentId,
        ids: &[ObjectId],
        layer: LayerId,
        index: usize,
    ) -> Vec<Effect> {
        let (Some(asset), Some(map)) = (
            self.documents.get(doc).map(|d| d.asset()),
            self.map_of(doc).cloned(),
        ) else {
            return Vec::new();
        };
        let Some(target) = map.layer(layer) else {
            return vec![warn(EditError::NoSuchLayer(layer))];
        };
        let Some(objects) = target.as_object_layer() else {
            return vec![warn(bt_edit::EditError::NotAnObjectLayer(layer).into())];
        };
        if map.effective_layer(layer).is_some_and(|l| l.locked) {
            return self
                .locked_objects_notice(doc, &[layer])
                .into_iter()
                .collect();
        }
        let split = unlocked_objects(&map, ids);
        let mut effects: Vec<Effect> = self
            .locked_objects_notice(doc, &split.locked_layers)
            .into_iter()
            .collect();
        if split.unlocked.is_empty() {
            return effects;
        }
        let within = split
            .unlocked
            .iter()
            .all(|id| map.object_layer_of(*id) == Some(layer));
        if within && objects.draw_order == DrawOrder::TopDown {
            effects.push(Effect::Notify(Notice::info(TOP_DOWN_ORDER)));
            return effects;
        }
        let label = if within {
            "Reorder Objects"
        } else {
            "Move Objects to Layer"
        };
        let result = self.transact(doc, label, |tx| {
            bt_edit::reorder_objects(tx.map_mut(asset)?, &split.unlocked, layer, index)
                .map_err(EditError::from)
        });
        effects.extend(result.err().map(warn));
        effects
    }

    /// Inserts an object into the current object layer, or among the current tile's collision
    /// shapes, as one step; it becomes the selection.
    pub(crate) fn insert_object(&mut self, doc: DocumentId, object: MapObject) -> Vec<Effect> {
        if let Some(ObjectHost::Tile { home, tile }) = self.object_host(doc) {
            let result = self.transact(doc, "Insert Object", |tx| {
                let id = change_tile_shapes(tx, home, tile, |objects, _| {
                    let mut ids = NewIds::after_highest(objects);
                    add_object_in(objects, object, None, &mut ids)
                })?;
                tx.selection.objects = vec![id];
                Ok::<_, EditError>(())
            });
            return result.err().map(warn).into_iter().collect();
        }
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

    /// Runs a change on the selected objects as one step labelled `label`: `on_map` on those of
    /// a map's unlocked layers (the locked layers holding the others are named), `on_shapes` on
    /// a tile's collision shapes.
    fn on_selected_objects(
        &mut self,
        doc: DocumentId,
        label: &str,
        on_map: impl FnOnce(&mut bt_core::Map, &[ObjectId]) -> Result<(), bt_edit::EditError>,
        on_shapes: impl FnOnce(
            &mut ObjectLayer,
            &ObjectSpace<'_>,
            &[ObjectId],
        ) -> Result<(), bt_edit::EditError>,
    ) -> Vec<Effect> {
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        if let Some(ObjectHost::Tile { home, tile }) = self.object_host(doc) {
            if selected.is_empty() {
                return Vec::new();
            }
            let result = self.transact(doc, label, |tx| {
                change_tile_shapes(tx, home, tile, |objects, space| {
                    on_shapes(objects, space, &selected)
                })
            });
            return result.err().map(warn).into_iter().collect();
        }
        let (Some(asset), Some(map)) = (
            self.documents.get(doc).map(|d| d.asset()),
            self.map_of(doc).cloned(),
        ) else {
            return Vec::new();
        };
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
            on_map(tx.map_mut(asset)?, &ids).map_err(EditError::from)
        });
        effects.extend(result.err().map(warn));
        effects
    }

    /// Delete on objects.
    pub(crate) fn delete_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.on_selected_objects(
            doc,
            "Delete Objects",
            |map, ids| remove_objects(map, ids).map(|_| ()),
            |objects, _, ids| remove_objects_in(objects, ids).map(|_| ()),
        )
    }

    /// Copy on objects.
    pub(crate) fn copy_selected_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        let selected = self
            .documents
            .get(doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        let copied = match self.object_host(doc) {
            Some(ObjectHost::Tile { home, tile }) => {
                let Some(shapes) = self.tile_shapes(home, tile) else {
                    return Vec::new();
                };
                copy_objects_in(&shapes.shapes, &shapes.space(), &selected)
            }
            Some(ObjectHost::Map(_)) => {
                let Some(map) = self.map_of(doc).cloned() else {
                    return Vec::new();
                };
                copy_objects(&map, &selected)
            }
            None => return Vec::new(),
        };
        match copied {
            Ok(clip) => {
                self.clipboard.objects = Some(clip);
                Vec::new()
            }
            Err(error) => vec![warn(error.into())],
        }
    }

    /// Cut on objects: Copy, then the unlocked ones are removed.
    pub(crate) fn cut_selected_objects(&mut self, doc: DocumentId) -> Vec<Effect> {
        let copied: std::cell::Cell<Option<ObjectClip>> = std::cell::Cell::new(None);
        let effects = self.on_selected_objects(
            doc,
            "Cut Objects",
            |map, ids| {
                copied.set(Some(cut_objects(map, ids)?));
                Ok(())
            },
            |objects, space, ids| {
                copied.set(Some(cut_objects_in(objects, space, ids)?));
                Ok(())
            },
        );
        if let Some(clip) = copied.into_inner() {
            self.clipboard.objects = Some(clip);
        }
        effects
    }

    /// Paste on objects: into the current object layer (or among the current tile's collision
    /// shapes), centred on `at` (pixel space) and snapped when given, else where they were
    /// copied from; they become the selection.
    pub(crate) fn paste_copied_objects(
        &mut self,
        doc: DocumentId,
        at: Option<DVec2>,
    ) -> Vec<Effect> {
        if let Some(ObjectHost::Tile { home, tile }) = self.object_host(doc) {
            let (Some(clip), Some(shapes)) =
                (self.clipboard.objects.clone(), self.tile_shapes(home, tile))
            else {
                return Vec::new();
            };
            let snapping = self.snap_mode(SnapToggle::Keep);
            let offset = at.map_or(DVec2::ZERO, |cursor| {
                paste_offset(shapes.space().geometry(), &clip, cursor, snapping)
            });
            let result = self.transact(doc, "Paste Objects", |tx| {
                let report = change_tile_shapes(tx, home, tile, |objects, _| {
                    let mut ids = NewIds::after_highest(objects);
                    paste_objects_in(objects, &clip, offset, &mut ids)
                })?;
                tx.selection.objects = report.changed;
                Ok::<_, EditError>(())
            });
            return result.err().map(warn).into_iter().collect();
        }
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
            Some(cursor) => paste_offset(
                &map.geometry(),
                &clip,
                cursor,
                self.snap_mode(SnapToggle::Keep),
            ),
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
        if let Some(ObjectHost::Tile { home, tile }) = self.object_host(doc) {
            let selected = self
                .documents
                .get(doc)
                .map(|d| d.selection().objects.clone())
                .unwrap_or_default();
            let result = self.transact(doc, "Duplicate Objects", |tx| {
                let copies = change_tile_shapes(tx, home, tile, |objects, _| {
                    let mut ids = NewIds::after_highest(objects);
                    duplicate_objects_in(objects, &selected, DVec2::ZERO, false, &mut ids)
                })?
                .changed;
                tx.selection.objects = copies;
                Ok::<_, EditError>(())
            });
            return result.err().map(warn).into_iter().collect();
        }
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
        self.on_selected_objects(
            doc,
            label,
            |map, ids| {
                match how {
                    Restack::Raise => raise_objects(map, ids)?,
                    Restack::Lower => lower_objects(map, ids)?,
                    Restack::ToTop => raise_objects_to_top(map, ids)?,
                    Restack::ToBottom => lower_objects_to_bottom(map, ids)?,
                };
                Ok(())
            },
            |objects, space, ids| {
                match how {
                    Restack::Raise => raise_objects_in(objects, space, ids)?,
                    Restack::Lower => lower_objects_in(objects, space, ids)?,
                    Restack::ToTop => raise_objects_to_top_in(objects, ids)?,
                    Restack::ToBottom => lower_objects_to_bottom_in(objects, ids)?,
                };
                Ok(())
            },
        )
    }

    /// Flips the selected objects around the middle of what they cover.
    pub(crate) fn flip_selected_objects(&mut self, doc: DocumentId, axis: Axis) -> Vec<Effect> {
        let label = match axis {
            Axis::Horizontal => "Flip Horizontally",
            Axis::Vertical => "Flip Vertically",
        };
        self.on_selected_objects(
            doc,
            label,
            |map, ids| {
                let Some(origin) = flip_origin(map, ids)? else {
                    return Ok(());
                };
                flip_objects(map, ids, axis, origin)?;
                Ok(())
            },
            |objects, space, ids| {
                let Some(origin) = flip_origin_in(objects, space, ids)? else {
                    return Ok(());
                };
                flip_objects_in(objects, space, ids, axis, origin)?;
                Ok(())
            },
        )
    }

    /// Turns each selected object a quarter around its own position (Tiled's Rotate Left and
    /// Right).
    pub(crate) fn rotate_selected_objects(&mut self, doc: DocumentId, degrees: f64) -> Vec<Effect> {
        let label = if degrees > 0.0 {
            "Rotate Right"
        } else {
            "Rotate Left"
        };
        self.on_selected_objects(
            doc,
            label,
            |map, ids| rotate_objects(map, ids, degrees, Pivot::OwnPosition).map(|_| ()),
            |objects, space, ids| {
                rotate_objects_in(objects, space, ids, degrees, Pivot::OwnPosition).map(|_| ())
            },
        )
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
        if !matches!(self.object_host(doc), Some(ObjectHost::Map(_))) {
            return Vec::new();
        }
        self.on_selected_objects(
            doc,
            "Move Objects to Layer",
            |map, ids| move_objects_to_layer(map, ids, layer).map(|_| ()),
            |_, _, _| Ok(()),
        )
    }

    /// Whether objects were copied.
    pub(crate) fn has_copied_objects(&self) -> bool {
        self.clipboard.objects.is_some()
    }

    /// Why Raise and Lower cannot move the selected objects: their layer draws top-down.
    pub(crate) fn restack_reason(&self, doc: DocumentId) -> Option<String> {
        let draw_order = match self.object_host(doc)? {
            ObjectHost::Tile { home, tile } => self.tile_shapes(home, tile)?.shapes.draw_order,
            ObjectHost::Map(_) => {
                let map = self.map_of(doc)?;
                let layer = self.documents.get(doc)?.selection().current_layer?;
                map.layer(layer)?.as_object_layer()?.draw_order
            }
        };
        (draw_order == DrawOrder::TopDown).then(|| TOP_DOWN_ORDER.to_owned())
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
