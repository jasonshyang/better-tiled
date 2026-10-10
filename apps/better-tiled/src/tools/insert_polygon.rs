//! Insert Polygon (P): each click adds a point; clicking the first point again closes the
//! shape into a polygon, a right-click or Enter finishes it as a polyline; Escape drops it,
//! and with nothing under way goes back to Select Objects (Tiled's `CreatePolygonObjectTool`).
//! Points snap as the preference says, Command toggling grid snapping and Command with Shift
//! the fine grid.

use bt_core::{MapObject, Shape};
use bt_editor::{ActionId, Intent, ToolKind};
use bt_render::VectorItem;
use egui::PointerButton;
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::Gesture;

/// How near the first point a click closes the polygon, in screen points.
const CLOSE_POINTS: f64 = 6.0;

/// Insert Polygon's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct InsertPolygon {
    /// The points so far, in pixel space.
    points: Vec<DVec2>,
    hover: Option<DVec2>,
    /// The modifiers held, which toggle snapping.
    modifiers: egui::Modifiers,
}

impl InsertPolygon {
    fn finish(&mut self, cx: &mut ToolContext<'_>, closed: bool) {
        let points = std::mem::take(&mut self.points);
        let enough = if closed { 3 } else { 2 };
        let Some(&origin) = points.first() else {
            return;
        };
        if points.len() < enough {
            return;
        }
        let relative: Vec<DVec2> = points.iter().map(|p| *p - origin).collect();
        cx.out.push(Intent::InsertObject {
            doc: cx.doc,
            object: Box::new(MapObject {
                pos: origin,
                shape: if closed {
                    Shape::Polygon(relative)
                } else {
                    Shape::Polyline(relative)
                },
                ..MapObject::default()
            }),
        });
    }
}

impl Tool for InsertPolygon {
    fn kind(&self) -> ToolKind {
        ToolKind::InsertPolygon
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => {
                self.hover = Some(
                    cx.editor
                        .snapped(cx.doc, at.pixel, snap_toggle(self.modifiers)),
                );
            }
            Gesture::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let at = cx
                    .editor
                    .snapped(cx.doc, press.at.pixel, snap_toggle(press.modifiers));
                let closes = self.points.len() >= 3
                    && self.points.first().is_some_and(|first| {
                        cx.pixel_to_world(*first).distance(press.at.world)
                            <= cx.world_length(CLOSE_POINTS)
                    });
                if closes {
                    self.finish(cx, true);
                } else {
                    self.points.push(at);
                }
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                self.finish(cx, false);
            }
            Gesture::Confirm => self.finish(cx, false),
            Gesture::Cancel if self.points.is_empty() => {
                cx.out.push(Intent::Run(ActionId::SelectObjectsTool));
            }
            Gesture::Cancel => self.points.clear(),
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if self.points.is_empty() {
            return;
        }
        let color = cx.editor.settings().theme.selection_color.0;
        let mut line: Vec<DVec2> = self.points.iter().map(|p| cx.pixel_to_world(*p)).collect();
        if let Some(hover) = self.hover {
            line.push(cx.pixel_to_world(hover));
        }
        out.vectors
            .push(VectorItem::polyline(line).stroke(color, 1.5));
    }

    fn hint(&self, _cx: &ToolContext<'_>) -> String {
        if self.points.is_empty() {
            "Click to start a polygon or a polyline".to_owned()
        } else {
            format!(
                "{} · click the first point to close a polygon · right-click or Enter finishes a polyline · Escape drops it",
                bt_utils::text::plural(self.points.len(), "point", "points")
            )
        }
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.points.clear();
        self.hover = None;
    }
}
