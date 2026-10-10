//! A document's camera: the world point at the centre of the view and the zoom, with zooms
//! that animate around the pointer, and the frame's mapping between world pixels and screen
//! points, snapped so world pixel edges land on device pixel edges as the GPU draws them.

use bt_editor::ViewState;
use egui::{Pos2, Rect, Vec2, pos2};
use glam::DVec2;

/// The smallest zoom.
pub(crate) const MIN_ZOOM: f64 = 1.0 / 32.0;
/// The largest zoom.
pub(crate) const MAX_ZOOM: f64 = 64.0;

/// The time constant of an animated zoom, in seconds: short enough to feel instant, long
/// enough for the eye to follow.
const ZOOM_TAU: f64 = 0.055;

/// How much of the view a fitted map fills.
const FIT_MARGIN: f64 = 0.9;

/// The camera of a document's view.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Camera {
    /// The world point at the centre of the view.
    pub(crate) center: DVec2,
    /// Screen points per world pixel (1 is 100 %).
    pub(crate) zoom: f64,
    target_zoom: f64,
    anchor: Option<ZoomAnchor>,
}

/// What stays put while an animated zoom runs: the screen offset from the view's centre and
/// the world point shown there.
#[derive(Copy, Clone, Debug, PartialEq)]
struct ZoomAnchor {
    offset: DVec2,
    world: DVec2,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera {
            center: DVec2::ZERO,
            zoom: 1.0,
            target_zoom: 1.0,
            anchor: None,
        }
    }
}

/// The mapping between world pixels and screen points for one frame.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ViewTransform {
    /// The view's rectangle, snapped to device pixels.
    pub(crate) rect: Rect,
    /// Device pixels per screen point.
    pub(crate) pixels_per_point: f64,
    /// The world point at `rect.min`.
    pub(crate) origin: DVec2,
    /// Device pixels per world pixel.
    pub(crate) zoom_px: f64,
}

impl ViewTransform {
    /// The world point at a screen point.
    pub(crate) fn screen_to_world(&self, screen: Pos2) -> DVec2 {
        let offset = DVec2::new(
            f64::from(screen.x - self.rect.min.x),
            f64::from(screen.y - self.rect.min.y),
        );
        self.origin + offset * self.pixels_per_point / self.zoom_px
    }

    /// The renderer's camera for this view.
    pub(crate) fn render_camera(&self) -> bt_render::Camera {
        bt_render::Camera {
            origin: self.origin,
            zoom: self.zoom_px,
        }
    }
}

/// A rectangle of screen points snapped to whole device pixels.
pub(crate) fn snap_rect(rect: Rect, pixels_per_point: f32) -> Rect {
    let snap = |v: f32| (v * pixels_per_point).round() / pixels_per_point;
    Rect::from_min_max(
        pos2(snap(rect.min.x), snap(rect.min.y)),
        pos2(snap(rect.max.x), snap(rect.max.y)),
    )
}

impl Camera {
    /// A camera where a saved view looked.
    pub(crate) fn from_view_state(view: ViewState) -> Camera {
        let zoom = view.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        Camera {
            center: view.center,
            zoom,
            target_zoom: zoom,
            anchor: None,
        }
    }

    /// Where the camera looks, to keep in the session (an animation's end, not its middle).
    pub(crate) fn view_state(&self) -> ViewState {
        let center = match self.anchor {
            Some(anchor) => anchor.world - anchor.offset / self.target_zoom,
            None => self.center,
        };
        ViewState {
            center,
            zoom: self.target_zoom,
        }
    }

    /// This frame's mapping for a view filling `rect`.
    pub(crate) fn transform(&self, rect: Rect, pixels_per_point: f32) -> ViewTransform {
        let rect = snap_rect(rect, pixels_per_point);
        let pixels_per_point = f64::from(pixels_per_point);
        let zoom_px = self.zoom * pixels_per_point;
        let half = DVec2::new(f64::from(rect.width()), f64::from(rect.height())) * 0.5 / self.zoom;
        let origin = ((self.center - half) * zoom_px).round() / zoom_px;
        ViewTransform {
            rect,
            pixels_per_point,
            origin,
            zoom_px,
        }
    }

    /// Moves the view by a screen delta in points; the map follows the delta.
    pub(crate) fn apply_pan(&mut self, delta: Vec2) {
        let delta = DVec2::new(f64::from(delta.x), f64::from(delta.y)) / self.zoom;
        self.center -= delta;
        if let Some(anchor) = &mut self.anchor {
            anchor.world -= delta;
        }
    }

    /// Zooms by `factor` at once, keeping the world point under `screen` where it is (pinches).
    pub(crate) fn apply_zoom_at(&mut self, factor: f64, screen: Pos2, view: &ViewTransform) {
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let offset = offset_from_center(screen, view);
        let world = self.center + offset / self.zoom;
        self.zoom = zoom;
        self.center = world - offset / self.zoom;
        self.target_zoom = zoom;
        self.anchor = None;
    }

