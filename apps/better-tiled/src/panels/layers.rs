//! The Layers panel: the map's layers as a tree, topmost first, with visibility and lock
//! toggles, selection, and renaming in place (double-click; Enter or clicking away keeps the
//! name, Escape drops it).

use std::collections::HashSet;

use bt_core::{Layer, LayerId, LayerKind};
use bt_editor::{DocumentId, Editor, Intent};

/// How far each level of the tree is indented, in points.
const INDENT: f32 = 14.0;

/// The panel's own state.
#[derive(Debug, Default)]
pub(crate) struct LayersPanel {
    renaming: Option<Renaming>,
    collapsed: HashSet<GroupKey>,
}

/// A layer whose name is being edited.
#[derive(Clone, Debug)]
struct Renaming {
    doc: DocumentId,
    layer: LayerId,
    name: String,
    focused: bool,
}

/// A group of a document, folded in the tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct GroupKey {
    doc: DocumentId,
    layer: LayerId,
}

/// What a row needs from the document.
struct RowContext<'a> {
    doc: DocumentId,
    selected: &'a [LayerId],
    current: Option<LayerId>,
}

impl LayersPanel {
    /// Shows the layers of `doc`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        out: &mut Vec<Intent>,
    ) {
        ui.heading("Layers");
        ui.separator();
        let Some(map) = editor.map_of(doc) else {
            return;
        };
        let Some(document) = editor.document(doc) else {
            return;
        };
        let context = RowContext {
            doc,
            selected: &document.selection().layers,
            current: document.selection().current_layer,
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for layer in map.layers.iter().rev() {
                    self.show_layer(ui, layer, 0, &context, out);
                }
            });
    }

    fn show_layer(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        depth: usize,
        context: &RowContext<'_>,
        out: &mut Vec<Intent>,
    ) {
        let key = GroupKey {
            doc: context.doc,
            layer: layer.id,
        };
        ui.horizontal(|ui| {
            ui.add_space(INDENT * depth as f32);
            let eye = if layer.visible { "👁" } else { "  " };
            if ui
                .add(
                    egui::Button::new(eye)
                        .frame(false)
                        .min_size(egui::vec2(18.0, 0.0)),
                )
                .on_hover_text(if layer.visible { "Hide" } else { "Show" })
                .clicked()
            {
                out.push(Intent::SetLayerVisible {
                    doc: context.doc,
                    layer: layer.id,
                    visible: !layer.visible,
                });
            }
            let lock = if layer.locked { "🔒" } else { "  " };
            if ui
                .add(
                    egui::Button::new(lock)
                        .frame(false)
                        .min_size(egui::vec2(18.0, 0.0)),
                )
                .on_hover_text(if layer.locked { "Unlock" } else { "Lock" })
                .clicked()
            {
                out.push(Intent::SetLayerLocked {
                    doc: context.doc,
                    layer: layer.id,
                    locked: !layer.locked,
                });
            }
            if matches!(layer.kind, LayerKind::Group(_)) {
                let folded = self.collapsed.contains(&key);
                let arrow = if folded { "⏵" } else { "⏷" };
                if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                    if folded {
                        self.collapsed.remove(&key);
                    } else {
                        self.collapsed.insert(key);
                    }
                }
            }
            self.show_name(ui, layer, context, out);
        });
        if !self.collapsed.contains(&key) {
            for child in layer.children().iter().rev() {
                self.show_layer(ui, child, depth + 1, context, out);
            }
        }
    }

    fn show_name(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        context: &RowContext<'_>,
        out: &mut Vec<Intent>,
    ) {
        if let Some(renaming) = &mut self.renaming
            && renaming.doc == context.doc
            && renaming.layer == layer.id
        {
            let response = ui.add(
                egui::TextEdit::singleline(&mut renaming.name)
                    .desired_width(f32::INFINITY)
                    .id(egui::Id::new("rename-layer").with(layer.id.0)),
            );
            if !renaming.focused {
                response.request_focus();
                renaming.focused = true;
            }
            if response.lost_focus() {
                let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if !cancelled && renaming.name != layer.name {
                    out.push(Intent::RenameLayer {
                        doc: context.doc,
                        layer: layer.id,
                        name: renaming.name.clone(),
                    });
                }
                self.renaming = None;
            }
            return;
        }
        let selected = context.selected.contains(&layer.id);
        let mut text = egui::RichText::new(display_name(layer));
        if context.current == Some(layer.id) {
            text = text.strong();
        }
        if !layer.visible {
            text = text.weak();
        }
        let response = ui
            .add(egui::Button::selectable(selected, text))
            .on_hover_text(layer.kind_name());
        if response.double_clicked() {
            self.renaming = Some(Renaming {
                doc: context.doc,
                layer: layer.id,
                name: layer.name.clone(),
                focused: false,
            });
        } else if response.clicked() {
            out.push(Intent::SelectLayer {
                doc: context.doc,
                layer: layer.id,
            });
        }
    }
}

/// A layer's name, or what it is when it has none.
fn display_name(layer: &Layer) -> String {
    if layer.name.is_empty() {
        format!("({})", layer.kind_name())
    } else {
        layer.name.clone()
    }
}
