//! Rectangular Select (R): drag a rectangle of cells; a click without dragging clears the
//! selection, as in Tiled. The modifiers at the press choose how it combines with the
//! selection (Shift adds, Command subtracts, both intersect); while dragging, pressing Shift
//! makes it square and pressing Command grows it from the start in every direction. Dragging
//! inside the selection moves its tiles (`carry_drag`).

use bt_editor::{Intent, SelectionMode, TileRegion, ToolKind, rect_select_rect};
use egui::{Modifiers, PointerButton};
use glam::IVec2;

use super::carry_drag::CarryDrag;
use super::selecting::{carry_hint, let_go, mode_at_press, region_preview, selection_hint};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The tool's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct RectSelect {
    drag: Option<RectDrag>,
    /// A press inside the selection, which moves its tiles if it drags.
    carry: Option<CarryDrag>,
}

/// A rectangle being dragged.
#[derive(Copy, Clone, Debug)]
struct RectDrag {
    start: IVec2,
    end: IVec2,
    mode: SelectionMode,
    square: bool,
    from_center: bool,
    /// The press moved far enough to be a drag.
    selecting: bool,
    /// The modifiers last seen, so only changes during the drag count.
    modifiers: Modifiers,
}

impl RectDrag {
    fn region(&self) -> TileRegion {
        TileRegion::from_rect(rect_select_rect(
            self.start,
            self.end,
            self.square,
            self.from_center,
        ))
    }
}

impl Tool for RectSelect {
    fn kind(&self) -> ToolKind {
        ToolKind::RectSelect
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let default = cx.editor.tools().selection_mode();
                self.carry = CarryDrag::at_press(press, cx);
                self.drag = Some(RectDrag {
                    start: press.at.cell,
                    end: press.at.cell,
                    mode: mode_at_press(press.modifiers, default),
                    square: false,
                    from_center: false,
                    selecting: false,
                    modifiers: press.modifiers,
                });
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                match self.drag.take() {
                    Some(drag) if drag.selecting => {}
                    _ => cx.out.push(Intent::SelectTiles {
                        doc: cx.doc,
                        region: TileRegion::new(),
                        mode: SelectionMode::Replace,
                    }),
                }
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                if let Some(carry) = &mut self.carry {
                    carry.follow(drag, cx);
                } else if let Some(state) = &mut self.drag {
                    state.end = drag.at.cell;
                    state.selecting |= drag.moved;
                }
            }
            Gesture::ModifiersChanged(modifiers) => {
                if let Some(state) = &mut self.drag {
                    if modifiers.shift != state.modifiers.shift {
                        state.square = modifiers.shift;
                    }
                    if modifiers.command != state.modifiers.command {
                        state.from_center = modifiers.command;
                    }
                    state.modifiers = *modifiers;
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(state) = self.drag.take() else {
                    return;
                };
                if self.carry.take().is_some_and(|carry| carry.finish(cx.out)) {
                    return;
                }
                let (region, mode) = if state.selecting {
                    (state.region(), state.mode)
                } else {
                    (TileRegion::new(), SelectionMode::Replace)
                };
                cx.out.push(Intent::SelectTiles {
                    doc: cx.doc,
                    region,
                    mode,
                });
            }
            Gesture::Cancel => {
                let pressed = self.drag.take().is_some();
                match self.carry.take() {
                    Some(carry) => carry.cancel(cx.out),
                    None if !pressed => let_go(cx),
                    None => {}
                }
            }
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(drag) = self.drag.filter(|d| d.selecting) {
            let color = cx.editor.settings().theme.selection_color.0;
            out.vectors
                .extend(region_preview(cx, &drag.region(), color, 1.0));
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        if let Some(hint) = carry_hint(self.carry, cx) {
            return hint;
        }
        match self.drag.filter(|d| d.selecting) {
            Some(drag) => {
                let size = rect_select_rect(drag.start, drag.end, drag.square, drag.from_center);
                format!(
                    "{} × {} · Shift: square · {}: from the start outwards",
                    size.w,
                    size.h,
                    cx.command_key()
                )
            }
            None => selection_hint("Drag to select a rectangle of tiles", cx.command_key()),
        }
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        self.drag = None;
        if let Some(carry) = self.carry.take() {
            carry.finish(out);
        }
    }
}
