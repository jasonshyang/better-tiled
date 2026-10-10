//! The Layers panel (Tiled's `LayerDock` and `LayerView`, `reference/tiled/src/tiled/layerdock.cpp`,
//! over its `LayerModel`, `layermodel.cpp`): the map's layers as a tree, topmost first, with
//! visibility and lock toggles and folding; renaming in place (double-click; Enter or clicking
//! away keeps the name, Escape drops it); selection, Shift-click for a range and ⌘-click to add or
//! take away a layer, the current layer always among the selected ones; dragging layers into
//! place, into and out of groups; a context menu of the layer commands; the opacity slider of
//! the selected layers (Tiled's before 1.4); and a strip of buttons below for a new layer,
//! raise, lower, duplicate and remove.

use std::collections::HashSet;

use bt_core::{Layer, LayerId, LayerKind, Map};
use bt_editor::{ActionId, DocumentId, Editor, Enabled, Intent, LayerLocation, Selection};
use egui::{Modifiers, Rect, Sense, Stroke};

use crate::menus::{MenuEntry, NEW_LAYER_MENU, menu_entries};
use MenuEntry::{Action, Separator, Submenu};

/// How far each level of the tree is indented, in points.
const INDENT: f32 = 14.0;

/// A layer's context menu (Tiled's `LayerView::contextMenuEvent`, with Layer via Copy and Cut).
const LAYER_MENU: &[MenuEntry] = &[
    Submenu("New", NEW_LAYER_MENU),
    Action(ActionId::GroupLayers),
    Action(ActionId::UngroupLayers),
    Action(ActionId::DuplicateLayers),
    Action(ActionId::MergeLayerDown),
    Action(ActionId::RemoveLayers),
    Separator,
    Action(ActionId::LayerViaCopy),
    Action(ActionId::LayerViaCut),
    Separator,
    Action(ActionId::SelectAllLayers),
    Action(ActionId::RaiseLayers),
    Action(ActionId::LowerLayers),
    Separator,
    Action(ActionId::ToggleSelectedLayers),
    Action(ActionId::ToggleLockSelectedLayers),
    Action(ActionId::ToggleOtherLayers),
    Action(ActionId::ToggleLockOtherLayers),
    Separator,
    Action(ActionId::LayerProperties),
];

/// The context menu of the space below the layers.
const SPACE_MENU: &[MenuEntry] = &[
    Submenu("New", NEW_LAYER_MENU),
    Separator,
    Action(ActionId::SelectAllLayers),
];

/// The buttons below the tree, after the New Layer menu, with their icons.
const BUTTONS: [(&str, ActionId); 4] = [
    ("⬆", ActionId::RaiseLayers),
    ("⬇", ActionId::LowerLayers),
    ("🗐", ActionId::DuplicateLayers),
    ("🗑", ActionId::RemoveLayers),
];

/// The panel's own state.
#[derive(Debug, Default)]
pub(crate) struct LayersPanel {
    renaming: Option<Renaming>,
    collapsed: HashSet<LayerKey>,
    /// Where a Shift-click's range starts: the layer clicked last without Shift.
    anchor: Option<LayerKey>,
    /// The layers being dragged.
    drag: Option<LayerDrag>,
    /// The opacity a drag of the slider set last; the drag ends the step when it lets go.
    opacity_drag: Option<f64>,
}

/// A layer whose name is being edited.
#[derive(Clone, Debug)]
struct Renaming {
    doc: DocumentId,
    layer: LayerId,
    name: String,
    focused: bool,
}

/// A layer of a document.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct LayerKey {
    doc: DocumentId,
    layer: LayerId,
}

/// Layers being dragged in a document.
#[derive(Clone, Debug)]
struct LayerDrag {
    doc: DocumentId,
    layers: Vec<LayerId>,
}

/// A row drawn this frame: the layer, where it is in the tree and on screen.
#[derive(Copy, Clone, Debug)]
struct Row {
    layer: LayerId,
    at: LayerLocation,
    /// The row, across the panel.
    rect: Rect,
    /// Where its name starts.
    name_left: f32,
    /// For a group, how many layers it holds.
    children: Option<usize>,
    /// Its layers are shown below it.
    open: bool,
}

/// What the rows of a frame share, and what they collect.
struct Rows<'a> {
    doc: DocumentId,
    editor: &'a Editor,
    selection: &'a Selection,
    rows: Vec<Row>,
    /// A click on a name, resolved once every row is drawn (a Shift-click needs them all).
    click: Option<NameClick>,
}

