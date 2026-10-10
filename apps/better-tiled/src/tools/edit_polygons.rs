//! Edit Polygons (O), as Tiled's `EditPolygonTool`
//! (`reference/tiled/src/tiled/editpolygontool.cpp`): the nodes of the selected polygons and
//! polylines show as handles. A click on a node selects it, Shift or Command adds it or takes it
//! out; dragging a node moves the selected nodes as one step, snapping as the preference says
//! (Command toggles grid snapping, with Shift the fine grid). A click on another object selects
//! it, so its nodes show; dragging from empty space draws a rubber band that selects the nodes
//! inside it; a double-click on a segment adds a node there. Delete removes the selected nodes,
//! and Join Nodes and Split Segments are in the Objects menu.

use bt_core::Color;
use bt_editor::{Intent, Node, ObjectStroke, ObjectStrokeKind, SelectionMode, ToolKind};
use bt_render::VectorItem;
use bt_utils::DRect;
use egui::PointerButton;
use glam::DVec2;

use super::tool::{Overlay, Tool, ToolContext, snap_toggle};
use crate::canvas::{Gesture, Pointer};

/// How near a node or a segment the pointer picks it, in screen points.
const REACH_POINTS: f64 = 6.0;

/// Half the side of a node's square, in screen points.
const NODE_HALF_POINTS: f64 = 3.5;

/// Edit Polygons' state.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditPolygons {
    /// The node under the pointer.
    hover: Option<Node>,
    press: Option<NodePress>,
}

/// A press under way.
#[derive(Copy, Clone, Debug)]
struct NodePress {
    at: Pointer,
    /// The node under the press.
    node: Option<Node>,
    shift: bool,
    command: bool,
    drag: Option<NodeDrag>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum NodeDrag {
    /// The selected nodes move.
    Move,
    /// A rubber band to `to` (the map's screen space).
    Band { to: DVec2 },
}

impl EditPolygons {
    fn selected(cx: &ToolContext<'_>) -> Vec<Node> {
        cx.selection().map(|s| s.nodes.clone()).unwrap_or_default()
    }

    fn select(cx: &mut ToolContext<'_>, nodes: Vec<Node>, mode: SelectionMode) {
        cx.out.push(Intent::SelectNodes {
            doc: cx.doc,
            nodes,
            mode,
        });
    }

    fn mode_of(shift: bool, command: bool) -> SelectionMode {
        if shift || command {
            SelectionMode::Add
        } else {
            SelectionMode::Replace
        }
    }
}

impl Tool for EditPolygons {
    fn kind(&self) -> ToolKind {
        ToolKind::EditPolygons
    }

