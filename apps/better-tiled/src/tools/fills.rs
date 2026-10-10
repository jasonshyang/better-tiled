//! Bucket Fill (F) and Shape Fill (P). Bucket Fill previews the area like the tile under the
//! pointer and what the brush paints there; dragging adds the kinds of tile passed over, the
//! release fills, Shift fills the selection, right-click or Escape cancels. Shape Fill drags a
//! rectangle or an ellipse (the toolbar's choice): Shift makes it square, Alt grows it from the
//! start.

use bt_core::Cell;
use bt_editor::{FillPreview, Intent, TileRegion, ToolKind, shape_fill_region};
use egui::PointerButton;
use glam::IVec2;

use super::painting::{outline_cells, preview_cells};
use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// Bucket Fill's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct BucketFill {
    hover: Option<IVec2>,
    /// The kinds of tile dragged over since the press, when a press is under way.
    kinds: Option<Vec<Cell>>,
    shift: bool,
    preview: FillPreview,
}

impl BucketFill {
    fn update(&mut self, cx: &ToolContext<'_>) {
        self.preview = match self.hover {
            Some(at) => cx.editor.fill_preview(
                cx.doc,
                at,
                self.kinds.as_deref().unwrap_or_default(),
                self.shift,
            ),
            None => FillPreview::default(),
        };
    }
}

impl Tool for BucketFill {
    fn kind(&self) -> ToolKind {
        ToolKind::BucketFill
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) if self.hover != Some(at.cell) => {
                self.hover = Some(at.cell);
                self.update(cx);
            }
            Gesture::ModifiersChanged(modifiers) => {
                self.shift = modifiers.shift;
                self.update(cx);
            }
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.kinds = Some(Vec::new());
                self.hover = Some(press.at.cell);
                self.update(cx);
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                if let (Some(previous), Some(kinds), Some(layer)) =
                    (self.hover, &mut self.kinds, cx.tile_layer())
                    && previous != drag.at.cell
                {
                    let kind = layer.cell(previous);
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                }
                self.hover = Some(drag.at.cell);
                self.update(cx);
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(kinds) = self.kinds.take() else {
                    return;
                };
                cx.out.push(Intent::Fill {
                    doc: cx.doc,
                    start: release.at.cell,
                    extra: kinds,
                    whole_selection: self.shift,
                });
                self.preview = FillPreview::default();
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                self.kinds = None;
                self.update(cx);
            }
            Gesture::Cancel => {
                self.kinds = None;
                self.update(cx);
            }
            Gesture::Leave => {
                self.hover = None;
                self.preview = FillPreview::default();
            }
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        preview_cells(cx, &self.preview.cells, out);
        let color = cx.editor.settings().theme.selection_color.0;
        outline_cells(cx, &self.preview.area, color, out);
    }

    fn hint(&self, _cx: &ToolContext<'_>) -> String {
        format!(
            "Click to fill {} · drag to add the kinds of tile you pass over · Shift fills the selection",
            bt_utils::text::plural(self.preview.area.len(), "cell", "cells")
        )
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        *self = BucketFill::default();
    }
}

/// Shape Fill's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct ShapeFill {
    hover: Option<IVec2>,
    drag: Option<ShapeDrag>,
}

#[derive(Copy, Clone, Debug)]
struct ShapeDrag {
    start: IVec2,
    end: IVec2,
    square: bool,
    from_center: bool,
}

impl ShapeFill {
    fn region(cx: &ToolContext<'_>, drag: ShapeDrag) -> TileRegion {
        shape_fill_region(
            cx.editor.tools().fill_shape(),
            drag.start,
            drag.end,
            drag.square,
            drag.from_center,
        )
    }
}

impl Tool for ShapeFill {
    fn kind(&self) -> ToolKind {
        ToolKind::ShapeFill
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(at.cell),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.drag = Some(ShapeDrag {
                    start: press.at.cell,
                    end: press.at.cell,
                    square: press.modifiers.shift,
                    from_center: press.modifiers.alt,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                if let Some(shape) = &mut self.drag {
                    shape.end = drag.at.cell;
                    shape.square = drag.modifiers.shift;
                    shape.from_center = drag.modifiers.alt;
                }
            }
            Gesture::ModifiersChanged(modifiers) => {
                if let Some(shape) = &mut self.drag {
                    shape.square = modifiers.shift;
                    shape.from_center = modifiers.alt;
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                if let Some(shape) = self.drag.take() {
                    let region = ShapeFill::region(cx, shape);
                    cx.out.push(Intent::ShapeFill {
                        doc: cx.doc,
                        region,
                    });
                }
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => self.drag = None,
            Gesture::Cancel => self.drag = None,
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let region = match self.drag {
            Some(shape) => ShapeFill::region(cx, shape),
            None => match self.hover {
                Some(at) => TileRegion::from_cells([at]),
                None => return,
            },
        };
        preview_cells(cx, &cx.editor.shape_preview(cx.doc, &region), out);
        let color = cx.editor.settings().theme.selection_color.0;
        outline_cells(cx, &region, color, out);
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        match self.drag {
            Some(shape) => {
                let size = ShapeFill::region(cx, shape)
                    .bounds()
                    .map_or(IVec2::ZERO, |b| b.size());
                format!(
                    "{} × {} · Shift: square · Alt: from the start outwards",
                    size.x, size.y
                )
            }
            None => "Drag to fill a rectangle or an ellipse with the brush".to_owned(),
        }
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.drag = None;
        self.hover = None;
    }
}
