//! The Layer menu's commands, each one step of the history: new layers, Layer via Copy and
//! Cut, groups, duplicates, merging down, removing, moving up and down, choosing the next or
//! previous layer, and showing, hiding, locking and unlocking the selected or the other layers;
//! and the Layers panel's drag and drop and opacity slider. The operations are `bt_edit`'s; this
//! decides what becomes selected afterwards.

use bt_core::{LayerId, LayerKind, Map};
use bt_edit::{
    LayerLocation, LayerType, LayerViaSource, LayersReport, can_merge_down, duplicate_layers,
    group_layers, layer_order, layer_via_copy, layer_via_cut, lower_layers, merge_layers_down,
    move_layers, new_layer, other_layers, raise_layers, remove_layers, toggle_locked,
    toggle_visible, ungroup_layers,
};

use crate::document::{DocumentId, Selection};
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::history::MergeKey;
use crate::notice::Notice;

/// What becomes selected after a layer command.
enum After {
    /// The selection stays.
    Same,
    /// The layers the command made, the first current.
    Created,
    /// The layer the command left in place of the current one.
    Replacement,
}

impl Editor {
    /// Adds a layer of `kind` above the current one (at the bottom of an empty map); it becomes
    /// current.
    pub(crate) fn add_layer(&mut self, doc: DocumentId, kind: LayerType) -> Vec<Effect> {
        let label = match kind {
            LayerType::Tile => "New Tile Layer",
            LayerType::Object => "New Object Layer",
            LayerType::Image => "New Image Layer",
            LayerType::Group => "New Group Layer",
        };
        self.layer_command(doc, label, After::Created, |map, selection| {
            let id = new_layer(map, kind, selection.current_layer)?;
            Ok(LayersReport {
                created: vec![id],
                ..LayersReport::default()
            })
        })
    }

    /// Duplicates the selected layers; the copies become selected.
    pub(crate) fn duplicate_selected_layers(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.layer_command(doc, "Duplicate Layers", After::Created, |map, selection| {
            Ok(duplicate_layers(map, &selection.layers, true)?)
        })
    }

    /// Removes the selected layers; the layer below them (or above) becomes current.
    pub(crate) fn remove_selected_layers(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.layer_command(
            doc,
            "Remove Layers",
            After::Replacement,
            |map, selection| Ok(remove_layers(map, &selection.layers)?),
        )
    }

    /// Puts the selected layers in a new group, which becomes current.
    pub(crate) fn group_selected_layers(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.layer_command(doc, "Group Layers", After::Created, |map, selection| {
            Ok(group_layers(map, &selection.layers)?)
        })
    }

    /// Takes the selected groups' layers out of them.
    pub(crate) fn ungroup_selected_layers(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.layer_command(
            doc,
            "Ungroup Layers",
            After::Replacement,
            |map, selection| Ok(ungroup_layers(map, &selection.layers)?),
        )
    }

    /// Merges the current layer into the one below, which becomes current.
    pub(crate) fn merge_current_layer_down(&mut self, doc: DocumentId) -> Vec<Effect> {
        self.layer_command(
            doc,
            "Merge Layer Down",
            After::Replacement,
            |map, selection| {
                let ids: Vec<LayerId> = selection.current_layer.into_iter().collect();
                Ok(merge_layers_down(map, &ids)?)
            },
        )
    }

    /// Moves the selected layers up or down a place.
    pub(crate) fn step_selected_layers(&mut self, doc: DocumentId, up: bool) -> Vec<Effect> {
        let label = if up { "Raise Layers" } else { "Lower Layers" };
        self.layer_command(doc, label, After::Same, |map, selection| {
            Ok(if up {
                raise_layers(map, &selection.layers)?
            } else {
                lower_layers(map, &selection.layers)?
            })
        })
    }

    /// Layer via Copy or Cut: the selected tiles of the selected tile layers, or the selected
    /// objects, into new layers above their own; the new layers become selected.
    pub(crate) fn layer_via(&mut self, doc: DocumentId, cut: bool) -> Vec<Effect> {
        let label = if cut {
            "Layer via Cut"
        } else {
            "Layer via Copy"
        };
        self.layer_command(doc, label, After::Created, |map, selection| {
            let tile_layers = selected_tile_layers(map, selection);
            let source = if !selection.tiles.is_empty() && !tile_layers.is_empty() {
                LayerViaSource::Tiles {
                    layers: &tile_layers,
                    region: &selection.tiles,
                }
            } else {
                LayerViaSource::Objects(&selection.objects)
            };
            Ok(if cut {
                layer_via_cut(map, source)?
            } else {
                layer_via_copy(map, source)?
            })
        })
    }

