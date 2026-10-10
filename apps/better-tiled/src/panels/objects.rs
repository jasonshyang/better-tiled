//! The Objects panel (Tiled's `ObjectsDock` and `ObjectsView`,
//! `reference/tiled/src/tiled/objectsdock.cpp` and `objectsview.cpp`, over its `MapObjectModel`):
//! the object layers inside their groups, topmost first, each with its objects topmost first, an
//! object shown by its name (or its kind and id) and its class; a filter that keeps the objects
//! whose name, class, kind or id holds a text, and the layers holding them. Clicking an object
//! selects it (Shift for a range, ⌘ to add or take it away), and the selection shown is the
//! editor's; double-clicking centres the view on it; the eyes show and hide objects and layers;
//! clicking a layer makes it current. Dragging objects puts them between the objects of a layer
//! drawn in list order, or on top of another layer when dropped on its row.

use std::collections::HashSet;

use bt_core::{DrawOrder, Layer, LayerId, Map, MapObject, ObjectId};
use bt_editor::{DocumentId, Editor, Intent, ObjectRow, Selection, SelectionMode};
use egui::{Modifiers, Rect, Sense, Stroke};

use super::layers::display_name;

/// How far each level of the tree is indented, in points.
const INDENT: f32 = 14.0;

/// The panel's own state.
#[derive(Debug, Default)]
pub(crate) struct ObjectsPanel {
    /// What the filter field holds.
    filter: String,
    /// The layers whose rows are folded.
    collapsed: HashSet<InDocument<LayerId>>,
    /// Where a Shift-click's range starts: the object clicked last without Shift.
    anchor: Option<InDocument<ObjectId>>,
    /// The objects being dragged.
    drag: Option<InDocument<Vec<ObjectId>>>,
}

/// Something of a document's: a layer, an object, the objects of a drag.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InDocument<T> {
    doc: DocumentId,
    item: T,
}

/// What the rows of a frame share, and what they collect.
struct Rows<'a> {
    doc: DocumentId,
    selection: &'a Selection,
    /// The rows shown (inside unfolded layers), topmost first.
    shown: Vec<ObjectRow<'a>>,
    /// Where each shown row was drawn this frame, if it was.
    rects: Vec<Option<Rect>>,
    /// Where each drawn row's text starts.
    lefts: Vec<f32>,
    /// A click on an object, resolved once every row is drawn.
    click: Option<ObjectClick>,
}

/// A click on an object's row, and the modifiers held.
#[derive(Copy, Clone, Debug)]
struct ObjectClick {
    object: ObjectId,
    modifiers: Modifiers,
}

/// Where dropped objects would go: a layer and a place in its list, and how the panel shows it.
#[derive(Copy, Clone, Debug, PartialEq)]
struct ObjectDrop {
    layer: LayerId,
    index: usize,
    /// A line at this height from `left`, or the layer's row framed when `None`.
    line: Option<f32>,
    left: f32,
    row: Rect,
}

