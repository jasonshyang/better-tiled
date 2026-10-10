//! The Eraser (E): click or drag to erase the selected layers, a square of the brush size around
//! the pointer, each drag one step; with Shift every shown, unlocked tile layer. Hidden layers
//! are never erased.

use bt_editor::{Intent, Stroke, StrokeKind, TileRegion, ToolKind};
use bt_utils::IRect;
use egui::PointerButton;
use glam::IVec2;

use super::painting::{ERASE_COLOR, cells_after, outline_cells};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The Eraser's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Eraser {
    hover: Option<IVec2>,
    /// The cell erased last in the stroke under way, and whether it erases every layer.
    erasing: Option<Erasing>,
}

#[derive(Copy, Clone, Debug)]
struct Erasing {
    last: IVec2,
    all_layers: bool,
}

impl Tool for Eraser {
    fn kind(&self) -> ToolKind {
        ToolKind::Eraser
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(at.cell),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let all_layers = press.modifiers.shift;
                cx.out.push(Intent::Stroke(Stroke::Begin {
                    doc: cx.doc,
                    kind: StrokeKind::Erase,
                }));
                cx.out.push(Intent::Stroke(Stroke::Erase {
                    points: vec![press.at.cell],
                    all_layers,
                }));
                self.erasing = Some(Erasing {
                    last: press.at.cell,
                    all_layers,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                self.hover = Some(drag.at.cell);
                if let Some(erasing) = &mut self.erasing
                    && erasing.last != drag.at.cell
                {
                    cx.out.push(Intent::Stroke(Stroke::Erase {
                        points: cells_after(erasing.last, drag.at.cell),
                        all_layers: erasing.all_layers,
                    }));
                    erasing.last = drag.at.cell;
                }
            }
            Gesture::Release(release)
                if release.button == PointerButton::Primary && self.erasing.is_some() =>
            {
                self.erasing = None;
                cx.out.push(Intent::Stroke(Stroke::End));
            }
            Gesture::Cancel if self.erasing.is_some() => {
                self.erasing = None;
                cx.out.push(Intent::Stroke(Stroke::Cancel));
            }
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let Some(hover) = self.hover else {
            return;
        };
        let side = i32::try_from(cx.editor.tools().brush_size()).unwrap_or(1);
        let square = IRect::from_min_size(hover - IVec2::splat((side - 1) / 2), IVec2::splat(side));
        outline_cells(cx, &TileRegion::from_rect(square), ERASE_COLOR, out);
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let keys = cx.editor.keymap();
        let key = |id| keys.key_text(id).unwrap_or_default();
        format!(
            "Click or drag to erase the selected layers · Shift: every shown layer · {} {} size",
            key(bt_editor::ActionId::BrushSizeDown),
            key(bt_editor::ActionId::BrushSizeUp),
        )
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if self.erasing.take().is_some() {
            out.push(Intent::Stroke(Stroke::End));
        }
        self.hover = None;
    }
}
