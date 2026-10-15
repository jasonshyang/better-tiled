//! Project Properties applies one draft transaction in the project's own document history.

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};
use crate::panels::PropertyDraft;
use bt_core::{CompatibilityVersion, FileRef, Project};
use bt_editor::{AssetId, DocumentId, Editor, Intent};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectDialog {
    base_version: u64,
    doc: DocumentId,
    project: AssetId,
    value: Project,
    rules: String,
    properties: PropertyDraft,
}

impl ProjectDialog {
    pub(crate) fn new(doc: DocumentId, editor: &Editor) -> Option<Self> {
        let project = editor.project_asset()?;
        let value = (**editor.store().project(project)?).clone();
        Some(Self {
            base_version: editor.store().get(project)?.version(),
            doc,
            project,
            rules: value
                .automapping_rules_file
                .as_ref()
                .map_or_else(String::new, |file| file.source.clone()),
            value,
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
        let current = editor
            .store()
            .get(self.project)
            .is_some_and(|asset| asset.version() == self.base_version);
        let response = egui::Modal::new(egui::Id::new("project-properties")).show(ctx, |ui| {
            ui.set_min_width(620.0);
            ui.heading("Project Properties");
            if let Some(path) = editor.project_path() {
                ui.weak(path.display().to_string());
            }
            ui.horizontal(|ui| {
                ui.label("Compatibility");
                egui::ComboBox::from_id_salt("project-compatibility")
                    .selected_text(label(self.value.compatibility_version))
                    .show_ui(ui, |ui| {
                        for version in [
                            CompatibilityVersion::Latest,
                            CompatibilityVersion::Tiled1_10,
                            CompatibilityVersion::Tiled1_9,
                            CompatibilityVersion::Tiled1_8,
                        ] {
                            ui.selectable_value(
                                &mut self.value.compatibility_version,
                                version,
                                label(version),
                            );
                        }
                    });
            });
            let name = ui.label("AutoMap Rules File");
            ui.text_edit_singleline(&mut self.rules)
                .labelled_by(name.id);
            ui.small(
                "Relative to the project file. Leave empty to discover rules beside each map.",
            );
            ui.separator();
            ui.strong("Custom Properties");
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    self.properties
                        .show(ui, &mut self.value.properties, editor.custom_types())
                });
            if !current { ui.colored_label(ui.visuals().error_fg_color, "The project changed while this draft was open. Cancel and reopen Project Properties."); }
            answer = dialog_buttons(ui, current);
        });
        let keys = DialogKeys::take(ctx);
        if current && (answer == ButtonsAnswer::Ok || keys.enter && !editing) {
            let Some(folder) = editor.project_path().and_then(std::path::Path::parent) else {
                return true;
            };
            self.value.automapping_rules_file = if self.rules.trim().is_empty() {
                None
            } else {
                Some(FileRef::new(
                    &self.rules,
                    bt_utils::paths::clean_path(&folder.join(&self.rules)),
                ))
            };
            out.push(Intent::SetProject {
                doc: self.doc,
                project: self.project,
                value: Box::new(self.value.clone()),
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape && !editing || response.should_close()
    }
}
fn label(version: CompatibilityVersion) -> String {
    match version {
        CompatibilityVersion::Latest => "Latest".into(),
        CompatibilityVersion::Tiled1_10 => "Tiled 1.10".into(),
        CompatibilityVersion::Tiled1_9 => "Tiled 1.9".into(),
        CompatibilityVersion::Tiled1_8 => "Tiled 1.8".into(),
        CompatibilityVersion::Other(number) => format!("Version {number}"),
    }
}
