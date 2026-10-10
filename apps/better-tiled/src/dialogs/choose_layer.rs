//! Move Objects to Layer: the map's other object layers as a list; clicking one moves the
//! selected objects there. Escape cancels.

use bt_core::{LayerId, LayerKind};
use bt_editor::{DocumentId, Editor, Intent};

use super::form::DialogKeys;

/// The dialog's state: the document whose objects move.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChooseLayerDialog {
    doc: DocumentId,
}

impl ChooseLayerDialog {
    /// A dialog for the selected objects of `doc`.
    pub(crate) fn new(doc: DocumentId) -> ChooseLayerDialog {
        ChooseLayerDialog { doc }
    }

    /// Shows the dialog; the chosen layer goes to `out`. Whether it closed.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &Editor,
        out: &mut Vec<Intent>,
    ) -> bool {
        let keys = DialogKeys::take(ctx);
        let current = editor
            .document(self.doc)
            .and_then(|d| d.selection().current_layer);
        let mut layers: Vec<(LayerId, String)> = Vec::new();
        if let Some(map) = editor.map_of(self.doc) {
            map.for_each_layer(|layer, _| {
                if matches!(layer.kind, LayerKind::Object(_)) && Some(layer.id) != current {
                    layers.push((layer.id, layer.name.clone()));
                }
            });
        }
        layers.reverse();
        let mut chosen = None;
        let mut cancelled = false;
        let response = egui::Modal::new(egui::Id::new("move_objects_to_layer")).show(ctx, |ui| {
            ui.set_min_width(260.0);
            ui.heading("Move Objects to Layer");
            ui.add_space(4.0);
            if layers.is_empty() {
                ui.weak("The map has no other object layer.");
            }
            for (id, name) in &layers {
                if ui.button(name).clicked() {
                    chosen = Some(*id);
                }
            }
            ui.add_space(4.0);
            if ui.button("Cancel").clicked() {
                cancelled = true;
            }
        });
        if let Some(layer) = chosen {
            out.push(Intent::MoveObjectsToLayer {
                doc: self.doc,
                layer,
            });
            return true;
        }
        cancelled || keys.escape || response.should_close()
    }
}
