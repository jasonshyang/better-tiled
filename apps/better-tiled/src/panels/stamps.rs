//! Saved stamps, their weighted variations and the nine quick stamps.

use std::collections::BTreeSet;
use std::path::Path;

use super::stamp_previews::StampPreviews;
use crate::workers::Workers;
use bt_editor::{Editor, FileRequest, Intent, SavedStamp, StampEdit};

#[derive(Default)]
pub(crate) struct StampsPanel {
    filter: String,
    name: String,
    previews: StampPreviews,
}

impl StampsPanel {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        workers: &mut Workers,
        out: &mut Vec<Intent>,
        files: &mut Vec<FileRequest>,
    ) {
        ui.heading("Stamps");
        ui.horizontal(|ui| {
            if ui.button("Set Folder…").clicked() {
                files.push(FileRequest::StampsFolder);
            }
            if ui
                .add_enabled(!editor.stamps().busy(), egui::Button::new("Reload"))
                .clicked()
            {
                out.push(Intent::Stamp(StampEdit::Reload));
            }
        });
        let folder = editor.stamps().folder().display().to_string();
        ui.add(egui::Label::new(egui::RichText::new(&folder).small()).truncate())
            .on_hover_text(folder);
        ui.horizontal(|ui| {
            let label = ui.label("Stamp name");
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(ui.available_width()))
                .labelled_by(label.id);
        });
        let can_save =
            !editor.stamps().busy() && !self.name.trim().is_empty() && editor.can_save_stamp();
        if ui
            .add_enabled(can_save, egui::Button::new("Save Current Stamp"))
            .clicked()
        {
            out.push(Intent::Stamp(StampEdit::Save(self.name.trim().to_owned())));
        }
        egui::CollapsingHeader::new("Quick Stamps").show(ui, |ui| {
            let command = if editor.keymap().is_mac() {
                "⌘"
            } else {
                "Ctrl"
            };
            ui.small(format!("1–9: use · {command}+1–9: store · +Shift: extend"));
            for slot in 0..9 {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            editor.tools().quick_stamp(slot).is_some(),
                            egui::Button::new(format!("Use {}", slot + 1)),
                        )
                        .clicked()
                    {
                        out.push(Intent::Stamp(StampEdit::ChooseQuick(slot)));
                    }
                    if ui
                        .add_enabled(
                            editor.can_save_stamp(),
                            egui::Button::new(format!("Store {}", slot + 1)),
                        )
                        .clicked()
                    {
                        out.push(Intent::Stamp(StampEdit::SaveQuick(slot)));
                    }
                    if ui
                        .add_enabled(
                            editor.can_save_stamp(),
                            egui::Button::new(format!("Extend {}", slot + 1)),
                        )
                        .clicked()
                    {
                        out.push(Intent::Stamp(StampEdit::ExtendQuick(slot)));
                    }
                });
            }
        });
        let label = ui.label("Filter stamps");
        ui.text_edit_singleline(&mut self.filter)
            .labelled_by(label.id);
        egui::ScrollArea::vertical()
            .id_salt("stamp-library")
            .show(ui, |ui| {
                self.show_folder(ui, editor, editor.stamps().folder(), workers, out);
                for problem in editor.stamps().problems() {
                    ui.colored_label(ui.visuals().error_fg_color, problem);
                }
            });
    }

    fn show_folder(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        folder: &Path,
        workers: &mut Workers,
        out: &mut Vec<Intent>,
    ) {
        let filter = self.filter.to_lowercase();
        let mut children = BTreeSet::new();
        for saved in editor.stamps().entries() {
            if !saved.stamp.name.to_lowercase().contains(&filter) {
                continue;
            }
            if saved.path.parent() == Some(folder) {
                self.show_stamp(ui, editor, saved, workers, out);
            } else if let Ok(relative) = saved.path.strip_prefix(folder)
                && let Some(component) = relative.components().next()
            {
                children.insert(component.as_os_str().to_owned());
            }
        }
        for name in children {
            let child = folder.join(&name);
            egui::CollapsingHeader::new(name.to_string_lossy())
                .id_salt(&child)
                .default_open(true)
                .show(ui, |ui| self.show_folder(ui, editor, &child, workers, out));
        }
    }

    fn show_stamp(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        saved: &SavedStamp,
        workers: &mut Workers,
        out: &mut Vec<Intent>,
    ) {
        ui.push_id(&saved.path, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                self.previews.show(ui, editor, saved, 0, workers, out);
                if ui.button(&saved.stamp.name).clicked() {
                    out.push(Intent::Stamp(StampEdit::Choose {
                        path: saved.path.clone(),
                        variation: None,
                    }));
                }
            });
            ui.add_enabled_ui(!editor.stamps().busy(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(editor.can_save_stamp(), egui::Button::new("Add Variation"))
                        .clicked()
                    {
                        out.push(Intent::Stamp(StampEdit::AddVariation(saved.path.clone())));
                    }
                    if ui
                        .add_enabled(!self.name.trim().is_empty(), egui::Button::new("Rename"))
                        .clicked()
                    {
                        out.push(Intent::Stamp(StampEdit::Rename {
                            path: saved.path.clone(),
                            name: self.name.trim().to_owned(),
                        }));
                    }
                    if ui.button("Delete Stamp").clicked() {
                        out.push(Intent::Stamp(StampEdit::Delete(saved.path.clone())));
                    }
                });
                let count = saved.stamp.variations.len();
                let noun = if count == 1 {
                    "variation"
                } else {
                    "variations"
                };
                egui::CollapsingHeader::new(format!("{count} {noun}")).show(ui, |ui| {
                    for (index, variation) in saved.stamp.variations.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                self.previews.show(ui, editor, saved, index, workers, out);
                                ui.vertical(|ui| {
                                    if ui.button(format!("Variation {}", index + 1)).clicked() {
                                        out.push(Intent::Stamp(StampEdit::Choose {
                                            path: saved.path.clone(),
                                            variation: Some(index),
                                        }));
                                    }
                                    ui.horizontal(|ui| {
                                        let draft_id = ui
                                            .id()
                                            .with("weight")
                                            .with(editor.stamps().generation());
                                        let mut weight = ui
                                            .data(|data| data.get_temp::<f64>(draft_id))
                                            .unwrap_or(variation.probability);
                                        let label = ui.label("Weight");
                                        let response = ui
                                            .add(
                                                egui::DragValue::new(&mut weight)
                                                    .range(0.0..=1_000_000.0)
                                                    .update_while_editing(false)
                                                    .speed(0.1),
                                            )
                                            .labelled_by(label.id);
                                        if response.dragged() {
                                            ui.data_mut(|data| data.insert_temp(draft_id, weight));
                                        }
                                        if (response.drag_stopped()
                                            || response.changed() && !response.dragged())
                                            && weight != variation.probability
                                        {
                                            ui.data_mut(|data| data.remove::<f64>(draft_id));
                                            out.push(Intent::Stamp(StampEdit::Probability {
                                                path: saved.path.clone(),
                                                variation: index,
                                                probability: weight,
                                            }));
                                        }
                                    });
                                });
                            });
                        });
                    }
                });
            });
        });
    }
}
