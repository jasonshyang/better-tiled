//! The Explain tool (A): click a cell and the AutoMap panel says what the rules did there, and
//! why each rule that could write it did or did not.

use bt_editor::{Intent, TileRegion, ToolKind};
use egui::PointerButton;
use glam::IVec2;

use super::painting::{EXPLAIN_COLOR, outline_cells};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The Explain tool's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct ExplainTool {
    hover: Option<IVec2>,
}

impl Tool for ExplainTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Explain
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(at.cell),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                cx.out.push(Intent::ExplainCell {
                    doc: cx.doc,
                    cell: press.at.cell,
                });
            }
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(cell) = cx.editor.explained_cell(cx.doc) {
            outline_cells(cx, &TileRegion::from_cells([cell]), EXPLAIN_COLOR, out);
        }
        if let Some(at) = self.hover {
            let color = cx.editor.settings().theme.selection_color.0;
            outline_cells(cx, &TileRegion::from_cells([at]), color, out);
        }
    }

    fn hint(&self, _cx: &ToolContext<'_>) -> String {
        "Click a cell: the AutoMap panel says what the rules did there, and why".to_owned()
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.hover = None;
    }
}