/// A click on a layer's name, and the modifiers held.
#[derive(Copy, Clone, Debug)]
struct NameClick {
    layer: LayerId,
    modifiers: Modifiers,
}

/// Where dropped layers would go, and how the panel shows it.
#[derive(Copy, Clone, Debug, PartialEq)]
enum DropPlace {
    /// Between rows: a line at `y` from `left`.
    Between {
        to: LayerLocation,
        y: f32,
        left: f32,
    },
    /// Into a group, on top of its layers: the group's row framed.
    Into { to: LayerLocation, rect: Rect },
}

impl DropPlace {
    fn to(self) -> LayerLocation {
        match self {
            DropPlace::Between { to, .. } | DropPlace::Into { to, .. } => to,
        }
    }
}

impl LayersPanel {
    /// Shows the layers of `doc`.
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
        let selection = document.selection();
        egui::Panel::bottom(egui::Id::new("layers-buttons"))
            .show_separator_line(true)
            .show(ui, |ui| show_buttons(ui, editor, out));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.show_opacity(ui, map, selection, doc, out);
            ui.separator();
            let mut rows = Rows {
                doc,
                editor,
                selection,
                rows: Vec::new(),
                click: None,
            };
            let scroll = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (index, layer) in map.layers.iter().enumerate().rev() {
                        let at = LayerLocation {
                            parent: None,
                            index,
                        };
                        self.show_layer(ui, layer, at, 0, &mut rows, out);
                    }
                    let space = ui.available_size().max(egui::vec2(0.0, 8.0));
                    ui.allocate_response(space, Sense::click())
                        .context_menu(|ui| menu_entries(ui, editor, SPACE_MENU, out));
                });
            if let Some(click) = rows.click {
                self.select_on_click(click, &rows, out);
            }
            self.drag_layers(ui, map, &rows, scroll.inner_rect, out);
        });
    }

    /// The selected layers' opacity: the slider shows the current layer's, and a drag of it is
    /// one step.
    fn show_opacity(
        &mut self,
        ui: &mut egui::Ui,
        map: &Map,
        selection: &Selection,
        doc: DocumentId,
        out: &mut Vec<Intent>,
    ) {
        let current = selection.current_layer.and_then(|id| map.layer(id));
        let opacity = current.map_or(1.0, |layer| layer.opacity);
        let mut percent = (opacity * 100.0).round();
        ui.horizontal(|ui| {
            let label = ui.label("Opacity");
            let slider = egui::Slider::new(&mut percent, 0.0..=100.0)
                .integer()
                .suffix(" %");
            let response = ui
                .add_enabled(current.is_some(), slider)
                .labelled_by(label.id);
            let held = response.is_pointer_button_down_on();
            let value = percent / 100.0;
            let set = |opacity: f64, done: bool| Intent::SetLayerOpacity {
                doc,
                layers: selection.layers.clone(),
                opacity,
                done,
            };
            if response.changed() && value != opacity {
                out.push(set(value, !held));
                self.opacity_drag = held.then_some(value);
            } else if !held && let Some(last) = self.opacity_drag.take() {
                // The drag let go: what it set is the step.
                out.push(set(last, true));
            }
        });
    }

    fn show_layer(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        at: LayerLocation,
        depth: usize,
        rows: &mut Rows<'_>,
        out: &mut Vec<Intent>,
    ) {
        let key = LayerKey {
            doc: rows.doc,
            layer: layer.id,
        };
        let children = match &layer.kind {
            LayerKind::Group(children) => Some(children.len()),
            LayerKind::Tile(_) | LayerKind::Object(_) | LayerKind::Image(_) => None,
        };
        let open = children.is_some() && !self.collapsed.contains(&key);
        let mut name_left = 0.0;
        let row = ui.horizontal(|ui| {
            if toggle(ui, layer.visible, ("👁", "Hide"), ("  ", "Show")) {
                out.push(Intent::SetLayerVisible {
                    doc: rows.doc,
                    layer: layer.id,
                    visible: !layer.visible,
                });
            }
            if toggle(ui, layer.locked, ("🔒", "Unlock"), ("  ", "Lock")) {
                out.push(Intent::SetLayerLocked {
                    doc: rows.doc,
                    layer: layer.id,
                    locked: !layer.locked,
                });
            }
            ui.add_space(INDENT * depth as f32);
            name_left = ui.cursor().left();
            if children.is_some() {
                let arrow = if open { "⏷" } else { "⏵" };
                if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                    if open {
                        self.collapsed.insert(key);
                    } else {
                        self.collapsed.remove(&key);
                    }
                }
            }
            self.show_name(ui, layer, rows, out);
        });
        rows.rows.push(Row {
            layer: layer.id,
            at,
            rect: row.response.rect,
            name_left,
            children,
            open,
        });
        if open {
            for (index, child) in layer.children().iter().enumerate().rev() {
                let at = LayerLocation {
                    parent: Some(layer.id),
                    index,
                };
                self.show_layer(ui, child, at, depth + 1, rows, out);
            }
        }
    }

    fn show_name(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        rows: &mut Rows<'_>,
        out: &mut Vec<Intent>,
    ) {
        if let Some(renaming) = &mut self.renaming
            && renaming.doc == rows.doc
            && renaming.layer == layer.id
        {
            let response = ui.add(
                egui::TextEdit::singleline(&mut renaming.name)
                    .desired_width(f32::INFINITY)
                    .id(egui::Id::new("rename-layer").with(layer.id.0)),
            );
            if !renaming.focused {
                response.request_focus();
                renaming.focused = true;
            }
            if response.lost_focus() {
                let cancelled = ui.input(|input| input.key_pressed(egui::Key::Escape));
                if !cancelled && renaming.name != layer.name {
                    out.push(Intent::RenameLayer {
                        doc: rows.doc,
                        layer: layer.id,
                        name: renaming.name.clone(),
                    });
                }
                self.renaming = None;
            }
            return;
        }
        let selected = rows.selection.layers.contains(&layer.id);
        let mut text = egui::RichText::new(display_name(layer));
        if rows.selection.current_layer == Some(layer.id) {
            text = text.strong();
        }
        if !layer.visible {
            text = text.weak();
        }
        let response = ui
            .add(egui::Button::selectable(selected, text))
            .interact(Sense::drag())
            .on_hover_text(layer.kind_name());
        if response.double_clicked() {
            self.renaming = Some(Renaming {
                doc: rows.doc,
                layer: layer.id,
                name: layer.name.clone(),
                focused: false,
            });
        } else if response.clicked() {
            rows.click = Some(NameClick {
                layer: layer.id,
                modifiers: ui.input(|input| input.modifiers),
            });
        }
        // As in Tiled, a right-click or a drag takes an unselected layer alone; a selected one
        // keeps the selection, which the menu then acts on or the drag carries.
        let takes_alone = !selected && (response.secondary_clicked() || response.drag_started());
        if takes_alone {
            out.push(Intent::SelectLayer {
                doc: rows.doc,
                layer: layer.id,
            });
        }
        if response.drag_started() {
            self.drag = Some(LayerDrag {
                doc: rows.doc,
                layers: if selected {
                    rows.selection.layers.clone()
                } else {
                    vec![layer.id]
                },
            });
        }
        response.context_menu(|ui| menu_entries(ui, rows.editor, LAYER_MENU, out));
    }

    /// A click on a name selects: the layer alone, a range from the anchor with Shift, or one
    /// layer more or less with ⌘ (the only selected layer stays selected).
    fn select_on_click(&mut self, click: NameClick, rows: &Rows<'_>, out: &mut Vec<Intent>) {
        let doc = rows.doc;
        let selected = &rows.selection.layers;
        if click.modifiers.shift {
            let anchor = self
                .anchor
                .filter(|anchor| anchor.doc == doc)
                .map(|anchor| anchor.layer)
                .or(rows.selection.current_layer)
                .unwrap_or(click.layer);
            out.push(Intent::SelectLayers {
                doc,
                current: click.layer,
                layers: rows_between(&rows.rows, anchor, click.layer),
            });
            return;
        }
        self.anchor = Some(LayerKey {
            doc,
            layer: click.layer,
        });
        if !click.modifiers.command {
            out.push(Intent::SelectLayer {
                doc,
                layer: click.layer,
            });
        } else if !selected.contains(&click.layer) {
            let mut layers = selected.clone();
            layers.push(click.layer);
            out.push(Intent::SelectLayers {
                doc,
                current: click.layer,
                layers,
            });
        } else if selected.len() > 1 {
            let layers: Vec<LayerId> = selected
                .iter()
                .copied()
                .filter(|id| *id != click.layer)
                .collect();
            let current = match rows.selection.current_layer {
                Some(current) if current != click.layer => Some(current),
                _ => nearest_selected(&rows.rows, click.layer, &layers),
            };
            if let Some(current) = current {
                out.push(Intent::SelectLayers {
                    doc,
                    current,
                    layers,
                });
            }
        }
    }

    /// Follows a drag of layers: shows where they would go, and moves them there when it lets
    /// go over the tree (anywhere else, nothing happens).
    fn drag_layers(
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
            .and_then(|at| drop_place(&rows.rows, map.layers.len(), at.y))
            .filter(|drop| drop_allowed(map, &drag.layers, drop.to()));
        let painter = ui.painter().with_clip_rect(area);
        let stroke = Stroke::new(2.0, ui.visuals().selection.stroke.color);
        match drop {
            Some(DropPlace::Between { y, left, .. }) => {
                painter.hline(left..=area.right(), y, stroke);
            }
            Some(DropPlace::Into { rect, .. }) => {
                let frame = Rect::from_x_y_ranges(area.x_range(), rect.y_range());
                painter.rect_stroke(frame, 2.0, stroke, egui::StrokeKind::Inside);
            }
            None => {}
        }
        if !ui.input(|input| input.pointer.primary_down()) {
            if let Some(drop) = drop {
                out.push(Intent::MoveLayers {
                    doc: rows.doc,
                    layers: drag.layers.clone(),
                    to: drop.to(),
                });
            }
            self.drag = None;
        }
    }
}

