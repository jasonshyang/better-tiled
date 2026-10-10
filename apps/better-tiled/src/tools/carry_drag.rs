//! Dragging the selected tiles with a tool that selects tiles: a press inside the selection
//! with no modifier held carries the tiles once it drags, keeping the pressed cell under the
//! pointer, as one step; released without dragging, it was a click, which the tool takes as
//! its own.

use bt_editor::{Intent, Stroke, StrokeKind};
use egui::PointerButton;
use glam::IVec2;

use super::tool::ToolContext;
use crate::canvas::{PointerDrag, PointerPress};

/// A press inside the selection that moves its tiles once it drags.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CarryDrag {
    /// The cell pressed.
    start: IVec2,
    /// How far the tiles were last moved; `None` until the press drags them.
    moved: Option<IVec2>,
}

impl CarryDrag {
    /// The drag a press may start: the primary button, no modifier, inside the selected cells
    /// of a tile layer.
    pub(crate) fn at_press(press: &PointerPress, cx: &ToolContext<'_>) -> Option<CarryDrag> {
        let plain = !(press.modifiers.shift || press.modifiers.command || press.modifiers.alt);
        let inside = cx
            .selection()
            .is_some_and(|s| s.tiles.contains(press.at.cell));
        (press.button == PointerButton::Primary && plain && inside && cx.tile_layer().is_some())
            .then_some(CarryDrag {
                start: press.at.cell,
                moved: None,
            })
    }

    /// Whether it has moved tiles.
    pub(crate) fn is_moving(&self) -> bool {
        self.moved.is_some()
    }

    /// Follows the pointer: once the press drags to another cell, the tiles move so the cell
    /// pressed stays under the pointer.
    pub(crate) fn follow(&mut self, drag: &PointerDrag, cx: &mut ToolContext<'_>) {
        let delta = drag.at.cell - self.start;
        if !drag.moved || (self.moved.is_none() && delta == IVec2::ZERO) {
            return;
        }
        if self.moved.is_none() {
            cx.out.push(Intent::Stroke(Stroke::Begin {
                doc: cx.doc,
                kind: StrokeKind::Move,
            }));
        }
        if self.moved != Some(delta) {
            cx.out.push(Intent::Stroke(Stroke::Move { delta }));
            self.moved = Some(delta);
        }
    }

    /// Ends the drag as one step; `false` when it never moved tiles (a click, the tool's).
    pub(crate) fn finish(self, out: &mut Vec<Intent>) -> bool {
        if self.is_moving() {
            out.push(Intent::Stroke(Stroke::End));
        }
        self.is_moving()
    }

    /// Drops the drag: the tiles go back to where it found them.
    pub(crate) fn cancel(self, out: &mut Vec<Intent>) {
        if self.is_moving() {
            out.push(Intent::Stroke(Stroke::Cancel));
        }
    }
}
