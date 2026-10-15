//! Select Objects (S), as Tiled's `ObjectSelectionTool`: a click selects the topmost object
//! under the pointer, Shift or Command adds it or takes it out, and a click on an object
//! already selected switches its handles between resizing and rotating. Dragging a selected
//! object (or the one under the pointer) moves the selection as one step, snapping as the
//! preference says, Command toggling grid snapping and Command with Shift the fine grid.
//! Dragging a resize handle resizes the selection, Command keeping its proportions and Shift
//! keeping its middle; dragging a rotate handle turns it around its middle, Command in steps of
//! 15°. Dragging from empty space draws a rubber band that selects what it touches (or
//! encloses, by preference), Shift adding to the selection; a click on empty space clears it.

use bt_core::{Color, ObjectId};
use bt_editor::{
    HandleFrame, Intent, ObjectStroke, ObjectStrokeKind, ResizeHandle, SelectionMode, ToolKind,
};
use bt_render::VectorItem;
use bt_utils::DRect;
use egui::PointerButton;
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::{Gesture, Pointer};

/// How near an object's outline a click picks it, in screen points.
const PICK_POINTS: f64 = 4.0;

/// How near a handle a press grabs it, in screen points.
const HANDLE_POINTS: f64 = 6.0;

/// How far out of the corners rotate handles sit, in screen points.
const ROTATE_OUT_POINTS: f64 = 14.0;

/// Half the side of a resize handle's square, in screen points.
const HANDLE_HALF_POINTS: f64 = 3.5;

/// Select Objects' state.
#[derive(Clone, Debug, Default)]
pub(crate) struct SelectObjects {
    /// The topmost object under the pointer.
    hover: Option<ObjectId>,
    press: Option<Press>,
    /// Which handles the selection shows.
    handles: HandleMode,
}

/// Which handles the selection shows; a click on a selected object switches them.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum HandleMode {
    /// Resize handles at the corners and sides.
    #[default]
    Resize,
    /// Rotate handles out of the corners.
    Rotate,
}

/// A press under way.
#[derive(Copy, Clone, Debug)]
struct Press {
    at: Pointer,
    /// The topmost object under the press.
    on: Option<ObjectId>,
    /// The handle under the press.
    grabbed: Option<Grab>,
    shift: bool,
    command: bool,
    /// What the drag turned into once the pointer moved.
    drag: Option<DragKind>,
}

/// A handle a press grabbed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Grab {
    Resize(ResizeHandle),
    Rotate,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum DragKind {
    /// The selection moves.
    Move,
    /// A rubber band to `to` (the map's screen space).
    Band { to: DVec2 },
    /// A handle is dragged.
    Handle(Grab),
}

impl SelectObjects {
    fn top_at(cx: &ToolContext<'_>, at: &Pointer) -> Option<ObjectId> {
        cx.editor
            .objects_at(cx.doc, at.world, cx.world_length(PICK_POINTS))
            .first()
            .copied()
    }

    fn selected(cx: &ToolContext<'_>) -> Vec<ObjectId> {
        cx.selection()
            .map(|s| s.objects.clone())
            .unwrap_or_default()
    }

    fn select(cx: &mut ToolContext<'_>, ids: Vec<ObjectId>, mode: SelectionMode) {
        cx.out.push(Intent::SelectObjects {
            doc: cx.doc,
            ids,
            mode,
        });
    }

    /// The handles the selection shows, where they are on screen; none without a selection, or
    /// for a lone point.
    fn shown_handles(&self, cx: &ToolContext<'_>) -> Vec<ShownHandle> {
        let Some(frame) = cx
            .editor
            .selection_handles(cx.doc)
            .filter(|frame| frame.resizable)
        else {
            return Vec::new();
        };
        match self.handles {
            HandleMode::Resize => ResizeHandle::ALL
                .into_iter()
                .map(|handle| ShownHandle {
                    grab: Grab::Resize(handle),
                    at: handle.on(frame.corners),
                })
                .collect(),
            HandleMode::Rotate => rotate_spots(&frame, cx.world_length(ROTATE_OUT_POINTS))
                .into_iter()
                .map(|at| ShownHandle {
                    grab: Grab::Rotate,
                    at,
                })
                .collect(),
        }
    }