    /// Shows or hides the selected layers (all are shown when all were hidden, else all are
    /// hidden), or locks or unlocks them (all are locked when any was unlocked).
    pub(crate) fn toggle_selected_layers(&mut self, doc: DocumentId, lock: bool) -> Vec<Effect> {
        let label = if lock {
            "Lock/Unlock Layers"
        } else {
            "Show/Hide Layers"
        };
        self.layer_command(doc, label, After::Same, |map, selection| {
            Ok(if lock {
                toggle_locked(map, &selection.layers)?
            } else {
                toggle_visible(map, &selection.layers)?
            })
        })
    }

    /// Shows or hides (locks or unlocks) every layer but the selected ones and their groups.
    pub(crate) fn toggle_other_layers(&mut self, doc: DocumentId, lock: bool) -> Vec<Effect> {
        let label = if lock {
            "Lock/Unlock Other Layers"
        } else {
            "Show/Hide Other Layers"
        };
        self.layer_command(doc, label, After::Same, |map, selection| {
            let others = other_layers(map, &selection.layers)?;
            Ok(if lock {
                toggle_locked(map, &others)?
            } else {
                toggle_visible(map, &others)?
            })
        })
    }

    /// Drags layers to `to` as one step (Tiled's `LayerModel::dropMimeData`): a layer inside a
    /// group dragged with it stays in its group, which carries it. The selection stays.
    pub(crate) fn move_layers_to(
        &mut self,
        doc: DocumentId,
        layers: &[LayerId],
        to: LayerLocation,
    ) -> Vec<Effect> {
        let Some(map) = self.map_of(doc) else {
            return Vec::new();
        };
        let outermost: Vec<LayerId> = layers
            .iter()
            .copied()
            .filter(|id| {
                map.layer_path(*id).is_none_or(|path| {
                    !path
                        .iter()
                        .any(|outer| outer != id && layers.contains(outer))
                })
            })
            .collect();
        let label = if outermost.len() == 1 {
            "Drag Layer"
        } else {
            "Drag Layers"
        };
        self.layer_command(doc, label, After::Same, |map, _| {
            Ok(move_layers(map, &outermost, to.parent, to.index)?)
        })
    }

    /// Sets layers' opacity; the changes of one drag of the slider fold into one step, which
    /// `done` ends. An opacity outside 0 to 1 changes nothing and says so.
    pub(crate) fn set_layers_opacity(
        &mut self,
        doc: DocumentId,
        layers: &[LayerId],
        opacity: f64,
        done: bool,
    ) -> Vec<Effect> {
        if !(0.0..=1.0).contains(&opacity) {
            return vec![Effect::Notify(Notice::warning(format!(
                "An opacity is from 0 to 1, not {opacity}"
            )))];
        }
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let key = MergeKey(format!("layer opacity {layers:?}"));
        let result = self.transact_merging(doc, "Change Layer Opacity", key, |tx| {
            let map = tx.map_mut(asset)?;
            for id in layers {
                map.layer_mut(*id)
                    .ok_or(EditError::NoSuchLayer(*id))?
                    .opacity = opacity;
            }
            Ok::<_, EditError>(())
        });
        if done {
            self.break_merge(doc);
        }
        result.err().map(warn).into_iter().collect()
    }

    /// Makes the next layer above (or below) the current one current, in draw order.
    pub(crate) fn select_neighbour_layer(&mut self, doc: DocumentId, above: bool) {
        let Some(map) = self.map_of(doc).cloned() else {
            return;
        };
        let order = layer_order(&map);
        let current = self
            .documents
            .get(doc)
            .and_then(|d| d.selection().current_layer);
        let position = current.and_then(|id| order.iter().position(|o| *o == id));
        let next = match (position, above) {
            (Some(at), true) => order.get(at + 1),
            (Some(at), false) => at.checked_sub(1).and_then(|i| order.get(i)),
            (None, true) => order.first(),
            (None, false) => order.last(),
        };
        if let Some(&layer) = next {
            self.select_layer(doc, layer);
        }
    }

