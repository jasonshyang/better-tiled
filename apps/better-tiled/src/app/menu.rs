//! The menu bar: the action table arranged in menus (`crate::menus` draws their items).

use bt_editor::{ActionId, Editor, Intent};

use crate::menus::{MenuEntry, NEW_LAYER_MENU, menu_entries};

/// A menu of the menu bar.
struct Menu {
    title: &'static str,
    entries: &'static [MenuEntry],
}

use MenuEntry::{Action, Separator, Submenu};

/// The menus, in order.
const MENUS: [Menu; 7] = [
    Menu {
        title: "File",
        entries: &[
            Action(ActionId::NewMap),
            Action(ActionId::NewTileset),
            Separator,
            Action(ActionId::Open),
            MenuEntry::RecentFiles,
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
            Action(ActionId::ShowPainted),
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
            Action(ActionId::CustomTypesEditor),
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
            Action(ActionId::AutoMap),
            Action(ActionId::LiveAutoMap),
            Separator,
            Action(ActionId::MapProperties),
        ],
    },
    Menu {
        title: "Layer",
        entries: &[
            Submenu("New", NEW_LAYER_MENU),
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
            Action(ActionId::JoinNodes),
            Action(ActionId::SplitSegments),
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
            Action(ActionId::TerrainTool),
            Action(ActionId::EraserTool),
            Action(ActionId::BucketFillTool),
            Action(ActionId::ShapeFillTool),
            Action(ActionId::PickerTool),
            Action(ActionId::RectSelectTool),
            Action(ActionId::MagicWandTool),
            Action(ActionId::SameTileTool),
            Action(ActionId::OffsetLayersTool),
            Action(ActionId::ExplainTool),
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

#[cfg(test)]
mod tests {
    use bt_editor::ACTIONS;

    use super::*;

    #[test]
    fn every_command_is_in_a_menu_once() {
        fn collect(entries: &[MenuEntry], listed: &mut Vec<ActionId>) {
            for entry in entries {
                match entry {
                    MenuEntry::Action(id) => listed.push(*id),
                    MenuEntry::Submenu(_, inner) => collect(inner, listed),
                    MenuEntry::Separator | MenuEntry::RecentFiles => {}
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
