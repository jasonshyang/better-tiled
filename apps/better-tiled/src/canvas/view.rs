//! A document's canvas: its camera, the cell under the pointer, and the input that pans and
//! zooms (trackpad scroll pans and pinch zooms; the wheel zooms or scrolls as the setting says,
//! and zooms with ⌘ or Ctrl; middle-drag and Space-drag pan).

use std::sync::Arc;

use bt_core::{LayerId, Map};
use bt_editor::{DocumentId, Intent, Settings, ViewRequest, ViewState, ViewToggles};
use bt_render::{FrameInput, LayerVisibility, ObjectStyle};
use egui::{CursorIcon, Event, MouseWheelUnit, PointerButton, Pos2, Sense, Vec2};
use glam::{DVec2, IVec2, UVec2};

use super::camera::{Camera, ViewTransform};
use super::overlay::{OverlayInput, overlay_vectors};
use super::paint::CanvasGpu;

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
}

/// What is under the pointer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Hover {
    /// The cell.
    pub(crate) cell: IVec2,
    /// The map pixel (object coordinates).
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
}

impl Canvas {
    /// Shows the canvas in the rest of `ui`; camera moves are told to the editor in `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &CanvasFrame<'_>,
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
        self.hover = pointer
            .filter(|_| response.hovered() || response.dragged())
            .map(|at| {
                let world = view.screen_to_world(at);
                Hover {
                    cell: geometry.tile_at(world),
                    pixel: geometry.screen_to_pixel(world),
                }
            });
        ui.painter()
            .rect_filled(view.rect, 0.0, ui.visuals().extreme_bg_color);
        if let Some(gpu) = frame.gpu {
            let time_ms = (ui.input(|input| input.time) * 1000.0) as u64;
            let input = self.frame_input(frame, &view, time_ms);
            ui.painter()
                .add(gpu.callback(view.rect, input, time_ms, ui.ctx().clone()));
        }
        let state = self.camera.view_state();
        if self.framed && self.told != Some(state) {
            self.told = Some(state);
            out.push(Intent::SetCamera {
                doc: frame.doc,
                view: state,
            });
        }
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
        &self,
        frame: &CanvasFrame<'_>,
        view: &ViewTransform,
        time_ms: u64,
    ) -> FrameInput {
        let pixels_per_point = view.pixels_per_point as f32;
        let overlays = overlay_vectors(&OverlayInput {
            map: frame.map,
            visible: view.render_camera().visible_rect(viewport_px(view)),
            zoom_px: view.zoom_px,
            pixels_per_point,
            grid: frame.toggles.grid,
            hover: self.hover.map(|hover| hover.cell),
            settings: frame.settings,
        });
        let layers = if frame.toggles.highlight_current_layer {
            LayerVisibility::default().highlight(
                frame.selected_layers.to_vec(),
                frame.settings.interface.fade_color.0,
            )
        } else {
            LayerVisibility::default()
        };
        let objects = ObjectStyle::default()
            .line_width(frame.settings.interface.object_line_width * pixels_per_point);
        FrameInput::new(
            Arc::clone(frame.map),
            view.render_camera(),
            viewport_px(view),
        )
        .time_ms(time_ms)
        .layers(layers)
        .objects(objects)
        .vectors(overlays)
        .background(frame.map.background_color)
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