    /// Selects every layer; the current one stays current.
    pub(crate) fn select_all_layers(&mut self, doc: DocumentId) {
        let Some(map) = self.map_of(doc).cloned() else {
            return;
        };
        let all = layer_order(&map);
        if let Some(document) = self.documents.get_mut(doc) {
            document.selection.layers = all;
        }
    }

    /// Whether the current layer can be merged into the one below.
    pub(crate) fn current_layer_merges_down(&self, doc: DocumentId) -> bool {
        let (Some(map), Some(current)) = (
            self.map_of(doc),
            self.documents
                .get(doc)
                .and_then(|d| d.selection().current_layer),
        ) else {
            return false;
        };
        can_merge_down(map, current).unwrap_or(false)
    }

    /// Runs a layer operation as one step, then selects what `after` says.
    fn layer_command(
        &mut self,
        doc: DocumentId,
        label: &str,
        after: After,
        operation: impl FnOnce(&mut Map, &Selection) -> Result<LayersReport, EditError>,
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let result = self.transact(doc, label, |tx| {
            let before = tx.selection.clone();
            let map = tx.map_mut(asset)?;
            let order_before = layer_order(map);
            let report = operation(map, &before)?;
            let selection = match after {
                After::Same => before.clone(),
                After::Created => match report.created.first() {
                    Some(&first) => Selection {
                        current_layer: Some(first),
                        layers: report.created.clone(),
                        ..before.clone()
                    },
                    None => before.clone(),
                },
                After::Replacement => replacement(map, &before, &order_before),
            };
            tx.selection = selection;
            Ok::<_, EditError>(())
        });
        let family = self.tool_family(doc);
        self.tools.follow_layer(family);
        result.err().map(warn).into_iter().collect()
    }
}

/// The selection after a command took layers away or changed them: the current layer if it is
/// still there, else the nearest layer below it in the old draw order that still is, else the
/// topmost.
fn replacement(map: &Map, before: &Selection, order_before: &[LayerId]) -> Selection {
    let exists = |id: &LayerId| map.layer(*id).is_some();
    let current = before.current_layer.filter(exists).or_else(|| {
        let at = before
            .current_layer
            .and_then(|id| order_before.iter().position(|o| *o == id))?;
        order_before
            .get(..at)?
            .iter()
            .rev()
            .chain(order_before.get(at..).unwrap_or_default())
            .copied()
            .find(exists)
    });
    let current = current.or_else(|| layer_order(map).last().copied());
    Selection {
        current_layer: current,
        layers: current.into_iter().collect(),
        ..before.clone()
    }
}

/// The selected tile layers, or the current one when it is a tile layer and none is selected.
fn selected_tile_layers(map: &Map, selection: &Selection) -> Vec<LayerId> {
    let is_tiles = |id: &LayerId| {
        map.layer(*id)
            .is_some_and(|l| matches!(l.kind, LayerKind::Tile(_)))
    };
    let selected: Vec<LayerId> = selection.layers.iter().copied().filter(is_tiles).collect();
    if selected.is_empty() {
        selection
            .current_layer
            .filter(is_tiles)
            .into_iter()
            .collect()
    } else {
        selected
    }
}

/// Says that the layers of `locked` were left as they were because they are locked: `one` ends
/// the sentence for one layer ("its contents stayed where they were"), `many` for several.
/// Nothing when no layer is locked.
pub(crate) fn locked_layers_notice(
    map: &Map,
    locked: &[LayerId],
    one: &str,
    many: &str,
) -> Option<Effect> {
    let names: Vec<String> = locked
        .iter()
        .filter_map(|id| map.layer(*id).map(|l| l.name.clone()))
        .collect();
    let quoted = bt_utils::text::quoted_list(&names);
    match names.len() {
        0 => None,
        1 => Some(Effect::Notify(Notice::info(format!(
            "{quoted} is locked: {one}"
        )))),
        _ => Some(Effect::Notify(Notice::info(format!(
            "{quoted} are locked: {many}"
        )))),
    }
}
