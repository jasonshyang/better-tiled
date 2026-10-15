//! The project's naturally sorted folder tree; scanning and mutations belong to the editor.

use bt_editor::{ActionId, Editor, Intent, ProjectEntry};

#[derive(Default)]
pub(crate) struct ProjectPanel {
    filter: String,
}

impl ProjectPanel {
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
        let Some(project) = editor.project_asset() else {
            ui.weak("Open or create a project to browse its folders.");
            return;
        };
        let dirty = editor
            .store()
            .get(project)
            .is_some_and(|asset| asset.is_dirty());
        ui.strong(format!(
            "{}{}",
            editor
                .project_path()
                .map_or_else(|| "Untitled Project".into(), bt_utils::paths::display_name),
            if dirty { " *" } else { "" }
        ));
        ui.horizontal_wrapped(|ui| {
            for action in [
                ActionId::AddProjectFolder,
                ActionId::RefreshProject,
                ActionId::ProjectProperties,
            ] {
                if ui.small_button(action.spec().label).clicked() {
                    out.push(Intent::Run(action));
                }
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter project files"));
        for problem in editor.project_scan_problems() {
            ui.colored_label(ui.visuals().warn_fg_color, problem.to_string());
        }
        if editor.project_entries().is_empty() {
            if editor
                .store()
                .project(project)
                .is_some_and(|project| project.folders.is_empty())
            {
                ui.weak("Add a folder to browse project files.");
            } else {
                ui.label("Scanning project…");
            }
        }
        let query = self.filter.to_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for entry in editor.project_entries() {
                if !matches(entry, &query) {
                    continue;
                }
                ui.push_id(&entry.path, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("Remove Folder").clicked() {
                            out.push(Intent::RemoveProjectFolder {
                                project,
                                path: entry.path.clone(),
                            });
                        }
                        let path = entry.path.display().to_string();
                        ui.add(egui::Label::new(&path).truncate())
                            .on_hover_text(path);
                    });
                    show_entry(ui, entry, &query, true, out);
                });
            }
        });
    }
}
fn matches(entry: &ProjectEntry, query: &str) -> bool {
    query.is_empty()
        || entry.path.to_string_lossy().to_lowercase().contains(query)
        || entry.children.iter().any(|child| matches(child, query))
}
fn show_entry(
    ui: &mut egui::Ui,
    entry: &ProjectEntry,
    query: &str,
    root: bool,
    out: &mut Vec<Intent>,
) {
    let name = bt_utils::paths::display_name(&entry.path);
    if entry.directory {
        egui::CollapsingHeader::new(name)
            .id_salt(&entry.path)
            .default_open(root || !query.is_empty())
            .show(ui, |ui| {
                for child in &entry.children {
                    if matches(child, query) {
                        show_entry(ui, child, query, false, out);
                    }
                }
            })
            .header_response
            .on_hover_text(entry.path.display().to_string());
    } else {
        let supported = entry.openable();
        if ui
            .add_enabled(supported, egui::Button::new(name).frame(false).truncate())
            .on_hover_text(entry.path.display().to_string())
            .clicked()
        {
            out.push(Intent::Open(entry.path.clone()));
        }
    }
}
