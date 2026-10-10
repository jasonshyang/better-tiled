//! An object's own coordinates, and where they are drawn. Libtiled's renderers draw a shape
//! (rectangle, ellipse, capsule, point, polygon, polyline) projected from pixel space onto the
//! screen, and a tile or text object upright at its projected position; libtiled's
//! `MiniMapRenderer` then turns the whole clockwise on screen around the projected position by
//! the object's rotation (`reference/tiled/src/libtiled/orthogonalrenderer.cpp`,
//! `isometricrenderer.cpp`, `minimaprenderer.cpp`).

use glam::DVec2;

use crate::geometry::MapGeometry;
use crate::object::{MapObject, Shape};
use crate::rotation::rotated;

/// Where an object's own coordinates are drawn. A shape's coordinates are pixels from its
/// position, the space its polygon points are in; a tile or text object's are screen pixels
/// from its projected position, since those stand upright on every grid. Either way the
/// coordinates turn clockwise on screen around the projected position by the object's
/// rotation, and the object's position is the origin of its coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ObjectFrame {
    geometry: MapGeometry,
    pos: DVec2,
    screen: DVec2,
    rotation: f64,
    upright: bool,
}

impl ObjectFrame {
    /// The frame of `object` on `geometry`'s grid: upright for tile and text objects, projected
    /// for shapes.
    pub fn of(geometry: &MapGeometry, object: &MapObject) -> ObjectFrame {
        let upright = object.tile.is_some() || matches!(object.shape, Shape::Text(_));
        ObjectFrame::placed(geometry, object, upright)
    }

    /// The upright frame of `object` whatever it is: where Tiled's markers for shapes without a
    /// size are drawn.
    pub(crate) fn upright_of(geometry: &MapGeometry, object: &MapObject) -> ObjectFrame {
        ObjectFrame::placed(geometry, object, true)
    }

    fn placed(geometry: &MapGeometry, object: &MapObject, upright: bool) -> ObjectFrame {
        ObjectFrame {
            geometry: *geometry,
            pos: object.pos,
            screen: geometry.pixel_to_screen(object.pos),
            rotation: object.rotation,
            upright,
        }
    }

    /// Whether the coordinates are screen pixels (tile and text objects) rather than pixels.
    pub fn is_upright(&self) -> bool {
        self.upright
    }

    /// Where `local` is drawn on screen.
    pub fn to_screen(&self, local: DVec2) -> DVec2 {
        if self.upright {
            self.screen + rotated(local, self.rotation)
        } else if self.rotation == 0.0 {
            self.geometry.pixel_to_screen(self.pos + local)
        } else {
            let projected = self.geometry.pixel_to_screen(self.pos + local) - self.screen;
            self.screen + rotated(projected, self.rotation)
        }
    }

    /// The pixel position drawn where `local` is: exact for unturned shapes and for the origin.
    pub fn to_pixels(&self, local: DVec2) -> DVec2 {
        if local == DVec2::ZERO {
            self.pos
        } else if self.is_plain() {
            self.pos + local
        } else {
            self.geometry.screen_to_pixel(self.to_screen(local))
        }
    }

    /// The coordinates drawn at the screen position `screen` (the inverse of
    /// [`ObjectFrame::to_screen`]).
    pub fn from_screen(&self, screen: DVec2) -> DVec2 {
        let unturned = rotated(screen - self.screen, -self.rotation);
        if self.upright {
            unturned
        } else {
            self.geometry.screen_to_pixel(self.screen + unturned) - self.pos
        }
    }

    /// The coordinates drawn where the pixel position `pixel` is drawn (the inverse of
    /// [`ObjectFrame::to_pixels`]): exact for unturned shapes.
    pub fn from_pixels(&self, pixel: DVec2) -> DVec2 {
        if self.is_plain() {
            pixel - self.pos
        } else {
            self.from_screen(self.geometry.pixel_to_screen(pixel))
        }
    }

    /// Whether the coordinates are pixels as they are: an unturned shape's.
    fn is_plain(&self) -> bool {
        !self.upright && self.rotation == 0.0
    }
}

#[cfg(test)]
mod tests {
    use bt_utils::rng::Rng;
    use glam::IVec2;

    use super::*;
    use crate::map::Orientation;
    use crate::text::TextData;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        (a - b).length() < 1e-9
    }

    fn object(pos: DVec2, rotation: f64, shape: Shape) -> MapObject {
        MapObject {
            pos,
            rotation,
            shape,
            ..MapObject::default()
        }
    }

    #[test]
    fn shapes_are_projected_and_upright_objects_are_not() {
        let iso = MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::splat(4));
        let shape = ObjectFrame::of(&iso, &object(v(32.0, 0.0), 0.0, Shape::Rectangle));
        assert!(!shape.is_upright());
        // A cell's side of pixels is drawn as its diamond's side.
        assert_eq!(
            shape.to_screen(v(32.0, 0.0)),
            iso.pixel_to_screen(v(64.0, 0.0))
        );
        assert_eq!(
            shape.to_pixels(v(32.0, 0.0)),
            v(64.0, 0.0),
            "exact unturned"
        );
        assert_eq!(shape.from_pixels(v(64.0, 32.0)), v(32.0, 32.0));
        let text = ObjectFrame::of(
            &iso,
            &object(v(32.0, 0.0), 0.0, Shape::Text(TextData::default())),
        );
        assert!(text.is_upright());
        assert_eq!(
            text.to_screen(v(10.0, 5.0)),
            iso.pixel_to_screen(v(32.0, 0.0)) + v(10.0, 5.0)
        );
    }

    #[test]
    fn frames_turn_around_the_projected_position() {
        let square = MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(4));
        let turned = ObjectFrame::of(&square, &object(v(10.0, 20.0), 90.0, Shape::Ellipse));
        assert_eq!(turned.to_screen(v(30.0, 0.0)), v(10.0, 50.0), "hangs down");
        assert_eq!(turned.to_pixels(DVec2::ZERO), v(10.0, 20.0));
        assert_eq!(turned.from_screen(v(10.0, 50.0)), v(30.0, 0.0));
    }

    #[test]
    fn frames_go_both_ways_on_every_grid() {
        let mut rng = Rng::new(0xF4A3E);
        let grids = [
            MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(8)),
            MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::splat(8)),
            MapGeometry {
                skew: IVec2::new(6, 2),
                ..MapGeometry::new(Orientation::Oblique, IVec2::splat(16), IVec2::splat(8))
            },
        ];
        for geometry in grids {
            for _ in 0..200 {
                let pos = v(
                    rng.next_f64() * 200.0 - 100.0,
                    rng.next_f64() * 200.0 - 100.0,
                );
                let rotation = rng.next_f64() * 720.0 - 360.0;
                let shape = if rng.below(2) == Some(0) {
                    Shape::Rectangle
                } else {
                    Shape::Text(TextData::default())
                };
                let frame = ObjectFrame::of(&geometry, &object(pos, rotation, shape));
                let local = v(rng.next_f64() * 80.0 - 40.0, rng.next_f64() * 80.0 - 40.0);
                assert!(close(frame.from_screen(frame.to_screen(local)), local));
                assert!(close(frame.from_pixels(frame.to_pixels(local)), local));
                assert!(close(
                    geometry.pixel_to_screen(frame.to_pixels(local)),
                    frame.to_screen(local)
                ));
            }
        }
    }
}
