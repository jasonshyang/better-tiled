//! A document's canvas: its camera, the cell under the pointer, and the input that pans and
//! zooms (trackpad scroll pans and pinch zooms; the wheel zooms or scrolls as the setting says,
//! and zooms with ⌘ or Ctrl; middle-drag and Space-drag pan).

use std::sync::Arc;

use bt_core::{LayerId, Map, MapGeometry, ObjectId};
use bt_editor::{
    DocumentId, Editor, Intent, Settings, TileRegion, ViewRequest, ViewState, ViewToggles,
};
use bt_render::{FrameInput, LayerVisibility, ObjectStyle};
use bt_utils::{DRect, IRect};
use egui::{CursorIcon, Event, MouseWheelUnit, PointerButton, Pos2, Sense, Vec2};
use glam::{DVec2, IVec2, UVec2};

use super::camera::{Camera, ViewTransform};
use super::gesture::{GestureReader, Pointer, ReadFrame};
use super::overlay::{EditedOutline, MarkShape, OverlayInput, mark_shapes, overlay_vectors};
use super::paint::CanvasGpu;
use crate::tools::{Overlay, ToolBox, ToolContext, region_polygons};

/// Tiled's zoom steps within the zoom range: what Zoom In and Zoom Out and wheel notches step
/// through.
const ZOOM_STEPS: [f64; 20] = [
    1.0 / 32.0,
    1.0 / 16.0,
    1.0 / 8.0,
    1.0 / 4.0,
    1.0 / 3.0,
    1.0 / 2.0,
    3.0 / 4.0,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    5.5,
    8.0,
    11.0,
    16.0,
    23.0,
    32.0,
    45.0,
    64.0,
];

/// How far a wheel line scrolls, in points.
const LINE_POINTS: f32 = 48.0;

/// How much a trackpad scroll zooms per point while ⌘ or Ctrl is held.
const SMOOTH_ZOOM_PER_POINT: f64 = 0.01;

/// A document's view of its map.
#[derive(Debug, Default)]
pub(crate) struct Canvas {
    camera: Camera,
    framed: bool,
    hover: Option<Hover>,
    last_view: Option<ViewTransform>,
    pointer: Option<Pos2>,
    wheel_notches: f32,
    told: Option<ViewState>,
    gestures: GestureReader,
    /// What the current tool draws over the map.
    tool_overlay: Overlay,
    /// What the current tool says it does.
    hint: Option<String>,
    /// The selection's outline, kept while the selection and the grid stay the same.
    selection_outline: Option<SelectionOutline>,
    /// The selected objects' frames, kept while the selection and the map stay the same.
    objects_outline: Option<ObjectsOutline>,
    /// The marks `bt` made, kept while they and the grid stay the same.
    marks_outline: Option<MarksOutline>,
    /// The current layer's offset (its groups' included): the tools measure the pointer from it
    /// and the grid, the selection and the hovered cell are drawn at it, as in Tiled.
    layer_offset: DVec2,
    /// The area `bt`'s last edit changed and when it landed, outlined while it fades.
    bt_edit: Option<BtEdit>,
}

/// How long the outline of `bt`'s last edit takes to fade.
const BT_EDIT_FADES_IN: std::time::Duration = std::time::Duration::from_secs(3);

/// The area `bt`'s last edit changed, and when it landed.
#[derive(Clone, Copy, Debug)]
struct BtEdit {
    area: IRect,
    landed: std::time::Instant,
}

/// A selection's outline on screen, with what it was worked out from.
#[derive(Clone, Debug)]
struct SelectionOutline {
    region: TileRegion,
    geometry: MapGeometry,
    polygons: Vec<Vec<DVec2>>,
}

/// The marks' shapes on screen, with what they were worked out from.
#[derive(Clone, Debug)]
struct MarksOutline {
    marks: Vec<bt_editor::Mark>,
    geometry: MapGeometry,
    shapes: Vec<MarkShape>,
}

/// The selected objects' frames on screen, with what they were worked out from.
#[derive(Clone, Debug)]
struct ObjectsOutline {
    map: Arc<Map>,
    ids: Vec<ObjectId>,
    frames: Vec<Vec<DVec2>>,
}

/// What is under the pointer, on the current layer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Hover {
    /// The cell.
    pub(crate) cell: IVec2,
    /// The pixel (object coordinates).
    pub(crate) pixel: DVec2,
}

