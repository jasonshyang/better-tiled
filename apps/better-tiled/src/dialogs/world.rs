//! World settings, explicit placements, filename patterns and custom properties as one draft.

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};
use crate::panels::PropertyDraft;
use bt_core::{World, WorldPattern};
use bt_editor::{AssetId, DocumentId, Editor, Intent};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorldDialog {
    doc: DocumentId,
    world: AssetId,
    value: World,
    properties: PropertyDraft,
}

impl WorldDialog {
    pub(crate) fn new(doc: DocumentId, editor: &Editor) -> Option<Self> {
        let world = editor.world_of(doc)?;
        Some(Self {
            doc,
            world,
            value: (**editor.store().world(world)?).clone(),
            properties: PropertyDraft::default(),
        })
    }
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &Editor,
        out: &mut Vec<Intent>,
    ) -> bool {
        let editing = crate::focus::is_typing(ctx);
        let mut answer = ButtonsAnswer::None;
        let mut valid = true;
        let response = egui::Modal::new(egui::Id::new("world-properties")).show(ctx, |ui| {
            ui.set_min_width(720.0);
            ui.heading("World Properties");
            if let Some(asset) = editor.store().get(self.world) { ui.weak(asset.display_name()); }
            ui.checkbox(&mut self.value.only_show_adjacent_maps, "Only show adjacent maps");
            ui.horizontal(|ui| {
                integer(ui, "Grid Width", &mut self.value.grid_size.x);
                integer(ui, "Grid Height", &mut self.value.grid_size.y);
            });
            ui.small("Set both grid dimensions to 0 to snap to each map's tile size.");
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                ui.collapsing("Maps", |ui| {
                    ui.small("Use Map → Add Map to World to choose another map. Positions and dimensions are pixels.");
                    let mut remove = None;
                    for (index, entry) in self.value.maps.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(&entry.file.source);
                                if ui.button("Remove Map").clicked() { remove = Some(index); }
                            });
                            ui.horizontal(|ui| {
                                integer(ui, "X", &mut entry.rect.x); integer(ui, "Y", &mut entry.rect.y);
                                integer(ui, "Width", &mut entry.rect.w); integer(ui, "Height", &mut entry.rect.h);
                            });
                        });
                    }
                    if let Some(index) = remove { self.value.maps.remove(index); }
                });
                ui.collapsing("Filename Patterns", |ui| {
                    ui.small("The first two regular-expression capture groups provide each map's column and row.");
                    let mut remove = None;
                    for (index, pattern) in self.value.patterns.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                let label = ui.label("Pattern");
                                ui.text_edit_singleline(&mut pattern.regexp).labelled_by(label.id);
                                if ui.button("Remove Pattern").clicked() { remove = Some(index); }
                            });
                            ui.horizontal(|ui| { integer(ui, "Column Pixels", &mut pattern.multiplier.x); integer(ui, "Row Pixels", &mut pattern.multiplier.y); });
                            ui.horizontal(|ui| { integer(ui, "Offset X", &mut pattern.offset.x); integer(ui, "Offset Y", &mut pattern.offset.y); });
                            ui.horizontal(|ui| { integer(ui, "Map Width", &mut pattern.map_size.x); integer(ui, "Map Height", &mut pattern.map_size.y); });
                        });
                    }
                    if let Some(index) = remove { self.value.patterns.remove(index); }
                    if ui.button("Add Pattern").clicked() { self.value.patterns.push(WorldPattern::new(r"map_(\d+)_(\d+)\.tmx")); }
                });
                ui.separator(); ui.strong("Custom Properties");
                self.properties.show(ui, &mut self.value.properties, editor.custom_types());
            });
            if let Some(problem) = bt_editor::world_problem(&self.value) { ui.colored_label(ui.visuals().error_fg_color, problem); valid = false; }
            answer = dialog_buttons(ui, valid);
        });
        let keys = DialogKeys::take(ctx);
        if valid && (answer == ButtonsAnswer::Ok || keys.enter && !editing) {
            out.push(Intent::SetWorld {
                doc: self.doc,
                world: self.world,
                value: Box::new(self.value.clone()),
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape && !editing || response.should_close()
    }
}
fn integer(ui: &mut egui::Ui, label: &str, value: &mut i32) {
    let label = ui.label(label);
    ui.add(egui::DragValue::new(value).speed(1))
        .labelled_by(label.id);
}