    fn handle(&mut self, gesture: &Gesture, cx: &mut ToolContext<'_>) {
        let reach = cx.world_length(REACH_POINTS);
        match gesture {
            Gesture::Hover(at) => self.hover = cx.editor.node_at(cx.doc, at.world, reach),
            Gesture::Press(press) if press.button == PointerButton::Primary => {
                self.press = Some(NodePress {
                    at: press.at,
                    node: cx.editor.node_at(cx.doc, press.at.world, reach),
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
                    press.drag = Some(match press.node {
                        Some(node) => {
                            if !EditPolygons::selected(cx).contains(&node) {
                                let mode = EditPolygons::mode_of(press.shift, press.command);
                                EditPolygons::select(cx, vec![node], mode);
                            }
                            cx.out.push(Intent::ObjectStroke(ObjectStroke::Begin {
                                doc: cx.doc,
                                kind: ObjectStrokeKind::MoveNodes,
                            }));
                            NodeDrag::Move
                        }
                        None => NodeDrag::Band { to: drag.at.world },
                    });
                }
                match &mut press.drag {
                    Some(NodeDrag::Move) => {
                        cx.out.push(Intent::ObjectStroke(ObjectStroke::MoveNodes {
                            delta: drag.at.world - press.at.world,
                            snap: snap_toggle(drag.modifiers),
                        }));
                    }
                    Some(NodeDrag::Band { to }) => *to = drag.at.world,
                    None => {}
                }
            }
            Gesture::Release(release) if release.button == PointerButton::Primary => {
                let Some(press) = self.press.take() else {
                    return;
                };
                match press.drag {
                    Some(NodeDrag::Move) => cx.out.push(Intent::ObjectStroke(ObjectStroke::End)),
                    Some(NodeDrag::Band { to }) => {
                        let band = DRect::from_points([press.at.world, to]).unwrap_or_default();
                        let inside: Vec<Node> = cx
                            .editor
                            .node_handles(cx.doc)
                            .into_iter()
                            .filter(|handle| band.contains(handle.at))
                            .map(|handle| handle.node)
                            .collect();
                        let mode = EditPolygons::mode_of(press.shift, press.command);
                        EditPolygons::select(cx, inside, mode);
                    }
                    None => match press.node {
                        Some(node) if press.shift || press.command => {
                            let mode = if EditPolygons::selected(cx).contains(&node) {
                                SelectionMode::Subtract
                            } else {
                                SelectionMode::Add
                            };
                            EditPolygons::select(cx, vec![node], mode);
                        }
                        Some(node) => EditPolygons::select(cx, vec![node], SelectionMode::Replace),
                        None => {
                            // Another object shows its nodes; empty space lets go of them.
                            let under = cx
                                .editor
                                .objects_at(cx.doc, press.at.world, reach)
                                .first()
                                .copied();
                            match under {
                                Some(object) => cx.out.push(Intent::SelectObjects {
                                    doc: cx.doc,
                                    ids: vec![object],
                                    mode: SelectionMode::Replace,
                                }),
                                None => {
                                    EditPolygons::select(cx, Vec::new(), SelectionMode::Replace);
                                }
                            }
                        }
                    },
                }
            }
            Gesture::DoubleClick(click) => {
                if let Some(segment) = cx.editor.segment_at(cx.doc, click.at.world, reach) {
                    cx.out.push(Intent::InsertNode {
                        doc: cx.doc,
                        segment,
                        near: click.at.world,
                    });
                }
            }
            Gesture::Cancel => match self.press.take() {
                Some(NodePress {
                    drag: Some(NodeDrag::Move),
                    ..
                }) => cx.out.push(Intent::ObjectStroke(ObjectStroke::Cancel)),
                Some(_) => {}
                None => EditPolygons::select(cx, Vec::new(), SelectionMode::Replace),
            },
            Gesture::Leave => self.hover = None,
            _ => {}
        }
    }

    fn overlay(&self, cx: &ToolContext<'_>, out: &mut Overlay) {
        let color = cx.editor.settings().theme.selection_color.0;
        let selected = EditPolygons::selected(cx);
        let half = cx.world_length(NODE_HALF_POINTS);
        for handle in cx.editor.node_handles(cx.doc) {
            let grown = if self.hover == Some(handle.node) {
                half * 1.4
            } else {
                half
            };
            let square = DRect::from_min_size(handle.at, DVec2::ZERO).expanded(DVec2::splat(grown));
            let fill = if selected.contains(&handle.node) {
                color
            } else {
                Color::rgba(255, 255, 255, 230)
            };
            out.vectors
                .push(VectorItem::rect(square).fill(fill).stroke(color, 1.0));
        }
        if let Some(NodePress {
            at,
            drag: Some(NodeDrag::Band { to }),
            ..
        }) = self.press
        {
            let band = DRect::from_points([at.world, to]).unwrap_or_default();
            out.vectors.push(
                VectorItem::rect(band)
                    .fill(Color::rgba(color.r, color.g, color.b, 40))
                    .stroke(Color::rgba(color.r, color.g, color.b, 200), 1.0),
            );
        }
    }

    fn hint(&self, cx: &ToolContext<'_>) -> String {
        let command = cx.command_key();
        let count = EditPolygons::selected(cx).len();
        format!(
            "Click a polygon or polyline to edit its nodes · drag nodes to move them · Shift or {command} adds and removes · double-click a segment to add a node · Delete removes nodes · {}",
            bt_utils::text::plural(count, "node selected", "nodes selected")
        )
    }

    fn reset(&mut self, out: &mut Vec<Intent>) {
        if let Some(NodePress {
            drag: Some(NodeDrag::Move),
            ..
        }) = self.press.take()
        {
            out.push(Intent::ObjectStroke(ObjectStroke::End));
        }
        self.hover = None;
    }
}