/// A frameless toggle showing `on` or `off` (its icon and what clicking it does); whether it
/// was clicked.
fn toggle(ui: &mut egui::Ui, state: bool, on: (&str, &str), off: (&str, &str)) -> bool {
    let (icon, hint) = if state { on } else { off };
    ui.add(
        egui::Button::new(icon)
            .frame(false)
            .min_size(egui::vec2(18.0, 0.0)),
    )
    .on_hover_text(hint)
    .clicked()
}

/// The strip below the tree: New Layer's menu, then raise, lower, duplicate and remove, each
/// greyed out with its reason when it cannot run.
fn show_buttons(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    ui.horizontal(|ui| {
        ui.menu_button("✚", |ui| menu_entries(ui, editor, NEW_LAYER_MENU, out))
            .response
            .on_hover_text("New Layer");
        for (icon, id) in BUTTONS {
            let hint = editor.keymap().with_key(id);
            let response = match editor.enabled(id) {
                Enabled::Enabled => ui.button(icon).on_hover_text(hint),
                Enabled::Disabled(reason) => ui
                    .add_enabled(false, egui::Button::new(icon))
                    .on_disabled_hover_text(reason.unwrap_or(hint)),
            };
            if response.clicked() {
                out.push(Intent::Run(id));
            }
        }
    });
}