impl ObjectsPanel {
    /// Shows the object layers and objects of `doc`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        out: &mut Vec<Intent>,
    ) {
        let (Some(map), Some(document)) = (editor.map_of(doc), editor.document(doc)) else {
            return;
        };
        ui.horizontal(|ui| {
            let label = ui.label("Filter");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("name, class, kind or id")
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
        });
        ui.separator();
        let filtering = !self.filter.trim().is_empty();
        let shown = self.unfolded(doc, editor.object_rows(doc, &self.filter), filtering);
        if shown.is_empty() {
            ui.weak(if filtering {
                "No object matches the filter."
            } else {
                "The map has no object layers."
            });
            return;
        }
        let count = shown.len();
        let mut rows = Rows {
            doc,
            selection: document.selection(),
            shown,
            rects: vec![None; count],
            lefts: vec![0.0; count],
            click: None,
        };
        let height = ui.spacing().interact_size.y;
        let scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, height, count, |ui, range| {
                for place in range {
                    self.show_row(ui, place, filtering, &mut rows, out);
                }
            });
        if let Some(click) = rows.click {
            self.select_on_click(click, &rows, out);
        }
        self.drag_objects(ui, map, &rows, scroll.inner_rect, out);
    }

    /// The rows outside folded layers (all of them while filtering, as Tiled shows every match).
    fn unfolded<'a>(
        &self,
        doc: DocumentId,
        rows: Vec<ObjectRow<'a>>,
        filtering: bool,
    ) -> Vec<ObjectRow<'a>> {
        let mut shown = Vec::with_capacity(rows.len());
        let mut folded_at: Option<usize> = None;
        for row in rows {
            if let Some(depth) = folded_at {
                if row.depth() > depth {
                    continue;
                }
                folded_at = None;
            }
            if let ObjectRow::Layer { layer, depth } = row
                && !filtering
                && self.collapsed.contains(&InDocument {
                    doc,
                    item: layer.id,
                })
            {
                folded_at = Some(depth);
            }
            shown.push(row);
        }
        shown
    }

    fn show_row(
        &mut self,
        ui: &mut egui::Ui,
        place: usize,
        filtering: bool,
        rows: &mut Rows<'_>,
        out: &mut Vec<Intent>,
    ) {
        let Some(row) = rows.shown.get(place).copied() else {
            return;
        };
        let mut left = 0.0;
        let drawn = ui.horizontal(|ui| match row {
            ObjectRow::Layer { layer, depth } => {
                if eye(ui, layer.visible, &display_name(layer)) {
                    out.push(Intent::SetLayerVisible {
                        doc: rows.doc,
                        layer: layer.id,
                        visible: !layer.visible,
                    });
                }
                ui.add_space(INDENT * depth as f32);
                left = ui.cursor().left();
                self.show_fold(ui, rows.doc, layer.id, filtering);
                self.show_layer_name(ui, layer, rows, out);
            }
            ObjectRow::Object { object, depth, .. } => {
                let label = object_label(object);
                if eye(ui, object.visible, &label) {
                    out.push(Intent::SetObjectVisible {
                        doc: rows.doc,
                        object: object.id,
                        visible: !object.visible,
                    });
                }
                ui.add_space(INDENT * depth as f32);
                left = ui.cursor().left();
                self.show_object_name(ui, object, label, rows, out);
                if !object.class.is_empty() {
                    ui.weak(&object.class);
                }
            }
        });
        if let Some(rect) = rows.rects.get_mut(place) {
            *rect = Some(drawn.response.rect);
        }
        if let Some(slot) = rows.lefts.get_mut(place) {
            *slot = left;
        }
    }

    /// The arrow that folds a layer's rows away; none while filtering.
    fn show_fold(&mut self, ui: &mut egui::Ui, doc: DocumentId, layer: LayerId, filtering: bool) {
        if filtering {
            return;
        }
        let key = InDocument { doc, item: layer };
        let folded = self.collapsed.contains(&key);
        let arrow = if folded { "⏵" } else { "⏷" };
        if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
            if folded {
                self.collapsed.remove(&key);
            } else {
                self.collapsed.insert(key);
            }
        }
    }

    /// A layer's name: clicking it makes the layer current, with no object selected (Tiled's
    /// `ObjectsView::mousePressEvent`).
    fn show_layer_name(
        &self,
        ui: &mut egui::Ui,
        layer: &Layer,
        rows: &Rows<'_>,
        out: &mut Vec<Intent>,
    ) {
        let mut text = egui::RichText::new(display_name(layer));
        if rows.selection.current_layer == Some(layer.id) {
            text = text.strong();
        }
        if !layer.visible {
            text = text.weak();
        }
        let response = ui
            .add(egui::Button::selectable(false, text))
            .on_hover_text(layer.kind_name());
        if response.clicked() {
            out.push(Intent::SelectLayer {
                doc: rows.doc,
                layer: layer.id,
            });
            out.push(Intent::SelectObjects {
                doc: rows.doc,
                ids: Vec::new(),
                mode: SelectionMode::Replace,
            });
        }
    }

    /// An object's name: a click selects, a double-click centres the view on it, a drag carries
    /// it (with the other selected objects when it is selected).
    fn show_object_name(
        &mut self,
        ui: &mut egui::Ui,
        object: &MapObject,
        label: String,
        rows: &mut Rows<'_>,
        out: &mut Vec<Intent>,
    ) {
        let selected = rows.selection.objects.contains(&object.id);
        let mut text = egui::RichText::new(label);
        if object.name.is_empty() || !object.visible {
            text = text.weak();
        }
        let response = ui
            .add(egui::Button::selectable(selected, text))
            .interact(Sense::drag())
            .on_hover_text(format!("{} {}", object.kind_name(), object.id.0));
        if response.double_clicked() {
            out.push(Intent::CenterOnObject {
                doc: rows.doc,
                object: object.id,
            });
        } else if response.clicked() {
            rows.click = Some(ObjectClick {
                object: object.id,
                modifiers: ui.input(|input| input.modifiers),
            });
        }
        if response.drag_started() {
            let objects = if selected {
                rows.selection.objects.clone()
            } else {
                out.push(Intent::SelectObjects {
                    doc: rows.doc,
                    ids: vec![object.id],
                    mode: SelectionMode::Replace,
                });
                vec![object.id]
            };
            self.drag = Some(InDocument {
                doc: rows.doc,
                item: objects,
            });
        }
    }

    /// A click on an object selects it alone, the objects shown from the anchor with Shift, or
    /// adds or takes it away with ⌘.
    fn select_on_click(&mut self, click: ObjectClick, rows: &Rows<'_>, out: &mut Vec<Intent>) {
        let doc = rows.doc;
        if click.modifiers.shift {
            let anchor = self
                .anchor
                .as_ref()
                .filter(|anchor| anchor.doc == doc)
                .map_or(click.object, |anchor| anchor.item);
            out.push(Intent::SelectObjects {
                doc,
                ids: objects_between(&rows.shown, anchor, click.object),
                mode: SelectionMode::Replace,
            });
            return;
        }
        self.anchor = Some(InDocument {
            doc,
            item: click.object,
        });
        let mode = if !click.modifiers.command {
            SelectionMode::Replace
        } else if rows.selection.objects.contains(&click.object) {
            SelectionMode::Subtract
        } else {
            SelectionMode::Add
        };
        out.push(Intent::SelectObjects {
            doc,
            ids: vec![click.object],
            mode,
        });
    }

    /// Follows a drag of objects: shows where they would go, and puts them there when it lets go
    /// over a place that takes them.
    fn drag_objects(
        &mut self,
        ui: &egui::Ui,
        map: &Map,
        rows: &Rows<'_>,
        area: Rect,
        out: &mut Vec<Intent>,
    ) {
        let Some(drag) = &self.drag else {
            return;
        };
        if drag.doc != rows.doc {
            self.drag = None;
            return;
        }
        let drop = ui
            .input(|input| input.pointer.latest_pos())
            .filter(|at| area.contains(*at))
            .and_then(|at| drop_place(rows, at.y))
            .filter(|drop| drop_allowed(map, &drag.item, drop.layer));
        let painter = ui.painter().with_clip_rect(area);
        let stroke = Stroke::new(2.0, ui.visuals().selection.stroke.color);
        if let Some(drop) = drop {
            match drop.line {
                Some(y) => {
                    painter.hline(drop.left..=area.right(), y, stroke);
                }
                None => {
                    let frame = Rect::from_x_y_ranges(area.x_range(), drop.row.y_range());
                    painter.rect_stroke(frame, 2.0, stroke, egui::StrokeKind::Inside);
                }
            }
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            if let Some(drop) = drop {
                out.push(Intent::ReorderObjects {
                    doc: rows.doc,
                    objects: drag.item.clone(),
                    layer: drop.layer,
                    index: drop.index,
                });
            }
            self.drag = None;
        }
    }
}

