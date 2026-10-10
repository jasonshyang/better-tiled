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
    /// A menu inside the menu.
    Submenu(&'static str, &'static [Entry]),
}

/// A menu of the menu bar.
struct Menu {
    title: &'static str,
    entries: &'static [Entry],
}

use Entry::{Action, Separator, Submenu};

/// The menus, in order.
const MENUS: [Menu; 7] = [
    Menu {
        title: "File",
        entries: &[
            Action(ActionId::NewMap),
            Action(ActionId::NewTileset),
            Separator,
            Action(ActionId::Open),
            Entry::RecentFiles,
            Separator,
            Action(ActionId::Save),
            Action(ActionId::SaveAs),
            Action(ActionId::SaveAll),
            Separator,
            Action(ActionId::Close),
            Action(ActionId::Quit),
        ],
    },
    Menu {
        title: "Edit",
        entries: &[
            Action(ActionId::Undo),
            Action(ActionId::Redo),
            Separator,
            Action(ActionId::Cut),
            Action(ActionId::Copy),
            Action(ActionId::Paste),
            Action(ActionId::PasteInPlace),
            Action(ActionId::Delete),
            Separator,
            Action(ActionId::SelectAll),
            Action(ActionId::SelectNone),
            Action(ActionId::InvertSelection),
            Separator,
            Action(ActionId::FlipHorizontal),
            Action(ActionId::FlipVertical),
            Action(ActionId::RotateLeft),
            Action(ActionId::RotateRight),
            Separator,
            Action(ActionId::SearchActions),
            Action(ActionId::Preferences),
        ],
    },
    Menu {
        title: "View",
        entries: &[
            Action(ActionId::ZoomIn),
            Action(ActionId::ZoomOut),
            Action(ActionId::ZoomNormal),
            Action(ActionId::FitInView),
            Separator,
            Action(ActionId::ShowGrid),
            Action(ActionId::HighlightCurrentLayer),
            Submenu(
                "Snapping",
                &[
                    Action(ActionId::SnapNone),
                    Action(ActionId::SnapToGrid),
                    Action(ActionId::SnapToFineGrid),
                    Action(ActionId::SnapToPixels),
                ],
            ),
            Separator,
            Action(ActionId::NextDocument),
            Action(ActionId::PreviousDocument),
        ],
    },
    Menu {
        title: "Map",
        entries: &[
            Action(ActionId::ResizeMap),
            Action(ActionId::OffsetMap),
            Action(ActionId::CropToSelection),
            Action(ActionId::Autocrop),
            Separator,
            Action(ActionId::AddExternalTileset),
            Separator,
            Action(ActionId::MapProperties),
        ],
    },
    Menu {
        title: "Layer",
        entries: &[
            Submenu(
                "New",
                &[
                    Action(ActionId::NewTileLayer),
                    Action(ActionId::NewObjectLayer),
                    Action(ActionId::NewImageLayer),
                    Action(ActionId::NewGroupLayer),
                ],
            ),
            Action(ActionId::LayerViaCopy),
            Action(ActionId::LayerViaCut),
            Separator,
            Action(ActionId::GroupLayers),
            Action(ActionId::UngroupLayers),
            Action(ActionId::DuplicateLayers),
            Action(ActionId::MergeLayerDown),
            Action(ActionId::RemoveLayers),
            Separator,
            Action(ActionId::SelectPreviousLayer),
            Action(ActionId::SelectNextLayer),
            Action(ActionId::SelectAllLayers),
            Action(ActionId::RaiseLayers),
            Action(ActionId::LowerLayers),
            Separator,
            Action(ActionId::ToggleSelectedLayers),
            Action(ActionId::ToggleLockSelectedLayers),
            Action(ActionId::ToggleOtherLayers),
            Action(ActionId::ToggleLockOtherLayers),
            Separator,
            Action(ActionId::LayerProperties),
        ],
    },
    Menu {
        title: "Objects",
        entries: &[
            Action(ActionId::DuplicateObjects),
            Separator,
            Action(ActionId::RaiseObjects),
            Action(ActionId::LowerObjects),
            Action(ActionId::RaiseObjectsToTop),
            Action(ActionId::LowerObjectsToBottom),
            Separator,
            Action(ActionId::MoveObjectsToLayer),
        ],
    },
    Menu {
        title: "Tools",
        entries: &[
            Action(ActionId::BrushTool),
            Action(ActionId::EraserTool),
            Action(ActionId::BucketFillTool),
            Action(ActionId::ShapeFillTool),
            Action(ActionId::PickerTool),
            Action(ActionId::RectSelectTool),
            Action(ActionId::MagicWandTool),
            Action(ActionId::SameTileTool),
            Action(ActionId::OffsetLayersTool),
            Separator,
            Action(ActionId::SelectObjectsTool),
            Action(ActionId::InsertRectangleTool),
            Action(ActionId::InsertEllipseTool),
            Action(ActionId::InsertCapsuleTool),
            Action(ActionId::InsertPointTool),
            Action(ActionId::InsertPolygonTool),
            Action(ActionId::InsertTileTool),
            Action(ActionId::InsertTextTool),
            Action(ActionId::EditPolygonsTool),
            Separator,
            Action(ActionId::RandomMode),
            Action(ActionId::BrushSizeUp),
            Action(ActionId::BrushSizeDown),
        ],
    },
];

/// Draws the menu bar; a chosen command goes to `out`.
pub(crate) fn menu_bar(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in &MENUS {
            ui.menu_button(menu.title, |ui| menu_entries(ui, editor, menu.entries, out));
        }
    });
}

fn menu_entries(ui: &mut egui::Ui, editor: &Editor, entries: &[Entry], out: &mut Vec<Intent>) {
    for entry in entries {
        match entry {
            Entry::Action(id) => action_item(ui, editor, *id, out),
            Entry::Separator => {
                ui.separator();
            }
            Entry::RecentFiles => recent_files(ui, editor, out),
            Entry::Submenu(title, inner) => {
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
        fn collect(entries: &[Entry], listed: &mut Vec<ActionId>) {
            for entry in entries {
                match entry {
                    Entry::Action(id) => listed.push(*id),
                    Entry::Submenu(_, inner) => collect(inner, listed),
                    Entry::Separator | Entry::RecentFiles => {}
                }
            }
        }
        let mut listed = Vec::new();
        for menu in &MENUS {
            collect(menu.entries, &mut listed);
        }
        for spec in ACTIONS {
            let times = listed.iter().filter(|id| **id == spec.id).count();
            assert_eq!(times, 1, "{:?}", spec.id);
        }
    }
}
