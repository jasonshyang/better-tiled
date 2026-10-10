//! The Brush (B): click or drag to paint the brush, each drag one step; Shift paints a line from
//! where the last stroke ended; a right-drag captures the tiles of the selected layers as the
//! brush, and Shift at its release cuts them. X and Y mirror the brush, Z and Shift+Z turn it,
//! D paints random tiles of it, [ and ] change its size (commands, so their keys are the
//! keymap's).

use bt_editor::{Intent, Stroke, StrokeKind, TileRegion, ToolKind};
use bt_utils::IRect;
use egui::PointerButton;
use glam::IVec2;

use super::painting::{cells_after, preview_cells};
use super::selecting::region_preview;
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// The Brush's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct BrushTool {
    hover: Option<IVec2>,
    /// The cell painted last in the stroke under way.
    painting: Option<IVec2>,
    /// A capture under way: where it started and where the pointer is.
    capture: Option<CaptureDrag>,
    /// Shift is held, so the preview is the line from the last stroke.
    shift: bool,
}

#[derive(Copy, Clone, Debug)]
struct CaptureDrag {
    start: IVec2,
    end: IVec2,
}

impl Tool for BrushTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Brush
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(at.cell),
            Gesture::ModifiersChanged(modifiers) => self.shift = modifiers.shift,
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let cell = press.at.cell;
                let line_from = cx
                    .editor
                    .tools()
                    .last_paint()
                    .filter(|_| press.modifiers.shift);
                cx.out.push(Intent::Stroke(Stroke::Begin {
                    doc: cx.doc,
                    kind: StrokeKind::Paint,
                }));
                match line_from {
                    Some(from) => {
                        cx.out.push(Intent::Stroke(Stroke::Paint {
                            points: bt_utils::raster::points_on_line(
                                from,
                                cell,
                                bt_utils::raster::Connectivity::Eight,
                            ),
                            line: true,
                        }));
                        cx.out.push(Intent::Stroke(Stroke::End));
                    }
                    None => {
                        cx.out.push(Intent::Stroke(Stroke::Paint {
                            points: vec![cell],
                            line: false,
                        }));
                        self.painting = Some(cell);
                    }
                }
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                self.hover = Some(drag.at.cell);
                if let Some(last) = self.painting
                    && last != drag.at.cell
                {
                    cx.out.push(Intent::Stroke(Stroke::Paint {
                        points: cells_after(last, drag.at.cell),
                        line: false,
                    }));
                    self.painting = Some(drag.at.cell);
                }
            }
            Gesture::Release(release)
                if release.button == PointerButton::Primary && self.painting.is_some() =>
            {
                self.painting = None;
                cx.out.push(Intent::Stroke(Stroke::End));
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                self.capture = Some(CaptureDrag {
                    start: press.at.cell,
                    end: press.at.cell,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Secondary => {
                if let Some(capture) = &mut self.capture {
                    capture.end = drag.at.cell;
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Secondary => {
                if let Some(capture) = self.capture.take() {
                    cx.out.push(Intent::Capture {
                        doc: cx.doc,
                        rect: IRect::from_corners(capture.start, capture.end),
                        cut: release.modifiers.shift,
                    });
                }
            }
            Gesture::Cancel => {
                if self.painting.take().is_some() {
                    cx.out.push(Intent::Stroke(Stroke::Cancel));
                }
                self.capture = None;
            }
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        if let Some(capture) = self.capture {
            let rect = IRect::from_corners(capture.start, capture.end);
            let color = cx.editor.settings().theme.selection_color.0;
            out.vectors.extend(region_preview(
                &cx.map.geometry(),
                &TileRegion::from_rect(rect),
                color,
                1.0,
            ));
            return;
        }
        let Some(hover) = self.hover else {
            return;
        };
        let line_from = cx
            .editor
            .tools()
            .last_paint()
            .filter(|_| self.shift && self.painting.is_none());
        let cells = match line_from {
            Some(from) => {
                let points = bt_utils::raster::points_on_line(
                    from,
                    hover,
                    bt_utils::raster::Connectivity::Eight,
                );
                cx.editor.brush_preview(cx.doc, &points, true)
            }
            None => cx.editor.brush_preview(cx.doc, &[hover], false),
        };
        preview_cells(cx, &cells, out);
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        if self.capture.is_some() {
            return "Release to take these tiles as the brush · Shift at release cuts them"
                .to_owned();
        }
        if cx.editor.tools().brush().is_none() {
            return "Pick tiles in the Tiles panel, or right-drag over the map, to paint with them"
                .to_owned();
        }
        let keys = cx.editor.keymap();
        let key = |id| keys.key_text(id).unwrap_or_default();
        format!(
            "Click or drag to paint · Shift: a line from the last stroke · right-drag captures · {} {} mirror, {} turns, {} random",
            key(bt_editor::ActionId::FlipHorizontal),
            key(bt_editor::ActionId::FlipVertical),
            key(bt_editor::ActionId::RotateRight),
            key(bt_editor::ActionId::RandomMode),
        )
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if self.painting.take().is_some() {
            out.push(Intent::Stroke(Stroke::End));
        }
        self.capture = None;
        self.hover = None;
    }
}
