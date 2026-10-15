//! PNG export options; the chosen file is handled by a window worker.

use bt_editor::{DocumentId, FileRequest};

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExportImageDialog {
    doc: DocumentId,
    scale_percent: u32,
    background: bool,
}

impl ExportImageDialog {
    pub(crate) fn new(doc: DocumentId) -> ExportImageDialog {
        ExportImageDialog {
            doc,
            scale_percent: 100,
            background: true,
        }
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, requests: &mut Vec<FileRequest>) -> bool {
        let editing = ctx.egui_wants_keyboard_input();
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("export-image")).show(ctx, |ui| {
            ui.heading("Export as Image");
            ui.horizontal(|ui| {
                let label = ui.label("Scale");
                ui.add(
                    egui::DragValue::new(&mut self.scale_percent)
                        .range(1..=800)
                        .suffix(" %"),
                )
                .labelled_by(label.id);
            });
            ui.checkbox(&mut self.background, "Include map background colour");
            ui.label("Exports the visible layers of the current map as a PNG.");
            answer = dialog_buttons(ui, true);
        });
        let keys = DialogKeys::take(ctx);
        if answer == ButtonsAnswer::Ok || (keys.enter && !editing) {
            requests.push(FileRequest::ExportImage {
                doc: self.doc,
                scale_percent: self.scale_percent,
                background: self.background,
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }
}
