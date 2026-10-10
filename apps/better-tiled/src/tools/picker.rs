//! The Picker (I): click a cell and the topmost tile shown there becomes the brush; the Brush
//! comes back.

use bt_editor::{Intent, TileRegion, ToolKind};
use egui::PointerButton;
use glam::IVec2;

use super::painting::outline_cells;
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The Picker's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Picker {
    hover: Option<IVec2>,
}

impl Tool for Picker {
    fn kind(&self) -> ToolKind {
        ToolKind::Picker
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(at.cell),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                cx.out.push(Intent::PickFromMap {
                    doc: cx.doc,
                    at: press.at.cell,
                });
            }
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(at) = self.hover {
            let color = cx.editor.settings().theme.selection_color.0;
            outline_cells(cx, &TileRegion::from_cells([at]), color, out);
        }
    }

    fn hint(&self, _cx: &ToolContext<'_>) -> String {
        "Click a tile to paint with it".to_owned()
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.hover = None;
    }
}