/// An eye that shows or hides what the row shows, named for screen readers by what it does;
/// whether it was clicked.
fn eye(ui: &mut egui::Ui, visible: bool, name: &str) -> bool {
    let (icon, action) = if visible {
        ("👁", "Hide")
    } else {
        ("  ", "Show")
    };
    let response = ui
        .add(
            egui::Button::new(icon)
                .frame(false)
                .min_size(egui::vec2(18.0, 0.0)),
        )
        .on_hover_text(action);
    let what = format!("{action} {name}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &what));
    response.clicked()
}

/// The object rows from `from` to `to`, both included, in the order shown; `to` alone when
/// `from` is not shown.
fn objects_between(rows: &[ObjectRow<'_>], from: ObjectId, to: ObjectId) -> Vec<ObjectId> {
    let objects: Vec<ObjectId> = rows
        .iter()
        .filter_map(|row| match row {
            ObjectRow::Object { object, .. } => Some(object.id),
            ObjectRow::Layer { .. } => None,
        })
        .collect();
    let place = |id: ObjectId| objects.iter().position(|o| *o == id);
    match (place(from), place(to)) {
        (Some(a), Some(b)) => objects
            .get(a.min(b)..=a.max(b))
            .unwrap_or_default()
            .to_vec(),
        _ => vec![to],
    }
}

/// Where objects dropped at height `y` go: over an object's upper half just above it in its
/// layer's list, over its lower half just below it, over an object layer's row on top of its
/// objects. Nowhere over a group's row or outside the rows.
fn drop_place(rows: &Rows<'_>, y: f32) -> Option<ObjectDrop> {
    let place = rows
        .rects
        .iter()
        .position(|rect| rect.is_some_and(|rect| rect.y_range().contains(y)))?;
    let row = rows.rects.get(place).copied().flatten()?;
    let left = rows.lefts.get(place).copied().unwrap_or(row.left());
    match rows.shown.get(place)? {
        ObjectRow::Layer { layer, .. } => Some(ObjectDrop {
            layer: layer.id,
            index: layer.as_object_layer()?.objects.len(),
            line: None,
            left,
            row,
        }),
        ObjectRow::Object { layer, index, .. } => {
            let upper = y < row.center().y;
            Some(ObjectDrop {
                layer: layer.id,
                index: if upper { index + 1 } else { *index },
                line: Some(if upper { row.top() } else { row.bottom() }),
                left,
                row,
            })
        }
    }
}

/// Whether objects can go to `layer`: an unlocked object layer, and, for objects all from it,
/// one drawn in list order (a layer drawn top-down is not ordered by hand).
fn drop_allowed(map: &Map, objects: &[ObjectId], layer: LayerId) -> bool {
    let Some(target) = map.layer(layer).and_then(Layer::as_object_layer) else {
        return false;
    };
    let locked = map.effective_layer(layer).is_none_or(|state| state.locked);
    let within = objects
        .iter()
        .all(|id| target.objects.iter().any(|o| o.id == *id));
    let reorders_top_down = within && target.draw_order == DrawOrder::TopDown;
    !locked && !reorders_top_down
}

/// What the panel calls an object: its name, or its kind and id when it has none.
fn object_label(object: &MapObject) -> String {
    if object.name.is_empty() {
        format!("({} {})", object.kind_name(), object.id.0)
    } else {
        object.name.clone()
    }
}
