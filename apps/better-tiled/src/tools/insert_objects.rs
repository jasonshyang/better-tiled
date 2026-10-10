//! The insert tools (Tiled's `CreateScalableObjectTool`, `CreatePointObjectTool`,
//! `CreateTileObjectTool` and `CreateTextObjectTool`). Insert Rectangle (R), Ellipse (C) and
//! Capsule (Shift+C) drag out a box from corner to corner (a click makes one without a size, as
//! Tiled's do), Shift making it square. Insert Point (I), Tile (T) and Text (E) show the object
//! under the pointer; a press picks it up, a drag carries it, the release places it; a tile's
//! image and a text box are centred on the pointer, and Insert Tile places the brush's first
//! tile. Positions snap as the preference says, Command toggling grid snapping and Command with
//! Shift the fine grid. The new object goes into the current object layer and becomes the
//! selection. Escape drops what is under way, and with nothing under way goes back to Select
//! Objects (Tiled's `CreateObjectTool`).

use bt_core::{Alignment, MapObject, Shape, TextData};
use bt_editor::{ActionId, Intent, SnapToggle, ToolKind};
use bt_render::VectorItem;
use bt_utils::DRect;
use egui::{Modifiers, PointerButton};
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::{Gesture, Pointer};

/// The size of a new text object, in pixels (Tiled measures "Hello World" in its default font).
const TEXT_SIZE: DVec2 = DVec2::new(96.0, 24.0);

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
    /// A box being dragged out.
    drag: Option<BoxDrag>,
    /// The pointer last seen over the map, and the modifiers then.
    pointer: Option<Pointer>,
    modifiers: Modifiers,
    /// A point, tile or text object picked up by a press, to be placed at the release.
    placing: bool,
}

