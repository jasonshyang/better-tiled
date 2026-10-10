//! The insert tools: Insert Rectangle (R), Ellipse (C) and Capsule (Shift+C) drag out a box
//! from corner to corner (a click makes one without a size, as Tiled's do), Shift making it
//! square; Insert Point (I) places a point; Insert Tile (T) places the brush's first tile
//! standing on the pointer; Insert Text (E) places a text box. Positions snap as the
//! preference says. The new object goes into the current object layer and becomes the
//! selection. Escape drops a box being drawn, and with nothing under way goes back to Select
//! Objects (Tiled's `CreateObjectTool`).

use bt_core::{MapObject, Shape, TextData};
use bt_editor::{ActionId, Intent, ToolKind};
use bt_render::VectorItem;
use bt_utils::DRect;
use egui::PointerButton;
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::Gesture;

/// What an insert tool makes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum InsertKind {
    /// Rectangles.
    Rectangle,
    /// Ellipses.
    Ellipse,
    /// Capsules.
    Capsule,
    /// Points.
    Point,
    /// Tile objects.
    Tile,
    /// Text.
    Text,
}

/// An insert tool's state.
#[derive(Clone, Debug)]
pub(crate) struct InsertObject {
    kind: InsertKind,
    /// Where a box drag started and is now, in pixel space, snapped.
    drag: Option<BoxDrag>,
    hover: Option<DVec2>,
}

#[derive(Copy, Clone, Debug)]
struct BoxDrag {
    start: DVec2,
    end: DVec2,
    square: bool,
}

impl InsertObject {
    /// The tool that makes `kind`.
    pub(crate) fn new(kind: InsertKind) -> InsertObject {
        InsertObject {
            kind,
            drag: None,
            hover: None,
        }
    }

    fn drags_a_box(&self) -> bool {
        matches!(
            self.kind,
            InsertKind::Rectangle | InsertKind::Ellipse | InsertKind::Capsule
        )
    }

    /// The box of a drag: from corner to corner, square on the larger side with Shift.
    fn box_of(drag: BoxDrag) -> DRect {
        let mut delta = drag.end - drag.start;
        if drag.square {
            let side = delta.x.abs().max(delta.y.abs());
            delta = DVec2::new(side.copysign(delta.x), side.copysign(delta.y));
        }
        DRect::from_points([drag.start, drag.start + delta]).unwrap_or_default()
    }

    fn object_at(&self, cx: &ToolContext<'_>, at: DVec2) -> Option<MapObject> {
        let object = match self.kind {
            InsertKind::Point => MapObject {
                pos: at,
                shape: Shape::Point,
                ..MapObject::default()
            },
            InsertKind::Text => MapObject {
                pos: at,
                size: DVec2::new(96.0, 24.0),
                shape: Shape::Text(TextData {
                    text: "Hello World".to_owned(),
                    ..TextData::default()
                }),
                ..MapObject::default()
            },
            InsertKind::Tile => {
                let cell = cx.editor.brush_tile(cx.doc)?;
                let size = cx
                    .map
                    .tileset_of(cell)
                    .map_or(DVec2::ZERO, |t| t.tile_pixel_size(cell.tile()).as_dvec2());
                MapObject {
                    pos: at,
                    size,
                    tile: Some(cell),
                    ..MapObject::default()
                }
            }
            InsertKind::Rectangle | InsertKind::Ellipse | InsertKind::Capsule => return None,
        };
        Some(object)
    }
}

impl Tool for InsertObject {
    fn kind(&self) -> ToolKind {
        match self.kind {
            InsertKind::Rectangle => ToolKind::InsertRectangle,
            InsertKind::Ellipse => ToolKind::InsertEllipse,
            InsertKind::Capsule => ToolKind::InsertCapsule,
            InsertKind::Point => ToolKind::InsertPoint,
            InsertKind::Tile => ToolKind::InsertTile,
            InsertKind::Text => ToolKind::InsertText,
        }
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = Some(cx.editor.snapped(cx.doc, at.pixel)),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let at = cx.editor.snapped(cx.doc, press.at.pixel);
                if self.drags_a_box() {
                    self.drag = Some(BoxDrag {
                        start: at,
                        end: at,
                        square: press.modifiers.shift,
                    });
                } else if let Some(object) = self.object_at(cx, at) {
                    cx.out.push(Intent::InsertObject {
                        doc: cx.doc,
                        object: Box::new(object),
                    });
                }
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                if let Some(state) = &mut self.drag {
                    state.end = cx.editor.snapped(cx.doc, drag.at.pixel);
                    state.square = drag.modifiers.shift;
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                if let Some(state) = self.drag.take() {
                    let area = InsertObject::box_of(state);
                    let shape = match self.kind {
                        InsertKind::Ellipse => Shape::Ellipse,
                        InsertKind::Capsule => Shape::Capsule,
                        _ => Shape::Rectangle,
                    };
                    cx.out.push(Intent::InsertObject {
                        doc: cx.doc,
                        object: Box::new(MapObject {
                            pos: area.min,
                            size: area.size(),
                            shape,
                            ..MapObject::default()
                        }),
                    });
                }
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => self.drag = None,
            Gesture::Cancel if self.drag.is_none() => {
                cx.out.push(Intent::Run(ActionId::SelectObjectsTool));
            }
            Gesture::Cancel => self.drag = None,
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let color = cx.editor.settings().theme.selection_color.0;
        let on_screen = |area: DRect| {
            [
                area.min,
                DVec2::new(area.max.x, area.min.y),
                area.max,
                DVec2::new(area.min.x, area.max.y),
            ]
            .map(|corner| cx.pixel_to_world(corner))
            .to_vec()
        };
        if let Some(drag) = self.drag {
            out.vectors.push(
                VectorItem::polygon(on_screen(InsertObject::box_of(drag))).stroke(color, 1.5),
            );
        } else if let Some(at) = self.hover {
            let mark =
                DRect::from_min_size(at, DVec2::ZERO).expanded(DVec2::splat(cx.world_length(3.0)));
            out.vectors
                .push(VectorItem::polygon(on_screen(mark)).stroke(color, 1.0));
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        match self.kind {
            InsertKind::Rectangle | InsertKind::Ellipse | InsertKind::Capsule => {
                "Drag to draw · Shift: square · a click makes one without a size".to_owned()
            }
            InsertKind::Point => "Click to place a point".to_owned(),
            InsertKind::Text => "Click to place text; edit it in the Properties panel".to_owned(),
            InsertKind::Tile if cx.editor.brush_tile(cx.doc).is_none() => {
                "Pick a tile in the Tiles panel to place it as an object".to_owned()
            }
            InsertKind::Tile => "Click to place the tile".to_owned(),
        }
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.drag = None;
        self.hover = None;
    }
}
