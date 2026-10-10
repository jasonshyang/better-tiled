//! Objects that are not tiles or text, drawn as Tiled draws them: a black shadow half a line
//! width down and right, the outline in the object's colour, and a fill of that colour at
//! alpha 50; points as pins, polygons and polylines with their first point marked. The colour is
//! the class's when it has one, else the layer's, else Tiled's grey.

use bt_core::{Color, LayerId, MapGeometry, MapObject, ObjectLayer, Shape};
use glam::DVec2;

use crate::input::ObjectStyle;
use crate::vector::VectorItem;

/// The alpha of shape fills.
const FILL_ALPHA: u8 = 50;

/// The colour an object is drawn in.
pub(crate) fn object_color(object: &MapObject, layer: &ObjectLayer, style: &ObjectStyle) -> Color {
    style
        .class_color(&object.class)
        .or(layer.color)
        .unwrap_or(Color::GRAY)
}

/// Appends the vector items of a shape object of layer `layer_id` (nothing for tile and text
/// objects).
pub(crate) fn shape_vectors(
    object: &MapObject,
    color: Color,
    geometry: &MapGeometry,
    style: &ObjectStyle,
    layer_id: LayerId,
    out: &mut Vec<VectorItem>,
) {
    if object.tile.is_some() || matches!(object.shape, Shape::Text(_)) {
        return;
    }
    let width = if style.width() > 0.0 {
        style.width()
    } else {
        1.0
    };
    let alpha = |a: u8| (f64::from(a) * object.opacity.clamp(0.0, 1.0)).round() as u8;
    let main = Color::rgba(color.r, color.g, color.b, alpha(color.a));
    let fill = Color::rgba(color.r, color.g, color.b, alpha(FILL_ALPHA));
    let shadow_color = Color::rgba(0, 0, 0, alpha(255));
    let shadow = DVec2::splat(f64::from(width) * 0.5);
    let origin = geometry.pixel_to_screen(object.pos);
    let rotation = object.rotation.to_radians();
    let place = |pixel: DVec2| rotate(geometry.pixel_to_screen(pixel), origin, rotation);
    let outline = Outline::of(object, origin, rotation, &place);
    let Some(outline) = outline else {
        return;
    };
    let item = |points: Vec<DVec2>| {
        if outline.closed {
            VectorItem::polygon(points)
        } else {
            VectorItem::polyline(points)
        }
    };
    out.push(
        item(outline.points.clone())
            .stroke(shadow_color, width)
            .device_offset(shadow)
            .on_layer(layer_id),
    );
    let mut drawn = item(outline.points.clone()).stroke(main, width);
    if outline.closed && outline.filled {
        drawn = drawn.fill(fill);
    }
    out.push(drawn.on_layer(layer_id));
    if outline.marks_first
        && let Some(&first) = outline.points.first()
    {
        let dot =
            |color: Color| VectorItem::polyline(vec![first, first]).stroke(color, width * 4.0);
        out.push(dot(shadow_color).device_offset(shadow).on_layer(layer_id));
        out.push(dot(main).on_layer(layer_id));
    }
}

/// An object's outline in world pixels.
struct Outline {
    points: Vec<DVec2>,
    closed: bool,
    filled: bool,
    marks_first: bool,
}

impl Outline {
    fn of(
        object: &MapObject,
        origin: DVec2,
        rotation: f64,
        place: &dyn Fn(DVec2) -> DVec2,
    ) -> Option<Outline> {
        let size = object.size;
        let degenerate = size.x == 0.0 || size.y == 0.0;
        // A shape without a size is drawn as a 20 pixel square about its position.
        let unsized_square = || {
            [
                DVec2::new(-10.0, -10.0),
                DVec2::new(10.0, -10.0),
                DVec2::new(10.0, 10.0),
                DVec2::new(-10.0, 10.0),
            ]
            .map(|corner| rotate(origin + corner, origin, rotation))
            .to_vec()
        };
        let closed = |points: Vec<DVec2>| Outline {
            points,
            closed: true,
            filled: true,
            marks_first: false,
        };
        Some(match &object.shape {
            Shape::Rectangle => {
                if size == DVec2::ZERO {
                    closed(unsized_square())
                } else {
                    closed(
                        rect_points(object.pos, size)
                            .into_iter()
                            .map(place)
                            .collect(),
                    )
                }
            }
            Shape::Ellipse | Shape::Capsule if degenerate && size != DVec2::ZERO => closed(
                rect_points(object.pos, size)
                    .into_iter()
                    .map(place)
                    .collect(),
            ),
            Shape::Ellipse => {
                if size == DVec2::ZERO {
                    closed(
                        ellipse_points(origin - DVec2::splat(10.0), DVec2::splat(20.0))
                            .into_iter()
                            .map(|p| rotate(p, origin, rotation))
                            .collect(),
                    )
                } else {
                    closed(
                        ellipse_points(object.pos, size)
                            .into_iter()
                            .map(place)
                            .collect(),
                    )
                }
            }
            Shape::Capsule => {
                if size == DVec2::ZERO {
                    closed(unsized_square())
                } else {
                    closed(
                        capsule_points(object.pos, size)
                            .into_iter()
                            .map(place)
                            .collect(),
                    )
                }
            }
            Shape::Polygon(points) => Outline {
                points: points.iter().map(|p| place(object.pos + *p)).collect(),
                closed: true,
                filled: true,
                marks_first: true,
            },
            Shape::Polyline(points) => Outline {
                points: points.iter().map(|p| place(object.pos + *p)).collect(),
                closed: false,
                filled: false,
                marks_first: true,
            },
            Shape::Point => closed(pin_points(origin)),
            Shape::Text(_) => return None,
        })
    }
}