    /// The handle within reach of `world`, the nearest first.
    fn handle_at(&self, cx: &ToolContext<'_>, world: DVec2) -> Option<Grab> {
        let reach = cx.world_length(HANDLE_POINTS);
        self.shown_handles(cx)
            .into_iter()
            .filter(|shown| shown.at.distance(world) <= reach)
            .min_by(|a, b| a.at.distance(world).total_cmp(&b.at.distance(world)))
            .map(|shown| shown.grab)
    }

    fn begin(cx: &mut ToolContext<'_>, kind: ObjectStrokeKind) {
        cx.out.push(Intent::ObjectStroke(ObjectStroke::Begin {
            doc: cx.doc,
            kind,
        }));
    }
}

/// A handle where it is drawn.
#[derive(Copy, Clone, Debug)]
struct ShownHandle {
    grab: Grab,
    at: DVec2,
}

/// Where rotate handles sit: out of each corner, away from the middle, by `out`.
fn rotate_spots(frame: &HandleFrame, out: f64) -> [DVec2; 4] {
    let centre = frame.centre();
    frame
        .corners
        .map(|corner| corner + (corner - centre).normalize_or_zero() * out)
}

impl Tool for SelectObjects {
    fn kind(&self) -> ToolKind {
        ToolKind::SelectObjects
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = SelectObjects::top_at(cx, at),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                let grabbed = self.handle_at(cx, press.at.world);
                self.press = Some(Press {
                    at: press.at,
                    on: SelectObjects::top_at(cx, &press.at),
                    grabbed,
                    shift: press.modifiers.shift,
                    command: press.modifiers.command,
                    drag: None,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                let Some(press) = &mut self.press else {
                    return;
                };
                if press.grabbed.is_none()
                    && press.on.is_some()
                    && cx.editor.template_of(cx.doc).is_some()
                {
                    return;
                }
                if press.drag.is_none() && drag.moved {
                    press.drag = Some(match (press.grabbed, press.on) {
                        (Some(grab), _) => {
                            let kind = match grab {
                                Grab::Resize(_) => ObjectStrokeKind::Resize,
                                Grab::Rotate => ObjectStrokeKind::Rotate,
                            };
                            SelectObjects::begin(cx, kind);
                            DragKind::Handle(grab)
                        }
                        (None, Some(id)) => {
                            if !SelectObjects::selected(cx).contains(&id) {
                                let mode = if press.shift || press.command {
                                    SelectionMode::Add
                                } else {
                                    SelectionMode::Replace
                                };
                                SelectObjects::select(cx, vec![id], mode);
                            }
                            SelectObjects::begin(cx, ObjectStrokeKind::Move);
                            DragKind::Move
                        }
                        (None, None) => DragKind::Band { to: drag.at.world },
                    });
                }
                let step = match &mut press.drag {
                    Some(DragKind::Move) => Some(ObjectStroke::Move {
                        delta: drag.at.world - press.at.world,
                        snap: snap_toggle(drag.modifiers),
                    }),
                    Some(DragKind::Handle(Grab::Resize(handle))) => Some(ObjectStroke::Resize {
                        handle: *handle,
                        to: drag.at.world,
                        from_centre: drag.modifiers.shift,
                        keep_proportions: drag.modifiers.command,
                    }),
                    Some(DragKind::Handle(Grab::Rotate)) => Some(ObjectStroke::Rotate {
                        from: press.at.world,
                        to: drag.at.world,
                        snap: drag.modifiers.command,
                    }),
                    Some(DragKind::Band { to }) => {
                        *to = drag.at.world;
                        None
                    }
                    None => None,
                };
                if let Some(step) = step {
                    cx.out.push(Intent::ObjectStroke(step));
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(press) = self.press.take() else {
                    return;
                };
                match press.drag {
                    Some(DragKind::Move | DragKind::Handle(_)) => {
                        cx.out.push(Intent::ObjectStroke(ObjectStroke::End));
                    }
                    Some(DragKind::Band { to }) => {
                        let band = DRect::from_points([press.at.world, to]).unwrap_or_default();
                        let ids = cx.editor.objects_in_rect(cx.doc, band);
                        let mode = if press.shift {
                            SelectionMode::Add
                        } else {
                            SelectionMode::Replace
                        };
                        self.handles = HandleMode::Resize;
                        SelectObjects::select(cx, ids, mode);
                    }
                    None => match press.on {
                        Some(id) if press.shift || press.command => {
                            let mode = if SelectObjects::selected(cx).contains(&id) {
                                SelectionMode::Subtract
                            } else {
                                SelectionMode::Add
                            };
                            SelectObjects::select(cx, vec![id], mode);
                        }
                        // A click on a selected object switches its handles, as in Tiled.
                        Some(id) if SelectObjects::selected(cx).contains(&id) => {
                            self.handles = match self.handles {
                                HandleMode::Resize => HandleMode::Rotate,
                                HandleMode::Rotate => HandleMode::Resize,
                            };
                        }
                        Some(id) => {
                            self.handles = HandleMode::Resize;
                            SelectObjects::select(cx, vec![id], SelectionMode::Replace);
                        }
                        None if !press.shift && press.grabbed.is_none() => {
                            SelectObjects::select(cx, Vec::new(), SelectionMode::Replace);
                        }
                        None => {}
                    },
                }
            }
            Gesture::Cancel => {
                if let Some(press) = self.press.take()
                    && matches!(press.drag, Some(DragKind::Move | DragKind::Handle(_)))
                {
                    cx.out.push(Intent::ObjectStroke(ObjectStroke::Cancel));
                }
            }
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let color = cx.editor.settings().theme.selection_color.0;
        let faint = |alpha| Color::rgba(color.r, color.g, color.b, alpha);
        if let Some(hover) = self.hover
            && !SelectObjects::selected(cx).contains(&hover)
        {
            for frame in cx.editor.object_frames(cx.doc, &[hover]) {
                out.vectors
                    .push(VectorItem::polygon(frame.corners.to_vec()).stroke(faint(140), 1.0));
            }
        }
        if let Some(Press {
            at,
            drag: Some(DragKind::Band { to }),
            ..
        }) = self.press
        {
            let band = DRect::from_points([at.world, to]).unwrap_or_default();
            out.vectors.push(
                VectorItem::rect(band)
                    .fill(faint(40))
                    .stroke(faint(200), 1.0),
            );
            return;
        }
        let half = cx.world_length(HANDLE_HALF_POINTS);
        for shown in self.shown_handles(cx) {
            let square = DRect::from_min_size(shown.at, DVec2::ZERO).expanded(DVec2::splat(half));
            let item = match shown.grab {
                Grab::Resize(_) => VectorItem::rect(square),
                Grab::Rotate => VectorItem::ellipse(square.expanded(DVec2::splat(half * 0.4))),
            };
            out.vectors.push(
                item.fill(Color::rgba(255, 255, 255, 230))
                    .stroke(faint(255), 1.0),
            );
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let count = SelectObjects::selected(cx).len();
        let command = cx.command_key();
        let handles = match self.handles {
            HandleMode::Resize => {
                format!(
                    "drag a handle to resize ({command}: keep proportions, Shift: from the middle)"
                )
            }
            HandleMode::Rotate => format!("drag a handle to rotate ({command}: in steps of 15°)"),
        };
        if cx.editor.template_of(cx.doc).is_some() {
            return format!(
                "Edit the template base in Properties; {handles} · click the object to switch its handles"
            );
        }
        format!(
            "Click to select, drag to move, drag on empty space to select an area · Shift or {command} adds and removes · click a selected object to switch its handles; {handles} · {}",
            bt_utils::text::plural(count, "object selected", "objects selected")
        )
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if let Some(press) = self.press.take()
            && matches!(press.drag, Some(DragKind::Move | DragKind::Handle(_)))
        {
            out.push(Intent::ObjectStroke(ObjectStroke::End));
        }
        self.hover = None;
    }
}
