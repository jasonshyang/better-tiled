//! Offset Layers (M): dragging moves the offsets of the selected unlocked layers live, as one
//! step, snapping as the preference says with Command toggling grid snapping and Command with
//! Shift the fine grid (Tiled's `LayerOffsetTool`, `reference/tiled/src/tiled/layeroffsettool.cpp`,
//! whose `SnapHelper` treats an offset as a position). Escape puts the layers back; the button
//! must be pressed again to start another drag.

use bt_editor::{Intent, ObjectStroke, ObjectStrokeKind, Snapping, ToolKind};
use egui::PointerButton;

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::{Gesture, Pointer};

/// Offset Layers' state.
#[derive(Clone, Debug, Default)]
pub(crate) struct OffsetLayers {
    /// Where the primary button went down, while it is held.
    press: Option<Pointer>,
    /// Whether the press turned into a drag of the layers.
    dragging: bool,
}

impl OffsetLayers {
    /// Ends a drag under way with `step`, and forgets the press.
    fn stop(&mut self, step: ObjectStroke, out: &mut Vec<Intent>) {
        self.press = None;
        if std::mem::take(&mut self.dragging) {
            out.push(Intent::ObjectStroke(step));
        }
    }
}

impl Tool for OffsetLayers {
    fn kind(&self) -> ToolKind {
        ToolKind::OffsetLayers
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.press = Some(press.at);
                self.dragging = false;
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                let Some(start) = self.press else {
                    return;
                };
                if !self.dragging {
                    if !drag.moved {
                        return;
                    }
                    self.dragging = true;
                    cx.out.push(Intent::ObjectStroke(ObjectStroke::Begin {
                        doc: cx.doc,
                        kind: ObjectStrokeKind::OffsetLayers,
                    }));
                }
                cx.out.push(Intent::ObjectStroke(ObjectStroke::Move {
                    delta: drag.at.world - start.world,
                    snap: snap_toggle(drag.modifiers),
                }));
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                self.stop(ObjectStroke::End, cx.out);
            }
            Gesture::Cancel => self.stop(ObjectStroke::Cancel, cx.out),
            _ => {}
        }
    }

    fn overlay(&self, _cx: &ToolContext<'_>, _out: &mut Overlay) {}

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let selected = cx.selection().map(|s| s.layers.clone()).unwrap_or_default();
        let locked = selected
            .iter()
            .filter(|id| cx.map.effective_layer(**id).is_some_and(|l| l.locked))
            .count();
        let snaps = matches!(
            cx.editor.settings().behaviour.snapping,
            Snapping::Grid | Snapping::FineGrid
        );
        let toggle = if snaps {
            "to move freely"
        } else {
            "to snap to the grid"
        };
        let mut hint = format!(
            "Drag to move the selected layers ({} {toggle}) · Esc puts them back · {}",
            cx.command_key(),
            bt_utils::text::plural(selected.len(), "layer selected", "layers selected")
        );
        if locked > 0 {
            hint.push_str(&format!(", {locked} locked"));
        }
        hint
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        self.stop(ObjectStroke::End, out);
    }
}
