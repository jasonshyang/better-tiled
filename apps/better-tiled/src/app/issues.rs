//! One panel for model, files, rules, image decoding and Godot diagnostics.
use super::App;
use bt_editor::{DocumentId, Intent, Issue, Severity};
impl App {
    pub(super) fn all_issues(&self, doc: DocumentId) -> Vec<Issue> {
        let mut issues = self.editor.issues(doc);
        if let Ok(mut images) = self.images.lock() {
            issues.extend(self.editor.image_issues(doc, &images.failures()));
        }
        issues.extend(self.godot_issues(doc));
        bt_editor::deduplicate_issues(issues)
    }
    pub(super) fn show_issues(&mut self, ui: &mut egui::Ui, doc: DocumentId) {
        let issues = self.all_issues(doc);
        ui.horizontal(|ui| {
            ui.label(if issues.len() == 1 {
                "1 issue".into()
            } else {
                format!("{} issues", issues.len())
            });
            if ui.small_button("Refresh").clicked() {
                self.intents.push(Intent::RefreshIssues);
            }
            if self.editor.issues_pending(doc) {
                ui.spinner();
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.issues_filter)
                .hint_text("Filter issues…")
                .desired_width(f32::INFINITY),
        );
        ui.separator();
        let filter = self.issues_filter.to_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            if issues.is_empty() {
                ui.label(if self.editor.issues_pending(doc) {
                    "Checking files and model…"
                } else {
                    "No issues found."
                });
            }
            for issue in issues {
                if !format!("{:?} {:?} {}", issue.source, issue.severity, issue.text)
                    .to_lowercase()
                    .contains(&filter)
                {
                    continue;
                }
                let color = match issue.severity {
                    Severity::Error => ui.visuals().error_fg_color,
                    Severity::Warning => ui.visuals().warn_fg_color,
                    Severity::Info => ui.visuals().text_color(),
                };
                ui.colored_label(color, format!("{:?} · {:?}", issue.source, issue.severity));
                let response = ui.add(egui::Label::new(&issue.text).wrap());
                if let bt_editor::IssueKind::Resource { path, .. } = &issue.kind {
                    response.on_hover_text(path.display().to_string());
                }
                if let Some(target) = issue.fix
                    && ui.small_button("Locate File…").clicked()
                {
                    self.intents.push(Intent::LocateIssue { doc, target });
                }
                for target in issue.targets {
                    if ui
                        .small_button("Go to source")
                        .on_hover_text(format!("{target:?}"))
                        .clicked()
                    {
                        self.intents.push(Intent::GoToIssue(target));
                    }
                }
                ui.separator();
            }
        });
    }
}
