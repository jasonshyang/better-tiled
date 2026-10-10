//! The menu bar: the action table arranged in menus, each item asking the editor whether it is
//! enabled and checked and the keymap what its key is.

use bt_editor::{ActionId, ActionKind, Editor, Enabled, Intent};

/// An entry of a menu.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Entry {
    /// A command.
    Action(ActionId),
    /// A line between groups.
    Separator,
    /// The recently opened files.
    RecentFiles,
}

/// A menu of the menu bar.
struct Menu {
    title: &'static str,
    entries: &'static [Entry],
}

/// The menus, in order.
const MENUS: [Menu; 3] = [
    Menu {
        title: "File",
        entries: &[
            Entry::Action(ActionId::Open),
            Entry::RecentFiles,
            Entry::Separator,
            Entry::Action(ActionId::Save),
            Entry::Action(ActionId::SaveAs),
            Entry::Action(ActionId::SaveAll),
            Entry::Separator,
            Entry::Action(ActionId::Close),
            Entry::Action(ActionId::Quit),
        ],
    },
    Menu {
        title: "Edit",
        entries: &[Entry::Action(ActionId::Undo), Entry::Action(ActionId::Redo)],
    },
    Menu {
        title: "View",
        entries: &[
            Entry::Action(ActionId::ZoomIn),
            Entry::Action(ActionId::ZoomOut),
            Entry::Action(ActionId::ZoomNormal),
            Entry::Action(ActionId::FitInView),
            Entry::Separator,
            Entry::Action(ActionId::ShowGrid),
            Entry::Action(ActionId::HighlightCurrentLayer),
            Entry::Separator,
            Entry::Action(ActionId::NextDocument),
            Entry::Action(ActionId::PreviousDocument),
        ],
    },
];

/// Draws the menu bar; a chosen command goes to `out`.
pub(crate) fn menu_bar(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in &MENUS {
            ui.menu_button(menu.title, |ui| {
                for entry in menu.entries {
                    match entry {
                        Entry::Action(id) => action_item(ui, editor, *id, out),
                        Entry::Separator => {
                            ui.separator();
                        }
                        Entry::RecentFiles => recent_files(ui, editor, out),
                    }
                }
            });
        }
    });
}

fn action_item(ui: &mut egui::Ui, editor: &Editor, id: ActionId, out: &mut Vec<Intent>) {
    let spec = id.spec();
    let label = match (spec.kind, editor.checked(id)) {
        (ActionKind::Toggle, Some(true)) => format!("✔ {}", spec.label),
        (ActionKind::Toggle, _) => format!("    {}", spec.label),
        (ActionKind::Command, _) => spec.label.to_owned(),
    };
    let mut button = egui::Button::new(label);
    if let Some(key) = editor.keymap().key_text(id) {
        button = button.shortcut_text(key);
    }
    let enabled = editor.enabled(id) == Enabled::Enabled;
    if ui.add_enabled(enabled, button).clicked() {
        out.push(Intent::Run(id));
        ui.close();
    }
}

fn recent_files(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    let recent = editor.session().recent_files;
    ui.add_enabled_ui(!recent.is_empty(), |ui| {
        ui.menu_button("Open Recent", |ui| {
            for path in recent {
                let name = bt_utils::paths::display_name(&path);
                let response = ui.button(name).on_hover_text(path.display().to_string());
                if response.clicked() {
                    out.push(Intent::Open(path));
                    ui.close();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use bt_editor::ACTIONS;

    use super::*;

    #[test]
    fn every_command_is_in_a_menu_once() {
        let listed: Vec<ActionId> = MENUS
            .iter()
            .flat_map(|menu| menu.entries.iter())
            .filter_map(|entry| match entry {
                Entry::Action(id) => Some(*id),
                Entry::Separator | Entry::RecentFiles => None,
            })
            .collect();
        for spec in ACTIONS {
            let times = listed.iter().filter(|id| **id == spec.id).count();
            assert_eq!(times, 1, "{:?}", spec.id);
        }
    }
}