/// What a frame of a canvas shows.
pub(crate) struct CanvasFrame<'a> {
    /// The document.
    pub(crate) doc: DocumentId,
    /// Its map.
    pub(crate) map: &'a Arc<Map>,
    /// Where its camera looked when it was last open (from the session).
    pub(crate) saved_camera: Option<ViewState>,
    /// Its View switches.
    pub(crate) toggles: ViewToggles,
    /// The layers selected in the Layers panel.
    pub(crate) selected_layers: &'a [LayerId],
    /// The preferences.
    pub(crate) settings: &'a Settings,
    /// The renderer; `None` draws no map (the window's tests run without a GPU).
    pub(crate) gpu: Option<&'a CanvasGpu>,
    /// A text field has the keyboard, so Space types instead of panning.
    pub(crate) typing: bool,
    /// A dialog is open, so Escape and Enter are its.
    pub(crate) dialog_open: bool,
    /// The editor, which the tools read.
    pub(crate) editor: &'a Editor,
    /// What the canvas shows.
    pub(crate) role: CanvasRole,
}

/// What a canvas shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum CanvasRole {
    /// A document's map: its selected cells and the hovered cell are drawn, and the camera is
    /// remembered with the document.
    Map,
    /// The tileset editor's collision view of a tile: objects only, with a camera of its own.
    Collision,
}