    /// Starts an animated zoom by `factor` around `screen` (wheel notches and keys).
    pub(crate) fn begin_zoom_animated(&mut self, factor: f64, screen: Pos2, view: &ViewTransform) {
        let offset = offset_from_center(screen, view);
        let world = self.center + offset / self.zoom;
        self.target_zoom = (self.target_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.anchor = Some(ZoomAnchor { offset, world });
    }

    /// Starts an animated zoom to `zoom` around `screen`.
    pub(crate) fn begin_zoom_to(&mut self, zoom: f64, screen: Pos2, view: &ViewTransform) {
        self.begin_zoom_animated(zoom / self.target_zoom, screen, view);
    }

    /// Advances an animated zoom by `dt` seconds; whether it is still running.
    pub(crate) fn advance(&mut self, dt: f64) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let t = 1.0 - (-dt / ZOOM_TAU).exp();
        let zoom = (self.zoom.ln() + (self.target_zoom.ln() - self.zoom.ln()) * t).exp();
        let done = (zoom / self.target_zoom - 1.0).abs() < 1e-3;
        self.zoom = if done { self.target_zoom } else { zoom };
        self.center = anchor.world - anchor.offset / self.zoom;
        if done {
            self.anchor = None;
        }
        !done
    }

    /// The zoom an animation is heading for (the zoom when none runs).
    pub(crate) fn target_zoom(&self) -> f64 {
        self.target_zoom
    }

    /// Sets the zoom at once, about the view's centre.
    pub(crate) fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.target_zoom = self.zoom;
        self.anchor = None;
    }

    /// Centres the world rectangle `min..max` in a view of `view_size` points, as large as fits.
    pub(crate) fn fit(&mut self, min: DVec2, max: DVec2, view_size: Vec2) {
        let size = max - min;
        let zoom_x = f64::from(view_size.x) * FIT_MARGIN / size.x.max(1.0);
        let zoom_y = f64::from(view_size.y) * FIT_MARGIN / size.y.max(1.0);
        self.set_zoom(zoom_x.min(zoom_y));
        self.center = (min + max) * 0.5;
    }
}

fn offset_from_center(screen: Pos2, view: &ViewTransform) -> DVec2 {
    let center = view.rect.center();
    DVec2::new(
        f64::from(screen.x - center.x),
        f64::from(screen.y - center.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_keeps_the_point_under_the_pointer() {
        let mut camera = Camera {
            center: DVec2::new(100.0, 50.0),
            ..Camera::default()
        };
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(810.0, 620.0));
        let view = camera.transform(rect, 2.0);
        let screen = pos2(300.0, 200.0);
        let before = view.screen_to_world(screen);
        camera.apply_zoom_at(2.5, screen, &view);
        let after = camera.transform(rect, 2.0).screen_to_world(screen);
        assert!((before - after).length() < 0.5, "{before} vs {after}");
    }

    #[test]
    fn transforms_round_trip_and_snap_to_device_pixels() {
        let camera = Camera {
            center: DVec2::new(33.3, 17.7),
            zoom: 1.5,
            ..Camera::default()
        };
        let rect = Rect::from_min_size(pos2(0.5, 0.5), Vec2::new(640.0, 480.0));
        let view = camera.transform(rect, 2.0);
        assert_eq!((view.origin * view.zoom_px).fract(), DVec2::ZERO);
        let corner = view.screen_to_world(view.rect.min);
        assert_eq!(corner, view.origin);
        let across = view.screen_to_world(view.rect.min + Vec2::new(30.0, 15.0));
        assert!((across - view.origin - DVec2::new(20.0, 10.0)).length() < 1e-9);
        assert_eq!(view.render_camera().zoom, 3.0);
    }

    #[test]
    fn animated_zooms_converge_and_save_their_end() {
        let mut camera = Camera::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(500.0));
        let view = camera.transform(rect, 1.0);
        camera.begin_zoom_animated(2.0, pos2(100.0, 100.0), &view);
        let saved = camera.view_state();
        let mut frames = 0;
        while camera.advance(1.0 / 120.0) {
            frames += 1;
            assert!(frames < 120);
        }
        assert_eq!(camera.zoom, 2.0);
        assert!((camera.center - saved.center).length() < 1e-9);
        assert_eq!(saved.zoom, 2.0);
    }

    #[test]
    fn zooms_stay_in_range_and_fit_centres() {
        let mut camera = Camera::default();
        camera.set_zoom(1000.0);
        assert_eq!(camera.zoom, MAX_ZOOM);
        camera.fit(
            DVec2::ZERO,
            DVec2::new(1000.0, 500.0),
            Vec2::new(500.0, 500.0),
        );
        assert_eq!(camera.center, DVec2::new(500.0, 250.0));
        assert!((camera.zoom - 0.45).abs() < 1e-9);
        let restored = Camera::from_view_state(ViewState {
            center: DVec2::new(1.0, 2.0),
            zoom: 0.0,
        });
        assert_eq!(restored.zoom, MIN_ZOOM);
    }
}
