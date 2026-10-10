//! The tab strip: a tab for each document, its name, a dot while it is unsaved, and a close
//! button; middle-click closes too.

use bt_editor::{Editor, Intent};

/// Draws the tabs; choices go to `out`.
pub(crate) fn tab_strip(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    let current = editor.current();
    egui::ScrollArea::horizontal()
        .id_salt("tabs")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for document in editor.documents().in_order() {
                    let Some(asset) = editor.store().get(document.asset()) else {
                        continue;
                    };
                    let dirty = if asset.is_dirty() { " •" } else { "" };
                    let title = format!("{}{dirty}", asset.display_name());
                    let selected = current == Some(document.id());
                    let tab = ui.selectable_label(selected, title);
                    let tab = match asset.path() {
                        Some(path) => tab.on_hover_text(path.display().to_string()),
                        None => tab,
                    };
                    if tab.clicked() {
                        out.push(Intent::SelectDocument(document.id()));
                    }
                    if tab.middle_clicked() {
                        out.push(Intent::CloseDocument(document.id()));
                    }
                    let close = ui
                        .add(egui::Button::new("×").frame(false).small())
                        .on_hover_text("Close");
                    if close.clicked() {
                        out.push(Intent::CloseDocument(document.id()));
                    }
                    ui.separator();
                }
            });
        });
}
