//! Menus of commands, as the menu bar and the panels' context menus draw them: each entry is a
//! command of the action table, a line, the recent files or a menu inside the menu. A command's
//! item asks the editor whether it is enabled (saying why not on hover, when there is a reason)
//! and checked, and the keymap what its key is.

use bt_editor::{ActionId, ActionKind, Editor, Enabled, Intent};

/// An entry of a menu.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuEntry {
    /// A command.
    Action(ActionId),
    /// A line between groups.
    Separator,
    /// The recently opened files.
    RecentFiles,
    /// A menu inside the menu.
    Submenu(&'static str, &'static [MenuEntry]),
}

/// The Layer menu's New submenu, which the Layers panel offers too.
pub(crate) const NEW_LAYER_MENU: &[MenuEntry] = &[
    MenuEntry::Action(ActionId::NewTileLayer),
    MenuEntry::Action(ActionId::NewObjectLayer),
    MenuEntry::Action(ActionId::NewImageLayer),
    MenuEntry::Action(ActionId::NewGroupLayer),
];

/// Draws the entries of a menu; a chosen command goes to `out`, and the menu closes.
pub(crate) fn menu_entries(
    ui: &mut egui::Ui,
    editor: &Editor,
    entries: &[MenuEntry],
    out: &mut Vec<Intent>,
) {
    for entry in entries {
        match entry {
            MenuEntry::Action(id) => action_item(ui, editor, *id, out),
            MenuEntry::Separator => {
                ui.separator();
            }
            MenuEntry::RecentFiles => recent_files(ui, editor, out),
            MenuEntry::Submenu(title, inner) => {
                ui.menu_button(*title, |ui| menu_entries(ui, editor, inner, out));
            }
        }
    }
}

fn action_item(ui: &mut egui::Ui, editor: &Editor, id: ActionId, out: &mut Vec<Intent>) {
    let spec = id.spec();
    let label = match (spec.kind, editor.checked(id)) {
        (ActionKind::Toggle | ActionKind::Tool(_), Some(true)) => format!("✔ {}", spec.label),
        (ActionKind::Toggle | ActionKind::Tool(_), _) => format!("    {}", spec.label),
        (ActionKind::Command, _) => spec.label.to_owned(),
    };
    let mut button = egui::Button::new(label);
    if let Some(key) = editor.keymap().key_text(id) {
        button = button.shortcut_text(key);
    }
    let response = match editor.enabled(id) {
        Enabled::Enabled => ui.add(button),
        Enabled::Disabled(Some(reason)) => {
            ui.add_enabled(false, button).on_disabled_hover_text(reason)
        }
        Enabled::Disabled(None) => ui.add_enabled(false, button),
    };
    if response.clicked() {
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
