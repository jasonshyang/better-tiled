//! A project-command draft: Cancel discards it and OK records one project transaction.

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};
use bt_core::Command;
use bt_editor::{Chord, DocumentId, Editor, Intent};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CommandsDialog {
    doc: DocumentId,
    commands: Vec<Command>,
    selected: usize,
}

impl CommandsDialog {
    pub(crate) fn new(doc: DocumentId, editor: &Editor) -> Self {
        Self {
            doc,
            commands: editor.project_commands().to_vec(),
            selected: 0,
        }
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, out: &mut Vec<Intent>) -> bool {
        let editing = crate::focus::is_typing(ctx);
        let mut answer = ButtonsAnswer::None;
        let mut valid = true;
        let response = egui::Modal::new(egui::Id::new("edit-commands")).show(ctx, |ui| {
            ui.set_min_width(600.0);
            ui.heading("Edit Commands");
            ui.horizontal(|ui| {
                if ui.button("Add Command").clicked() {
                    self.commands.push(Command { name: format!("Command {}", self.commands.len() + 1), ..Command::default() });
                    self.selected = self.commands.len() - 1;
                }
                if ui.add_enabled(self.selected < self.commands.len(), egui::Button::new("Remove Command")).clicked() {
                    self.commands.remove(self.selected);
                    self.selected = self.selected.saturating_sub(1);
                }
                if ui.add_enabled(self.selected > 0 && self.selected < self.commands.len(), egui::Button::new("Move Up")).clicked() {
                    self.commands.swap(self.selected, self.selected - 1); self.selected -= 1;
                }
                if ui.add_enabled(self.selected + 1 < self.commands.len(), egui::Button::new("Move Down")).clicked() {
                    self.commands.swap(self.selected, self.selected + 1); self.selected += 1;
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (index, command) in self.commands.iter().enumerate() { ui.selectable_value(&mut self.selected, index, &command.name); }
            });
            ui.separator();
            if let Some(command) = self.commands.get_mut(self.selected) {
                egui::Grid::new("command-fields").num_columns(2).show(ui, |ui| {
                    field(ui, "Name", &mut command.name);
                    field(ui, "Executable", &mut command.command);
                    field(ui, "Arguments", &mut command.arguments);
                    field(ui, "Working Directory", &mut command.working_directory);
                    field(ui, "Shortcut", &mut command.shortcut);
                });
                ui.checkbox(&mut command.enabled, "Enabled");
                ui.checkbox(&mut command.save_before_execute, "Save before executing");
                ui.checkbox(&mut command.show_output, "Show output");
            }
            ui.small("Use double quotes around arguments containing spaces. Placeholders such as %mapfile, %mappath and %projectpath stay single argument values.");
            ui.small("Shortcuts use Tiled notation, for example Ctrl+Shift+R.");
            for command in &self.commands {
                if command.name.trim().is_empty() || command.command.trim().is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, "Each command needs a name and an executable."); valid = false; break;
                }
                if !command.shortcut.is_empty() && Chord::from_qt(&command.shortcut).is_none() {
                    ui.colored_label(ui.visuals().error_fg_color, "A shortcut must be one supported Tiled key chord."); valid = false; break;
                }
            }
            answer = dialog_buttons(ui, valid);
        });
        let keys = DialogKeys::take(ctx);
        if valid && (answer == ButtonsAnswer::Ok || keys.enter && !editing) {
            out.push(Intent::SetProjectCommands {
                doc: self.doc,
                commands: self.commands.clone(),
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape && !editing || response.should_close()
    }
}

fn field(ui: &mut egui::Ui, name: &str, value: &mut String) {
    let label = ui.label(name);
    ui.add(egui::TextEdit::singleline(value).desired_width(460.0))
        .labelled_by(label.id);
    ui.end_row();
}