/// A box being dragged out, in the current layer's pixels, snapped.
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
            pointer: None,
            modifiers: Modifiers::NONE,
            placing: false,
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

    /// The object a point, tile or text tool would place with the pointer at `at`: a point at
    /// the pointer; a tile's image or a text box centred on it, its position where its
    /// alignment puts it; snapped. `None` for the box tools, and for Insert Tile without a
    /// brush.
    fn object_for(
        &self,
        cx: &ToolContext<'_>,
        at: &Pointer,
        toggle: SnapToggle,
    ) -> Option<MapObject> {
        let geometry = cx.map.geometry();
        let local = at.world - cx.layer_offset;
        // A box of `size` centred on the pointer, anchored where `alignment` says.
        let anchored = |size: DVec2, alignment: Alignment| {
            let anchor = local - size / 2.0 + alignment.origin_fraction() * size;
            cx.editor
                .snapped(cx.doc, geometry.screen_to_pixel(anchor), toggle)
        };
        let object = match self.kind {
            InsertKind::Point => MapObject {
                pos: cx.editor.snapped(cx.doc, at.pixel, toggle),
                shape: Shape::Point,
                ..MapObject::default()
            },
            InsertKind::Text => MapObject {
                pos: anchored(TEXT_SIZE, geometry.object_alignment(None)),
                size: TEXT_SIZE,
                shape: Shape::Text(TextData {
                    text: "Hello World".to_owned(),
                    ..TextData::default()
                }),
                ..MapObject::default()
            },
            InsertKind::Tile => {
                let cell = cx.editor.brush_tile(cx.doc)?;
                let tileset = cx.map.tileset_of(cell)?;
                let size = tileset.tile_pixel_size(cell.tile()).as_dvec2();
                MapObject {
                    pos: anchored(size, geometry.object_alignment(Some(tileset))),
                    size,
                    tile: Some(cell),
                    ..MapObject::default()
                }
            }
            InsertKind::Rectangle | InsertKind::Ellipse | InsertKind::Capsule => return None,
        };
        Some(object)
    }

    /// The object placed with the pointer where it was last seen.
    fn object_now(&self, cx: &ToolContext<'_>) -> Option<MapObject> {
        let pointer = self.pointer?;
        self.object_for(cx, &pointer, snap_toggle(self.modifiers))
    }

    fn insert(cx: &mut ToolContext<'_>, object: MapObject) {
        cx.out.push(Intent::InsertObject {
            doc: cx.doc,
            object: Box::new(object),
        });
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
            Gesture::Hover(at) => self.pointer = Some(*at),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.pointer = Some(press.at);
                self.modifiers = press.modifiers;
                if self.drags_a_box() {
                    let at =
                        cx.editor
                            .snapped(cx.doc, press.at.pixel, snap_toggle(press.modifiers));
                    self.drag = Some(BoxDrag {
                        start: at,
                        end: at,
                        square: press.modifiers.shift,
                    });
                } else {
                    self.placing = true;
                }
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                self.pointer = Some(drag.at);
                self.modifiers = drag.modifiers;
                if let Some(state) = &mut self.drag {
                    state.end =
                        cx.editor
                            .snapped(cx.doc, drag.at.pixel, snap_toggle(drag.modifiers));
                    state.square = drag.modifiers.shift;
                }
            }
            Gesture::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                self.pointer = Some(release.at);
                self.modifiers = release.modifiers;
                if let Some(state) = self.drag.take() {
                    let area = InsertObject::box_of(state);
                    let shape = match self.kind {
                        InsertKind::Ellipse => Shape::Ellipse,
                        InsertKind::Capsule => Shape::Capsule,
                        _ => Shape::Rectangle,
                    };
                    InsertObject::insert(
                        cx,
                        MapObject {
                            pos: area.min,
                            size: area.size(),
                            shape,
                            ..MapObject::default()
                        },
                    );
                } else if std::mem::take(&mut self.placing)
                    && let Some(object) = self.object_now(cx)
                {
                    InsertObject::insert(cx, object);
                }
            }
            Gesture::Press(press) if press.button == PointerButton::Secondary => {
                self.drag = None;
                self.placing = false;
            }
            Gesture::Cancel if self.drag.is_none() && !self.placing => {
                cx.out.push(Intent::Run(ActionId::SelectObjectsTool));
            }
            Gesture::Cancel => {
                self.drag = None;
                self.placing = false;
            }
            Gesture::Leave if !self.placing => self.pointer = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let color = cx.editor.settings().theme.selection_color.0;
        let geometry = cx.map.geometry();
        let corners = |area: DRect| {
            [
                area.min,
                DVec2::new(area.max.x, area.min.y),
                area.max,
                DVec2::new(area.min.x, area.max.y),
            ]
        };
        if let Some(drag) = self.drag {
            let area = corners(InsertObject::box_of(drag)).map(|c| cx.pixel_to_world(c));
            out.vectors
                .push(VectorItem::polygon(area.to_vec()).stroke(color, 1.5));
            return;
        }
        if self.drags_a_box() {
            return;
        }
        let Some(object) = self.object_now(cx) else {
            return;
        };
        let width = if self.placing { 1.5 } else { 1.0 };
        match object.shape {
            Shape::Point => {
                let at = cx.pixel_to_world(object.pos);
                let r = cx.world_length(4.0);
                let diamond = vec![
                    at + DVec2::new(0.0, -r),
                    at + DVec2::new(r, 0.0),
                    at + DVec2::new(0.0, r),
                    at + DVec2::new(-r, 0.0),
                ];
                out.vectors
                    .push(VectorItem::polygon(diamond).stroke(color, width));
            }
            _ => {
                let alignment =
                    geometry.object_alignment(object.tile.and_then(|cell| cx.map.tileset_of(cell)));
                let upright = geometry.upright_object_rect(object.pos, object.size, alignment);
                let area = corners(upright).map(|c| c + cx.layer_offset);
                out.vectors
                    .push(VectorItem::polygon(area.to_vec()).stroke(color, width));
            }
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let snapping = format!(
            "{}: snap or not · {}+Shift: fine grid",
            cx.command_key(),
            cx.command_key()
        );
        match self.kind {
            InsertKind::Rectangle | InsertKind::Ellipse | InsertKind::Capsule => {
                format!(
                    "Drag to draw · Shift: square · a click makes one without a size · {snapping}"
                )
            }
            InsertKind::Point => format!("Click to place a point · {snapping}"),
            InsertKind::Text => {
                format!("Click to place text; edit it in the Properties panel · {snapping}")
            }
            InsertKind::Tile if cx.editor.brush_tile(cx.doc).is_none() => {
                "Pick a tile in the Tiles panel to place it as an object".to_owned()
            }
            InsertKind::Tile => format!("Click to place the tile · {snapping}"),
        }
    }

    fn reset(&mut self, _out: &mut Vec<Intent>) {
        self.drag = None;
        self.pointer = None;
        self.placing = false;
    }
}
