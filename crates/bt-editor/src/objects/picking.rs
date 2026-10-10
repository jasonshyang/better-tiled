//! Which objects the pointer or a rubber band picks, as the object tools ask: from the visible,
//! unlocked object layers the object-selection preference allows, topmost first, each layer's
//! offset taken into account; and where the selected objects are on screen, for highlighting
//! and handles. Positions given and returned are in the map's screen space (world pixels),
//! where the canvas measures.

use std::collections::HashSet;

use bt_core::{LayerId, LayerKind, Map, ObjectId};
use bt_edit::{ObjectSpace, RectMode, layer_order, object_bounds, objects_at, objects_in_quad};
use bt_utils::DRect;
use glam::DVec2;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::settings::ObjectSelection;

/// Where a selected object is on screen: the corners of its bounds, around it.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectFrame {
    /// The object.
    pub id: ObjectId,
    /// The corners, in the map's screen space.
    pub corners: [DVec2; 4],
}

impl Editor {
    /// The objects a click at `world` picks, topmost first: those within `tolerance` world
    /// pixels of it on the layers the object-selection preference allows (the current layer's
    /// first when it prefers it). Hidden and locked layers give none.
    pub fn objects_at(&self, doc: DocumentId, world: DVec2, tolerance: f64) -> Vec<ObjectId> {
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let space = ObjectSpace::of_map(map);
        let geometry = map.geometry();
        let mut found = Vec::new();
        for layer in self.pickable_layers(doc, map) {
            let (Some(objects), Some(state)) = (
                map.layer(layer).and_then(|l| l.as_object_layer()),
                map.effective_layer(layer),
            ) else {
                continue;
            };
            let pixel = geometry.screen_to_pixel(world - state.offset);
            found.extend(objects_at(objects, &space, pixel, tolerance));
        }
        found
    }

    /// The objects a rubber band over `rect` (world) selects: those it touches, or with the
    /// preference those it encloses.
    pub fn objects_in_rect(&self, doc: DocumentId, rect: DRect) -> Vec<ObjectId> {
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let mode = if self.settings.behaviour.select_enclosed {
            RectMode::Enclosed
        } else {
            RectMode::Touching
        };
        let space = ObjectSpace::of_map(map);
        let geometry = map.geometry();
        let mut found = Vec::new();
        for layer in self.pickable_layers(doc, map) {
            let (Some(objects), Some(state)) = (
                map.layer(layer).and_then(|l| l.as_object_layer()),
                map.effective_layer(layer),
            ) else {
                continue;
            };
            let corners = [
                rect.min,
                DVec2::new(rect.max.x, rect.min.y),
                rect.max,
                DVec2::new(rect.min.x, rect.max.y),
            ]
            .map(|corner| geometry.screen_to_pixel(corner - state.offset));
            found.extend(objects_in_quad(objects, &space, corners, mode));
        }
        found
    }

    /// Where the objects with `ids` are on screen.
    pub fn object_frames(&self, doc: DocumentId, ids: &[ObjectId]) -> Vec<ObjectFrame> {
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let space = ObjectSpace::of_map(map);
        let geometry = map.geometry();
        let wanted: HashSet<ObjectId> = ids.iter().copied().collect();
        let mut frames = Vec::new();
        map.for_each_layer(|layer, _| {
            let (Some(objects), Some(state)) =
                (layer.as_object_layer(), map.effective_layer(layer.id))
            else {
                return;
            };
            for object in objects.objects.iter().filter(|o| wanted.contains(&o.id)) {
                let bounds = object_bounds(&space, object);
                let corners = [
                    bounds.min,
                    DVec2::new(bounds.max.x, bounds.min.y),
                    bounds.max,
                    DVec2::new(bounds.min.x, bounds.max.y),
                ]
                .map(|corner| geometry.pixel_to_screen(corner) + state.offset);
                frames.push(ObjectFrame {
                    id: object.id,
                    corners,
                });
            }
        });
        frames
    }

    /// The screen rectangle holding the objects with `ids`; `None` when none is found.
    pub fn objects_screen_bounds(&self, doc: DocumentId, ids: &[ObjectId]) -> Option<DRect> {
        DRect::from_points(
            self.object_frames(doc, ids)
                .iter()
                .flat_map(|frame| frame.corners),
        )
    }

    /// The object layers a click may pick from, the first tried first: shown and unlocked ones,
    /// topmost first, the current one first when the preference prefers it, or only it.
    fn pickable_layers(&self, doc: DocumentId, map: &Map) -> Vec<LayerId> {
        let current = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().current_layer);
        let usable = |id: &LayerId| {
            let state = map.effective_layer(*id);
            let objects = map
                .layer(*id)
                .is_some_and(|l| matches!(l.kind, LayerKind::Object(_)));
            objects && state.is_some_and(|s| s.visible && !s.locked)
        };
        let mut layers: Vec<LayerId> = layer_order(map).into_iter().rev().filter(usable).collect();
        match self.settings.behaviour.object_selection {
            ObjectSelection::AnyLayer => {}
            ObjectSelection::PreferCurrentLayer => {
                if let Some(at) = layers.iter().position(|id| Some(*id) == current) {
                    let first = layers.remove(at);
                    layers.insert(0, first);
                }
            }
            ObjectSelection::CurrentLayerOnly => layers.retain(|id| Some(*id) == current),
        }
        layers
    }
}