impl Canvas {
    /// Shows the canvas in the rest of `ui`; camera moves and the current tool's edits are
    /// told to the editor in `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &CanvasFrame<'_>,
        tools: &mut ToolBox,
        out: &mut Vec<Intent>,
    ) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let pixels_per_point = ui.ctx().pixels_per_point();
        if !self.framed && rect.width() > 1.0 && rect.height() > 1.0 {
            self.camera = match frame.saved_camera {
                Some(view) => Camera::from_view_state(view),
                None => fitted(frame.map, rect.size()),
            };
            self.framed = true;
        }
        let before = self.camera.transform(rect, pixels_per_point);
        let pointer = ui
            .input(|input| input.pointer.hover_pos())
            .filter(|p| rect.contains(*p));
        if response.hovered()
            && let Some(at) = pointer
        {
            self.apply_wheel(ui, &before, at, frame.settings);
        }
        self.apply_drag(ui, &response, frame.typing);
        let dt = ui.input(|input| f64::from(input.stable_dt).min(0.1));
        if self.camera.advance(dt) {
            ui.ctx().request_repaint();
        }
        let view = self.camera.transform(rect, pixels_per_point);
        self.last_view = Some(view);
        self.pointer = pointer;
        let geometry = frame.map.geometry();
        self.layer_offset = current_layer_offset(frame);
        let layer_offset = self.layer_offset;
        self.hover = pointer
            .filter(|_| response.hovered() || response.dragged())
            .map(|at| {
                let local = view.screen_to_world(at) - layer_offset;
                Hover {
                    cell: geometry.tile_at(local),
                    pixel: geometry.screen_to_pixel(local),
                }
            });
        self.run_tool(ui, frame, &view, response.hovered(), tools, out);
        ui.painter()
            .rect_filled(view.rect, 0.0, ui.visuals().extreme_bg_color);
        if self.is_fading() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        if let Some(gpu) = frame.gpu {
            let time_ms = (ui.input(|input| input.time) * 1000.0) as u64;
            let input = self.frame_input(frame, &view, time_ms);
            let inputs = self.world_inputs(frame, &view, input, time_ms, out);
            ui.painter()
                .add(gpu.callback(view.rect, inputs, time_ms, ui.ctx().clone()));
        }
        let state = self.camera.view_state();
        if frame.role == CanvasRole::Map && self.framed && self.told != Some(state) {
            self.told = Some(state);
            out.push(Intent::SetCamera {
                doc: frame.doc,
                view: state,
            });
        }
    }

    /// Hands the frame's gestures to the current tool and keeps what it shows and says.
    fn run_tool(
        &mut self,
        ui: &egui::Ui,
        frame: &CanvasFrame<'_>,
        view: &ViewTransform,
        hovered: bool,
        tools: &mut ToolBox,
        out: &mut Vec<Intent>,
    ) {
        let geometry = frame.map.geometry();
        let layer_offset = self.layer_offset;
        let to_pointer = |screen: egui::Pos2| {
            let world = view.screen_to_world(screen);
            let local = world - layer_offset;
            Pointer {
                screen,
                world,
                pixel: geometry.screen_to_pixel(local),
                cell: geometry.tile_at(local),
            }
        };
        let space = !frame.typing && ui.input(|input| input.key_down(egui::Key::Space));
        let read = ReadFrame {
            rect: view.rect,
            hovered,
            panning: space || frame.typing,
            keys: !frame.typing && !frame.dialog_open,
            to_pointer: &to_pointer,
        };
        let gestures = ui.input(|input| self.gestures.read(input, &read));
        self.tool_overlay = Overlay::default();
        self.hint = None;
        let Some(tool) = tools.activate(frame.editor.tools().tool(), frame.doc, out) else {
            return;
        };
        let mut cx = ToolContext {
            editor: frame.editor,
            doc: frame.doc,
            map: frame.map,
            zoom: self.camera.zoom,
            layer_offset: self.layer_offset,
            out,
        };
        for gesture in &gestures {
            tool.handle(gesture, &mut cx);
        }
        tool.overlay(&cx, &mut self.tool_overlay);
        self.hint = Some(tool.hint(&cx));
    }

    /// Where the middle of the current layer's cell is on screen in the last frame (for the
    /// window's tests).
    #[cfg(test)]
    pub(crate) fn cell_on_screen(&self, map: &Map, cell: IVec2) -> Option<egui::Pos2> {
        let view = self.last_view?;
        let corners = map.geometry().tile_polygon(cell);
        let middle = corners.iter().copied().sum::<DVec2>() / corners.len().max(1) as f64;
        Some(view.world_to_screen(middle + self.layer_offset))
    }

    /// What the current tool says it does.
    pub(crate) fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The selection's outline, worked out again only when the selection or the grid changed.
    fn selection_polygons(&mut self, frame: &CanvasFrame<'_>) -> &[Vec<DVec2>] {
        let region = frame
            .editor
            .document(frame.doc)
            .map(|d| d.selection().tiles.clone())
            .unwrap_or_default();
        let geometry = frame.map.geometry();
        let fresh = self
            .selection_outline
            .as_ref()
            .is_some_and(|o| o.region == region && o.geometry == geometry);
        if !fresh {
            let polygons = region_polygons(&geometry, &region);
            self.selection_outline = Some(SelectionOutline {
                region,
                geometry,
                polygons,
            });
        }
        self.selection_outline
            .as_ref()
            .map_or(&[], |o| o.polygons.as_slice())
    }

    /// Outlines `area`, which an edit of `bt`'s just changed, until the outline fades.
    pub(crate) fn outline_bt_edit(&mut self, area: IRect) {
        self.bt_edit = Some(BtEdit {
            area,
            landed: std::time::Instant::now(),
        });
    }

    /// The outline of `bt`'s last edit and how strongly it is drawn, while it fades; forgets it
    /// once it has.
    fn bt_edit_outline(&mut self, geometry: &MapGeometry) -> Option<EditedOutline> {
        let edit = self.bt_edit?;
        let age = edit.landed.elapsed();
        if age >= BT_EDIT_FADES_IN {
            self.bt_edit = None;
            return None;
        }
        Some(EditedOutline {
            outline: geometry.rect_outline(edit.area),
            strength: 1.0 - age.as_secs_f32() / BT_EDIT_FADES_IN.as_secs_f32(),
        })
    }

    /// Whether the outline of `bt`'s last edit is still fading, so frames must keep coming.
    pub(crate) fn is_fading(&self) -> bool {
        self.bt_edit
            .is_some_and(|edit| edit.landed.elapsed() < BT_EDIT_FADES_IN)
    }

    /// The marks `bt` made on the document's map, worked out again only when they or the grid
    /// changed.
    fn mark_shapes(&mut self, frame: &CanvasFrame<'_>) -> &[MarkShape] {
        let marks = frame.editor.marks(frame.doc);
        let geometry = frame.map.geometry();
        let fresh = self
            .marks_outline
            .as_ref()
            .is_some_and(|o| o.marks == marks && o.geometry == geometry);
        if !fresh {
            self.marks_outline = Some(MarksOutline {
                marks: marks.to_vec(),
                shapes: mark_shapes(&geometry, marks),
                geometry,
            });
        }
        self.marks_outline
            .as_ref()
            .map_or(&[], |o| o.shapes.as_slice())
    }

    /// The selected objects' frames, worked out again only when the selection or the map
    /// changed.
    fn object_polygons(&mut self, frame: &CanvasFrame<'_>) -> &[Vec<DVec2>] {
        let ids = frame
            .editor
            .document(frame.doc)
            .map(|d| d.selection().objects.clone())
            .unwrap_or_default();
        let fresh = self
            .objects_outline
            .as_ref()
            .is_some_and(|o| Arc::ptr_eq(&o.map, frame.map) && o.ids == ids);
        if !fresh {
            let frames = frame
                .editor
                .object_frames(frame.doc, &ids)
                .into_iter()
                .map(|f| f.corners.to_vec())
                .collect();
            self.objects_outline = Some(ObjectsOutline {
                map: Arc::clone(frame.map),
                ids,
                frames,
            });
        }
        self.objects_outline
            .as_ref()
            .map_or(&[], |o| o.frames.as_slice())
    }

    /// Carries out a camera move the editor asked for, about the pointer when it is over the
    /// canvas and the centre otherwise.
    pub(crate) fn apply_view_request(&mut self, request: ViewRequest, map: &Map) {
        let Some(view) = self.last_view else {
            return;
        };
        let anchor = self.pointer.unwrap_or_else(|| view.rect.center());
        match request {
            ViewRequest::ZoomIn => self.step_zoom(1, anchor, &view),
            ViewRequest::ZoomOut => self.step_zoom(-1, anchor, &view),
            ViewRequest::ZoomNormal => self.camera.begin_zoom_to(1.0, anchor, &view),
            ViewRequest::Fit => self.camera = fitted(map, view.rect.size()),
            ViewRequest::CenterOnCell(cell) => {
                // The middle of the cell's image box, where Tiled's Go to Tile looks.
                let box_ = map
                    .geometry()
                    .cell_image_rect(cell, map.tile_size().as_dvec2());
                self.camera.center = (box_.min + box_.max) / 2.0;
                self.framed = true;
            }
            ViewRequest::CenterOn(point) => {
                self.camera.center = point;
                self.framed = true;
            }
        }
    }

    /// What is under the pointer.
    pub(crate) fn hover(&self) -> Option<Hover> {
        self.hover
    }

    /// The zoom in points per world pixel.
    pub(crate) fn zoom(&self) -> f64 {
        self.camera.zoom
    }

    /// The last frame's mapping between world pixels and screen points.
    pub(crate) fn last_view(&self) -> Option<ViewTransform> {
        self.last_view
    }

    /// Moves the camera at once (the bench).
    pub(crate) fn set_camera(&mut self, center: DVec2, zoom: f64) {
        self.camera.set_zoom(zoom);
        self.camera.center = center;
        self.framed = true;
    }

    fn frame_input(
        &mut self,
        frame: &CanvasFrame<'_>,
        view: &ViewTransform,
        time_ms: u64,
    ) -> FrameInput {
        let pixels_per_point = view.pixels_per_point as f32;
        let cells = frame.role == CanvasRole::Map;
        let selection = if cells {
            self.selection_polygons(frame).to_vec()
        } else {
            Vec::new()
        };
        let objects = self.object_polygons(frame).to_vec();
        let marks = if cells {
            self.mark_shapes(frame).to_vec()
        } else {
            Vec::new()
        };
        let edited = if cells {
            self.bt_edit_outline(&frame.map.geometry())
        } else {
            None
        };
        let mut overlays = overlay_vectors(&OverlayInput {
            map: frame.map,
            visible: view.render_camera().visible_rect(viewport_px(view)),
            zoom_px: view.zoom_px,
            pixels_per_point,
            grid: frame.toggles.grid,
            hover: self.hover.filter(|_| cells).map(|hover| hover.cell),
            settings: frame.settings,
            selection: &selection,
            layer_offset: self.layer_offset,
            objects: &objects,
            carrying: cells
                && frame
                    .editor
                    .document(frame.doc)
                    .is_some_and(|d| d.selection().carries_tiles()),
            tool: &self.tool_overlay.vectors,
            marks: &marks,
            edited: edited.as_ref(),
        });
        if let Some(origin) = frame.editor.world_origin(frame.doc) {
            let mut vectors = overlays.to_vec();
            let visible = view.render_camera().visible_rect(viewport_px(view));
            for entry in frame.editor.world_maps_in_view(frame.doc, visible) {
                let area = DRect::from_min_size(
                    entry.rect.min().as_dvec2() - origin,
                    entry.rect.size().as_dvec2(),
                );
                let current = Some(&entry.file.path) == frame.editor.path_of(frame.doc).as_ref();
                let color = if current {
                    bt_core::Color::rgba(80, 185, 240, 240)
                } else {
                    bt_core::Color::rgba(200, 200, 200, 160)
                };
                vectors.push(
                    bt_render::VectorItem::rect(area)
                        .stroke(color, if current { 2.0 } else { 1.0 }),
                );
                vectors.push(bt_render::VectorItem::text(
                    bt_render::TextItem::new(
                        bt_utils::paths::display_name(&entry.file.path),
                        area.min,
                    )
                    .screen_size(12.0 * pixels_per_point)
                    .color(color),
                ));
            }
            overlays = vectors.into();
        }
        let layers = if frame.toggles.highlight_current_layer {
            LayerVisibility::default().highlight(
                frame.selected_layers.to_vec(),
                frame.settings.interface.fade_color.0,
            )
        } else {
            LayerVisibility::default()
        };
        let objects = ObjectStyle::default()
            .custom_types(frame.editor.custom_types())
            .line_width(frame.settings.interface.object_line_width * pixels_per_point);
        let mut input = FrameInput::new(
            Arc::clone(frame.map),
            view.render_camera(),
            viewport_px(view),
        );
        if let Some(layer) = self.tool_overlay.preview_layer {
            input = input.previews(layer, Arc::from(self.tool_overlay.previews.clone()));
        }
        input
            .time_ms(time_ms)
            .layers(layers)
            .objects(objects)
            .vectors(overlays)
            .background(frame.map.background_color)
    }

    /// Neighbours use their own map geometry and camera, sharing the canvas viewport/device.
    fn world_inputs(
        &self,
        frame: &CanvasFrame<'_>,
        view: &ViewTransform,
        mut current: FrameInput,
        time_ms: u64,
        out: &mut Vec<Intent>,
    ) -> Vec<FrameInput> {
        if frame.role != CanvasRole::Map || frame.editor.template_of(frame.doc).is_some() {
            return vec![current];
        }
        let Some(origin) = frame.editor.world_origin(frame.doc) else {
            return vec![current];
        };
        let camera = view.render_camera();
        let viewport = viewport_px(view);
        let visible = camera.visible_rect(viewport);
        if frame.editor.needs_world_maps(frame.doc, visible) {
            out.push(Intent::LoadWorldMaps {
                doc: frame.doc,
                visible,
            });
        }
        let current_path = frame.editor.path_of(frame.doc);
        let mut inputs = Vec::new();
        for entry in frame.editor.world_maps_in_view(frame.doc, visible) {
            if Some(&entry.file.path) == current_path.as_ref() {
                continue;
            }
            let Some(asset) = frame.editor.store().by_path(&entry.file.path) else {
                continue;
            };
            let Some(map) = frame.editor.store().map(asset) else {
                continue;
            };
            let offset =
                entry.rect.min().as_dvec2() - origin - map.bounding_rect().min().as_dvec2();
            let bounds = DRect::from_min_size(
                map.bounding_rect().min().as_dvec2(),
                map.bounding_rect().size().as_dvec2(),
            );
            inputs.push(
                FrameInput::new(
                    Arc::clone(map),
                    bt_render::Camera {
                        origin: camera.origin - offset,
                        ..camera
                    },
                    viewport,
                )
                .time_ms(time_ms)
                .objects(ObjectStyle::default().custom_types(frame.editor.custom_types()))
                .background(map.background_color)
                .background_bounds(bounds),
            );
        }
        current = current.background_bounds(DRect::from_min_size(
            frame.map.bounding_rect().min().as_dvec2(),
            frame.map.bounding_rect().size().as_dvec2(),
        ));
        inputs.push(current);
        inputs
    }

    fn apply_wheel(&mut self, ui: &egui::Ui, view: &ViewTransform, at: Pos2, settings: &Settings) {
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                Event::Zoom(factor) => self.camera.apply_zoom_at(f64::from(factor), at, view),
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    let wheel = unit != MouseWheelUnit::Point;
                    if modifiers.command || (settings.behaviour.wheel_zooms && wheel) {
                        if wheel {
                            self.wheel_notches += delta.y;
                            let notches = self.wheel_notches.trunc();
                            self.wheel_notches -= notches;
                            self.step_zoom(notches as i32, at, view);
                        } else {
                            let factor = (f64::from(delta.y) * SMOOTH_ZOOM_PER_POINT).exp();
                            self.camera.apply_zoom_at(factor, at, view);
                        }
                    } else {
                        let points = match unit {
                            MouseWheelUnit::Point => delta,
                            MouseWheelUnit::Line => delta * LINE_POINTS,
                            MouseWheelUnit::Page => delta * view.rect.size(),
                        };
                        self.camera.apply_pan(points);
                    }
                }
                _ => {}
            }
        }
    }

    fn apply_drag(&mut self, ui: &egui::Ui, response: &egui::Response, typing: bool) {
        let space = !typing && ui.input(|input| input.key_down(egui::Key::Space));
        let panning = response.dragged_by(PointerButton::Middle)
            || (space && response.dragged_by(PointerButton::Primary));
        if panning {
            self.camera.apply_pan(response.drag_delta());
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if space && response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
    }

    /// Zooms `steps` of Tiled's zoom steps in (or out, when negative), animated about `at`.
    fn step_zoom(&mut self, steps: i32, at: Pos2, view: &ViewTransform) {
        let mut zoom = self.camera.target_zoom();
        for _ in 0..steps.unsigned_abs() {
            zoom = if steps > 0 {
                ZOOM_STEPS
                    .iter()
                    .copied()
                    .find(|step| *step > zoom * 1.001)
                    .unwrap_or(zoom)
            } else {
                ZOOM_STEPS
                    .iter()
                    .rev()
                    .copied()
                    .find(|step| *step < zoom / 1.001)
                    .unwrap_or(zoom)
            };
        }
        if zoom != self.camera.target_zoom() {
            self.camera.begin_zoom_to(zoom, at, view);
        }
    }
}

