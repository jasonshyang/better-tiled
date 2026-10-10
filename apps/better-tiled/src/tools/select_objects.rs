//! Select Objects (S): a click selects the topmost object under the pointer, Shift or Command
//! adds it or takes it out; dragging a selected object (or the one under the pointer) moves the
//! selection as one step, snapping as the preference says unless Command is held; dragging from
//! empty space draws a rubber band that selects what it touches (or encloses, by preference),
//! Shift adding to the selection; a click on empty space clears it.

use bt_core::ObjectId;
use bt_editor::{Intent, ObjectStroke, ObjectStrokeKind, SelectionMode, ToolKind};
use bt_render::VectorItem;
use bt_utils::DRect;
use egui::PointerButton;
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext};
use crate::canvas::{Gesture, Pointer};

/// How near an object's outline a click picks it, in screen points.
const PICK_POINTS: f64 = 4.0;

/// Select Objects' state.
#[derive(Clone, Debug, Default)]
pub(crate) struct SelectObjects {
    /// The topmost object under the pointer.
    hover: Option<ObjectId>,
    press: Option<Press>,
}

/// A press under way.
#[derive(Copy, Clone, Debug)]
struct Press {
    at: Pointer,
    /// The topmost object under the press.
    on: Option<ObjectId>,
    shift: bool,
    command: bool,
    /// What the drag turned into once the pointer moved.
    drag: Option<DragKind>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum DragKind {
    /// The selection moves.
    Move,
    /// A rubber band to `to` (the map's screen space).
    Band { to: DVec2 },
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
}

impl Tool for SelectObjects {
    fn kind(&self) -> ToolKind {
        ToolKind::SelectObjects
    }

    fn handle(&mut self, gesture: &crate::canvas::Gesture, cx: &mut ToolContext<'_>) {
        match gesture {
            Gesture::Hover(at) => self.hover = SelectObjects::top_at(cx, at),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.press = Some(Press {
                    at: press.at,
                    on: SelectObjects::top_at(cx, &press.at),
                    shift: press.modifiers.shift,
                    command: press.modifiers.command,
                    drag: None,
                });
            }
            Gesture::Drag(drag) if drag.button == PointerButton::Primary => {
                let Some(press) = &mut self.press else {
                    return;
                };
                if press.drag.is_none() && drag.moved {
                    press.drag = Some(match press.on {
                        Some(id) => {
                            let selected = SelectObjects::selected(cx);
                            if !selected.contains(&id) {
                                let mode = if press.shift || press.command {
                                    SelectionMode::Add
                                } else {
                                    SelectionMode::Replace
                                };
                                SelectObjects::select(cx, vec![id], mode);
                            }
                            cx.out.push(Intent::ObjectStroke(ObjectStroke::Begin {
                                doc: cx.doc,
                                kind: ObjectStrokeKind::Move,
                            }));
                            DragKind::Move
                        }
                        None => DragKind::Band { to: drag.at.world },
                    });
                }
                match &mut press.drag {
                    Some(DragKind::Move) => cx.out.push(Intent::ObjectStroke(ObjectStroke::Move {
                        delta: drag.at.world - press.at.world,
                        free: drag.modifiers.command,
                    })),
                    Some(DragKind::Band { to }) => *to = drag.at.world,
                    None => {}
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(press) = self.press.take() else {
                    return;
                };
                match press.drag {
                    Some(DragKind::Move) => cx.out.push(Intent::ObjectStroke(ObjectStroke::End)),
                    Some(DragKind::Band { to }) => {
                        let band = DRect::from_points([press.at.world, to]).unwrap_or_default();
                        let ids = cx.editor.objects_in_rect(cx.doc, band);
                        let mode = if press.shift {
                            SelectionMode::Add
                        } else {
                            SelectionMode::Replace
                        };
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
                        Some(id) => SelectObjects::select(cx, vec![id], SelectionMode::Replace),
                        None if !press.shift => {
                            SelectObjects::select(cx, Vec::new(), SelectionMode::Replace);
                        }
                        None => {}
                    },
                }
            }
            Gesture::Cancel => {
                if let Some(press) = self.press.take()
                    && press.drag == Some(DragKind::Move)
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
        if let Some(hover) = self.hover
            && !SelectObjects::selected(cx).contains(&hover)
        {
            for frame in cx.editor.object_frames(cx.doc, &[hover]) {
                out.vectors.push(
                    VectorItem::polygon(frame.corners.to_vec())
                        .stroke(bt_core::Color::rgba(color.r, color.g, color.b, 140), 1.0),
                );
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
                    .fill(bt_core::Color::rgba(color.r, color.g, color.b, 40))
                    .stroke(bt_core::Color::rgba(color.r, color.g, color.b, 200), 1.0),
            );
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let count = SelectObjects::selected(cx).len();
        format!(
            "Click to select, drag to move ({} to move freely), drag on empty space to select an area · Shift or {} adds and removes · {}",
            cx.command_key(),
            cx.command_key(),
            bt_utils::text::plural(count, "object selected", "objects selected")
        )
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if let Some(press) = self.press.take()
            && press.drag == Some(DragKind::Move)
        {
            out.push(Intent::ObjectStroke(ObjectStroke::End));
        }
        self.hover = None;
    }
}
