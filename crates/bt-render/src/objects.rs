//! Objects that are not tiles or text, drawn as Tiled draws them: a black shadow half a line
//! width down and right, the outline in the object's colour, and a fill of that colour at
//! alpha 50; points as pins, polygons and polylines with their first point marked. The colour is
//! the class's when it has one, else the layer's, else Tiled's grey. Where a shape is drawn is
//! `bt_core`'s object outline, the one the editor picks objects by: shapes projected and turned
//! around their position, Tiled's marker for those without a size.

use bt_core::{
    Color, LayerId, MapGeometry, MapObject, ObjectLayer, Shape, object_marker, object_outline,
};
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
/// objects, nor for polygons and polylines without points).
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
    let points = match &object.shape {
        Shape::Point => pin_points(geometry.pixel_to_screen(object.pos)),
        Shape::Polygon(points) | Shape::Polyline(points) if points.is_empty() => return,
        _ => object_marker(geometry, object)
            .unwrap_or_else(|| object_outline(geometry, object, None))
            .into_points(),
    };
    let closed = !matches!(object.shape, Shape::Polyline(_));
    let marks_first = matches!(object.shape, Shape::Polygon(_) | Shape::Polyline(_));
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
    let item = |points: Vec<DVec2>| {
        if closed {
            VectorItem::polygon(points)
        } else {
            VectorItem::polyline(points)
        }
    };
    let first = points.first().copied();
    out.push(
        item(points.clone())
            .stroke(shadow_color, width)
            .device_offset(shadow)
            .on_layer(layer_id),
    );
    let mut drawn = item(points).stroke(main, width);
    if closed {
        drawn = drawn.fill(fill);
    }
    out.push(drawn.on_layer(layer_id));
    if marks_first && let Some(first) = first {
        let dot =
            |color: Color| VectorItem::polyline(vec![first, first]).stroke(color, width * 4.0);
        out.push(dot(shadow_color).device_offset(shadow).on_layer(layer_id));
        out.push(dot(main).on_layer(layer_id));
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

    #[test]
    fn shapes_without_a_size_are_drawn_as_tileds_marker() {
        let geometry = MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(4));
        let style = ObjectStyle::default();
        let drawn = |shape: Shape| {
            let object = MapObject {
                shape,
                pos: DVec2::new(50.0, 50.0),
                ..MapObject::default()
            };
            let mut out = Vec::new();
            shape_vectors(
                &object,
                Color::GRAY,
                &geometry,
                &style,
                LayerId(1),
                &mut out,
            );
            out.into_iter()
                .map(|item| match item.shape {
                    crate::vector::VectorShape::Polygon(points) => points,
                    other => panic!("{other:?}"),
                })
                .collect::<Vec<_>>()
        };
        let square = drawn(Shape::Rectangle);
        assert_eq!(square[1].len(), 4);
        assert_eq!(square[1][0], DVec2::new(40.0, 40.0));
        // Ellipses and capsules are circles of the same size, as Tiled draws them.
        for round in [Shape::Ellipse, Shape::Capsule] {
            let circle = drawn(round);
            assert_eq!(circle[1].len(), 64);
            assert!(
                circle[1]
                    .iter()
                    .all(|p| (p.distance(DVec2::new(50.0, 50.0)) - 10.0).abs() < 1e-9)
            );
        }
        assert!(
            drawn(Shape::Polygon(Vec::new())).is_empty(),
            "nothing to draw"
        );
    }
}