/// The offset of the document's current layer, its groups' included; none without one.
fn current_layer_offset(frame: &CanvasFrame<'_>) -> DVec2 {
    frame
        .editor
        .document(frame.doc)
        .and_then(|d| d.selection().current_layer)
        .and_then(|layer| frame.map.effective_layer(layer))
        .map_or(DVec2::ZERO, |state| state.offset)
}

/// A camera showing the whole map in a view of `size` points.
fn fitted(map: &Map, size: Vec2) -> Camera {
    let bounds = map.bounding_rect();
    let mut camera = Camera::default();
    camera.fit(bounds.min().as_dvec2(), bounds.max().as_dvec2(), size);
    camera
}

/// The view's size in device pixels, as egui-wgpu sets the callback's viewport.
fn viewport_px(view: &ViewTransform) -> UVec2 {
    let pixels = |from: f32, to: f32| {
        let ppp = view.pixels_per_point as f32;
        ((to * ppp).round() - (from * ppp).round()).max(0.0) as u32
    };
    UVec2::new(
        pixels(view.rect.min.x, view.rect.max.x),
        pixels(view.rect.min.y, view.rect.max.y),
    )
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use egui::{Rect, pos2};

    use super::*;

    #[test]
    fn zoom_steps_are_tileds_and_stay_in_range() {
        assert_eq!(ZOOM_STEPS.first(), Some(&super::super::MIN_ZOOM));
        assert_eq!(ZOOM_STEPS.last(), Some(&super::super::MAX_ZOOM));
        assert!(ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]));
        let mut canvas = Canvas::default();
        let view = canvas
            .camera
            .transform(Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(400.0)), 1.0);
        canvas.step_zoom(2, pos2(200.0, 200.0), &view);
        assert_eq!(canvas.camera.target_zoom(), 2.0);
        canvas.step_zoom(-30, pos2(200.0, 200.0), &view);
        assert_eq!(canvas.camera.target_zoom(), 1.0 / 32.0);
    }

    #[test]
    fn fitting_frames_the_map_and_viewports_match_egui() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(40, 20))
            .build()
            .unwrap();
        let camera = fitted(&map, Vec2::new(400.0, 400.0));
        assert_eq!(camera.center, DVec2::new(320.0, 160.0));
        let view = camera.transform(
            Rect::from_min_size(pos2(10.3, 20.0), Vec2::new(300.4, 200.0)),
            2.0,
        );
        let egui_px = egui::epaint::ViewportInPixels::from_points(&view.rect, 2.0, [4000, 4000]);
        let ours = viewport_px(&view);
        assert_eq!(
            [ours.x, ours.y],
            [egui_px.width_px as u32, egui_px.height_px as u32]
        );
    }
}