/// Turns `p` clockwise on screen by `radians` about `pivot`.
pub(crate) fn rotate(p: DVec2, pivot: DVec2, radians: f64) -> DVec2 {
    if radians == 0.0 {
        return p;
    }
    let d = p - pivot;
    let (s, c) = radians.sin_cos();
    pivot + DVec2::new(d.x * c - d.y * s, d.x * s + d.y * c)
}

fn rect_points(min: DVec2, size: DVec2) -> Vec<DVec2> {
    vec![
        min,
        min + DVec2::new(size.x, 0.0),
        min + size,
        min + DVec2::new(0.0, size.y),
    ]
}

fn ellipse_points(min: DVec2, size: DVec2) -> Vec<DVec2> {
    let center = min + size * 0.5;
    let radius = size * 0.5;
    let count = 64;
    (0..count)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from(i) / f64::from(count);
            center + DVec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// A rectangle with its short sides rounded into half circles.
fn capsule_points(min: DVec2, size: DVec2) -> Vec<DVec2> {
    let radius = size.x.min(size.y) * 0.5;
    let steps = 16;
    let arc = |center: DVec2, from: f64| -> Vec<DVec2> {
        (0..=steps)
            .map(|i| {
                let angle = from + std::f64::consts::PI * f64::from(i) / f64::from(steps);
                center + DVec2::new(angle.cos(), angle.sin()) * radius
            })
            .collect()
    };
    let half = std::f64::consts::FRAC_PI_2;
    if size.x >= size.y {
        let left = min + DVec2::new(radius, radius);
        let right = min + DVec2::new(size.x - radius, radius);
        let mut points = arc(right, -half);
        points.extend(arc(left, half));
        points
    } else {
        let top = min + DVec2::new(radius, radius);
        let bottom = min + DVec2::new(radius, size.y - radius);
        let mut points = arc(bottom, 0.0);
        points.extend(arc(top, std::f64::consts::PI));
        points
    }
}

/// Tiled's point marker: a pin whose tip is at the point, ten pixels in radius.
fn pin_points(tip: DVec2) -> Vec<DVec2> {
    let radius = 10.0;
    let sweep = 235f64.to_radians();
    let start = 90f64.to_radians() - sweep / 2.0;
    let center = tip - DVec2::new(0.0, 2.0 * radius);
    let steps = 24;
    let mut points: Vec<DVec2> = (0..=steps)
        .map(|i| {
            // Counter-clockwise in Qt's convention (y up), from the start angle.
            let angle = start + sweep * f64::from(i) / f64::from(steps);
            center + DVec2::new(angle.cos(), -angle.sin()) * radius
        })
        .collect();
    points.push(tip);
    points
}

#[cfg(test)]
mod tests {
    use bt_core::{ObjectId, Orientation};
    use glam::IVec2;

    use super::*;

    #[test]
    fn shapes_get_a_shadow_an_outline_and_a_fill() {
        let geometry = MapGeometry::new(
            Orientation::Orthogonal,
            IVec2::new(16, 16),
            IVec2::new(4, 4),
        );
        let object = MapObject {
            id: ObjectId(1),
            pos: DVec2::new(10.0, 20.0),
            size: DVec2::new(30.0, 10.0),
            ..MapObject::default()
        };
        let style = ObjectStyle::default();
        let mut out = Vec::new();
        let color = Color::rgb(255, 0, 0);
        shape_vectors(&object, color, &geometry, &style, LayerId(3), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].stroke.unwrap().color, Color::rgba(0, 0, 0, 255));
        assert_eq!(out[0].device_offset, DVec2::splat(1.0));
        assert_eq!(out[1].fill, Some(Color::rgba(255, 0, 0, 50)));
        assert!(
            out.iter()
                .all(|i| i.placement() == crate::vector::Placement::OnLayer(LayerId(3)))
        );
        // Polylines mark their first point and have no fill.
        let line = MapObject {
            shape: Shape::Polyline(vec![DVec2::ZERO, DVec2::new(5.0, 5.0)]),
            ..object.clone()
        };
        out.clear();
        shape_vectors(&line, color, &geometry, &style, LayerId(3), &mut out);
        assert_eq!(out.len(), 4);
        assert!(out[1].fill.is_none());
        // Rotation turns the outline about the position.
        let turned = MapObject {
            rotation: 90.0,
            ..object
        };
        out.clear();
        shape_vectors(&turned, color, &geometry, &style, LayerId(3), &mut out);
        let crate::vector::VectorShape::Polygon(points) = &out[1].shape else {
            panic!("a polygon");
        };
        assert!(
            (points[1] - DVec2::new(10.0, 50.0)).length() < 1e-9,
            "{points:?}"
        );
    }

    #[test]
    fn pins_point_at_the_object() {
        let pin = pin_points(DVec2::new(5.0, 5.0));
        assert_eq!(*pin.last().unwrap(), DVec2::new(5.0, 5.0));
        assert!(pin.iter().all(|p| p.y <= 5.0 + 1e-9));
    }
}
