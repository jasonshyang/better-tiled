//! A template ghost rendered from the same prepared map used by placement.

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::Gesture;
use bt_editor::{ActionId, Intent, ToolKind};
use egui::{Modifiers, PointerButton};
use glam::DVec2;

#[derive(Default)]
pub(crate) struct InsertTemplate {
    pointer: Option<DVec2>,
    modifiers: Modifiers,
    placing: bool,
}

impl Tool for InsertTemplate {
    fn kind(&self) -> ToolKind {
        ToolKind::InsertTemplate
    }
    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.pointer = Some(at.world),
            Gesture::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.pointer = Some(press.at.world);
                self.modifiers = press.modifiers;
                self.placing = true;
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                self.pointer = Some(drag.at.world);
                self.modifiers = drag.modifiers;
            }
            Gesture::Release(release)
                if release.button == PointerButton::Primary && self.placing =>
            {
                self.placing = false;
                cx.out.push(Intent::PlaceTemplate {
                    doc: cx.doc,
                    at: release.at.world,
                    snap: snap_toggle(release.modifiers),
                });
            }
            Gesture::Cancel if self.placing => self.placing = false,
            Gesture::Cancel => cx.out.push(Intent::Run(ActionId::SelectObjectsTool)),
            Gesture::Leave if !self.placing => self.pointer = None,
            _ => {}
        }
    }
    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(at) = self.pointer {
            out.map_preview = cx
                .editor
                .template_preview(cx.doc, at, snap_toggle(self.modifiers));
        }
    }
    fn hint(&self, cx: &ToolContext<'_>) -> String {
        format!(
            "Click to place a template instance · {} toggles snapping · Choose Template selects another base · Esc cancels",
            cx.command_key()
        )
    }
    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.pointer = None;
        self.placing = false;
    }
}