/// The layers of the rows from `from` to `to`, both included, in the order shown; `to` alone
/// when `from` is not shown.
fn rows_between(rows: &[Row], from: LayerId, to: LayerId) -> Vec<LayerId> {
    let place = |layer: LayerId| rows.iter().position(|row| row.layer == layer);
    match (place(from), place(to)) {
        (Some(a), Some(b)) => rows
            .get(a.min(b)..=a.max(b))
            .unwrap_or_default()
            .iter()
            .map(|row| row.layer)
            .collect(),
        _ => vec![to],
    }
}

/// The layer of `layers` shown nearest below `from` (else above it), or the first of them when
/// none is shown.
fn nearest_selected(rows: &[Row], from: LayerId, layers: &[LayerId]) -> Option<LayerId> {
    let shown = |row: &&Row| layers.contains(&row.layer);
    let at = rows.iter().position(|row| row.layer == from).unwrap_or(0);
    let below = rows.iter().skip(at).find(shown);
    let above = rows.iter().take(at).rev().find(shown);
    below
        .or(above)
        .map(|row| row.layer)
        .or_else(|| layers.first().copied())
}

/// Where layers dropped at height `y` go. Over a row's upper half they go above its layer and
/// over its lower half below it; over the middle of a group's row (or the lower part of an open
/// one, whose layers are shown below it) they go into the group, on top of its layers. Above the
/// rows they go on top of the map, below them to its bottom. `None` without rows.
fn drop_place(rows: &[Row], top_level: usize, y: f32) -> Option<DropPlace> {
    let first = rows.first()?;
    if y < first.rect.top() {
        return Some(DropPlace::Between {
            to: LayerLocation {
                parent: None,
                index: top_level,
            },
            y: first.rect.top(),
            left: first.name_left,
        });
    }
    let next_tops = rows.iter().skip(1).map(|row| row.rect.top());
    let found = rows
        .iter()
        .zip(next_tops.map(Some).chain([None]))
        .find(|(row, next)| y < next.unwrap_or(row.rect.bottom()));
    let Some((row, _)) = found else {
        let last = rows.last()?;
        let left = rows
            .iter()
            .map(|row| row.name_left)
            .fold(f32::INFINITY, f32::min);
        return Some(DropPlace::Between {
            to: LayerLocation {
                parent: None,
                index: 0,
            },
            y: last.rect.bottom(),
            left,
        });
    };
    let along = ((y - row.rect.top()) / row.rect.height().max(1.0)).clamp(0.0, 1.0);
    let above = DropPlace::Between {
        to: LayerLocation {
            parent: row.at.parent,
            index: row.at.index + 1,
        },
        y: row.rect.top(),
        left: row.name_left,
    };
    let below = DropPlace::Between {
        to: row.at,
        y: row.rect.bottom(),
        left: row.name_left,
    };
    Some(match row.children {
        Some(children) if along >= 0.25 && (along <= 0.75 || row.open) => DropPlace::Into {
            to: LayerLocation {
                parent: Some(row.layer),
                index: children,
            },
            rect: row.rect,
        },
        Some(_) if along < 0.25 => above,
        Some(_) => below,
        None if along < 0.5 => above,
        None => below,
    })
}

/// Whether layers can go to `to`: a group never goes into itself or a group inside it.
fn drop_allowed(map: &Map, layers: &[LayerId], to: LayerLocation) -> bool {
    match to.parent {
        None => true,
        Some(parent) => map
            .layer_path(parent)
            .is_some_and(|path| !path.iter().any(|outer| layers.contains(outer))),
    }
}

/// A layer's name, or what it is when it has none.
pub(crate) fn display_name(layer: &Layer) -> String {
    if layer.name.is_empty() {
        format!("({})", layer.kind_name())
    } else {
        layer.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use egui::pos2;

    use super::*;

    /// A row 20 points high from `top`.
    fn row(
        layer: u32,
        parent: Option<u32>,
        index: usize,
        top: f32,
        children: Option<usize>,
    ) -> Row {
        Row {
            layer: LayerId(layer),
            at: LayerLocation {
                parent: parent.map(LayerId),
                index,
            },
            rect: Rect::from_min_max(pos2(0.0, top), pos2(200.0, top + 20.0)),
            name_left: 40.0,
            children,
            open: children.is_some(),
        }
    }

    #[test]
    fn drops_go_above_below_or_into_the_row_under_the_pointer() {
        // sky (top), walls[things, floor] open, ground (bottom).
        let rows = [
            row(4, None, 2, 0.0, None),
            row(5, None, 1, 20.0, Some(2)),
            row(3, Some(5), 1, 40.0, None),
            row(2, Some(5), 0, 60.0, None),
            row(1, None, 0, 80.0, None),
        ];
        let to = |y: f32| drop_place(&rows, 3, y).map(DropPlace::to);
        let at = |parent: Option<u32>, index: usize| {
            Some(LayerLocation {
                parent: parent.map(LayerId),
                index,
            })
        };
        assert_eq!(to(-5.0), at(None, 3), "above everything: on top");
        assert_eq!(to(4.0), at(None, 3), "above the sky");
        assert_eq!(to(16.0), at(None, 2), "below the sky");
        assert_eq!(to(22.0), at(None, 2), "above the walls");
        assert_eq!(to(30.0), at(Some(5), 2), "into the walls, on top");
        assert_eq!(
            to(38.0),
            at(Some(5), 2),
            "the lower part of an open group too"
        );
        assert_eq!(
            to(75.0),
            at(Some(5), 0),
            "below the floor: the bottom of the walls"
        );
        assert_eq!(to(95.0), at(None, 0), "below the ground");
        assert_eq!(to(200.0), at(None, 0), "below everything: at the bottom");
        assert_eq!(drop_place(&[], 0, 10.0), None);
    }

    #[test]
    fn ranges_and_neighbours_follow_the_rows_shown() {
        let rows = [
            row(4, None, 2, 0.0, None),
            row(3, None, 1, 20.0, None),
            row(1, None, 0, 40.0, None),
        ];
        assert_eq!(
            rows_between(&rows, LayerId(1), LayerId(4)),
            [4, 3, 1].map(LayerId)
        );
        assert_eq!(rows_between(&rows, LayerId(9), LayerId(3)), [LayerId(3)]);
        assert_eq!(
            nearest_selected(&rows, LayerId(3), &[LayerId(4), LayerId(1)]),
            Some(LayerId(1)),
            "below first"
        );
        assert_eq!(
            nearest_selected(&rows, LayerId(1), &[LayerId(4)]),
            Some(LayerId(4))
        );
    }
}
