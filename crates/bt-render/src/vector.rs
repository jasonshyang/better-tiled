//! Everything drawn that is not an image: polylines, polygons, rectangles, ellipses and text,
//! with a stroke, a fill, dashes, and the layer they sit on or above.
//!
//! A stroke's width is cosmetic (device pixels, whatever the zoom) unless asked otherwise, and so
//! are dashes and offsets: the tessellated vertices carry a world position plus a device-pixel
//! extrusion, so a zoom changes nothing on the CPU. Strokes are anti-aliased across their width;
//! fills are not.

use bt_core::{Color, LayerId};
use bt_utils::DRect;
use glam::DVec2;

use crate::text::TextItem;

/// What a [`VectorItem`] draws.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorShape {
    /// Connected segments.
    Polyline(Vec<DVec2>),
    /// A closed outline, filled with the even-odd rule.
    Polygon(Vec<DVec2>),
    /// A rectangle.
    Rect(DRect),
    /// The ellipse inside a rectangle.
    Ellipse(DRect),
    /// Laid-out text.
    Text(TextItem),
}

/// Where an item is drawn among the layers.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// After every layer.
    #[default]
    AboveAll,
    /// Right after a layer, in world coordinates (its offset does not apply).
    AboveLayer(LayerId),
    /// As part of a layer: its offset, parallax and opacity apply.
    OnLayer(LayerId),
}

/// A dash pattern in device pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Dash {
    /// Pixels drawn.
    pub on: f32,
    /// Pixels left out.
    pub off: f32,
    /// Where the pattern starts (marching ants advance it).
    pub phase: f32,
}

/// How an outline is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Stroke {
    /// Its colour.
    pub color: Color,
    /// Its width: device pixels when cosmetic, world pixels otherwise.
    pub width: f32,
    /// Whether the width is in device pixels.
    pub cosmetic: bool,
    /// Its dashes, if any.
    pub dash: Option<Dash>,
}

/// One thing to draw that is not an image.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorItem {
    pub(crate) shape: VectorShape,
    pub(crate) stroke: Option<Stroke>,
    pub(crate) fill: Option<Color>,
    pub(crate) placement: Placement,
    pub(crate) device_offset: DVec2,
}

impl VectorItem {
    fn of(shape: VectorShape) -> VectorItem {
        VectorItem {
            shape,
            stroke: None,
            fill: None,
            placement: Placement::AboveAll,
            device_offset: DVec2::ZERO,
        }
    }

    /// Connected segments through `points` (world pixels).
    pub fn polyline(points: Vec<DVec2>) -> VectorItem {
        VectorItem::of(VectorShape::Polyline(points))
    }

    /// A closed outline through `points`.
    pub fn polygon(points: Vec<DVec2>) -> VectorItem {
        VectorItem::of(VectorShape::Polygon(points))
    }

    /// A rectangle.
    pub fn rect(rect: DRect) -> VectorItem {
        VectorItem::of(VectorShape::Rect(rect))
    }

    /// The ellipse inside `rect`.
    pub fn ellipse(rect: DRect) -> VectorItem {
        VectorItem::of(VectorShape::Ellipse(rect))
    }

    /// Text.
    pub fn text(text: TextItem) -> VectorItem {
        VectorItem::of(VectorShape::Text(text))
    }

    /// An outline `width` device pixels wide.
    pub fn stroke(mut self, color: Color, width: f32) -> VectorItem {
        self.stroke = Some(Stroke {
            color,
            width,
            cosmetic: true,
            dash: None,
        });
        self
    }

    /// An outline `width` world pixels wide, which grows with the zoom.
    pub fn world_stroke(mut self, color: Color, width: f32) -> VectorItem {
        self.stroke = Some(Stroke {
            color,
            width,
            cosmetic: false,
            dash: None,
        });
        self
    }

    /// Dashes on the outline set before.
    pub fn dashed(mut self, dash: Dash) -> VectorItem {
        if let Some(stroke) = &mut self.stroke {
            stroke.dash = Some(dash);
        }
        self
    }

    /// A fill (polygons, rectangles and ellipses).
    pub fn fill(mut self, color: Color) -> VectorItem {
        self.fill = Some(color);
        self
    }

    /// Draws the item right after `layer`.
    pub fn above_layer(mut self, layer: LayerId) -> VectorItem {
        self.placement = Placement::AboveLayer(layer);
        self
    }

    /// Draws the item as part of `layer`.
    pub fn on_layer(mut self, layer: LayerId) -> VectorItem {
        self.placement = Placement::OnLayer(layer);
        self
    }

    /// Moves the item by device pixels (a shadow).
    pub fn device_offset(mut self, offset: DVec2) -> VectorItem {
        self.device_offset = offset;
        self
    }

    /// Where the item is drawn.
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// The text, for a text item.
    pub(crate) fn as_text(&self) -> Option<&TextItem> {
        match &self.shape {
            VectorShape::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// A tessellated vertex: a world position, a device-pixel extrusion, and what the fragment
/// shader needs for anti-aliasing and dashes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct VectorVertex {
    /// World pixels.
    pub(crate) pos: [f32; 2],
    /// Device pixels added after the camera.
    pub(crate) extrude: [f32; 2],
    /// Straight-alpha colour.
    pub(crate) color: [u8; 4],
    /// −1 to 1 across a stroke; 0 for fills.
    pub(crate) edge: f32,
    /// Half a stroke's width in device pixels; negative for fills (no anti-aliasing).
    pub(crate) half_width: f32,
    /// World pixels along the outline, for dashes.
    pub(crate) along: f32,
    /// Dash on, off and phase in device pixels; on = 0 for a solid line.
    pub(crate) dash: [f32; 3],
}

/// How many vertices a vertex buffer entry takes.
pub(crate) const VECTOR_VERTEX_SIZE: u64 = std::mem::size_of::<VectorVertex>() as u64;

/// Appends the triangles of `item` (not text) at `zoom` device pixels per world pixel.
pub(crate) fn tessellate(item: &VectorItem, zoom: f64, out: &mut Vec<VectorVertex>) {
    let zoom = zoom.max(1e-9);
    let offset = item.device_offset;
    let outline: Option<Outline> = match &item.shape {
        VectorShape::Polyline(points) => Some(Outline {
            points: points.clone(),
            closed: false,
        }),
        VectorShape::Polygon(points) => Some(Outline {
            points: points.clone(),
            closed: true,
        }),
        VectorShape::Rect(rect) => Some(Outline {
            points: vec![
                rect.min,
                DVec2::new(rect.max.x, rect.min.y),
                rect.max,
                DVec2::new(rect.min.x, rect.max.y),
            ],
            closed: true,
        }),
        VectorShape::Ellipse(rect) => Some(Outline {
            points: ellipse_points(*rect, zoom),
            closed: true,
        }),
        VectorShape::Text(_) => None,
    };
    let Some(outline) = outline else {
        return;
    };
    if let Some(fill) = item.fill
        && outline.closed
    {
        fill_polygon(&outline.points, fill, offset, out);
    }
    if let Some(stroke) = item.stroke {
        stroke_outline(&outline, stroke, zoom, offset, out);
    }
}

/// An outline's points and whether it closes.
struct Outline {
    points: Vec<DVec2>,
    closed: bool,
}

/// Points around an ellipse, about four device pixels apart.
fn ellipse_points(rect: DRect, zoom: f64) -> Vec<DVec2> {
    let center = (rect.min + rect.max) * 0.5;
    let radius = rect.size() * 0.5;
    let perimeter_px = std::f64::consts::TAU * radius.max_element() * zoom;
    let count = (perimeter_px / 4.0).ceil().clamp(16.0, 256.0) as usize;
    (0..count)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / count as f64;
            center + DVec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

fn vertex(pos: DVec2, extrude: DVec2, color: Color) -> VectorVertex {
    VectorVertex {
        pos: [pos.x as f32, pos.y as f32],
        extrude: [extrude.x as f32, extrude.y as f32],
        color: color.to_array(),
        edge: 0.0,
        half_width: -1.0,
        along: 0.0,
        dash: [0.0; 3],
    }
}

/// Fills a polygon (ear clipping; a polygon that crosses itself falls back to a fan).
fn fill_polygon(points: &[DVec2], color: Color, offset: DVec2, out: &mut Vec<VectorVertex>) {
    for triangle in triangulate(points) {
        for index in triangle.corners {
            if let Some(&p) = points.get(index) {
                out.push(vertex(p, offset, color));
            }
        }
    }
}

/// A triangle of a polygon, by the indices of its corners.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Triangle {
    pub(crate) corners: [usize; 3],
}

/// Triangulates a simple polygon by ear clipping.
pub(crate) fn triangulate(points: &[DVec2]) -> Vec<Triangle> {
    let mut ring: Vec<usize> = (0..points.len()).collect();
    ring.dedup_by(|a, b| points.get(*a) == points.get(*b));
    let first = ring.first().and_then(|&i| points.get(i));
    if ring.len() >= 2 && first == ring.last().and_then(|&i| points.get(i)) {
        ring.pop();
    }
    if ring.len() < 3 {
        return Vec::new();
    }
    let at = |i: usize| points.get(i).copied().unwrap_or(DVec2::ZERO);
    let twice_area: f64 = ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&a, &b)| at(a).perp_dot(at(b)))
        .sum();
    // Screen y points down: a positive area turns clockwise on screen.
    let clockwise = twice_area > 0.0;
    let mut triangles = Vec::with_capacity(ring.len() - 2);
    while ring.len() > 3 {
        let ear = (0..ring.len()).find_map(|k| {
            let corners = corners_at(&ring, k)?;
            is_ear(points, &ring, corners, clockwise).then_some(Ear { at: k, corners })
        });
        let Some(ear) = ear else {
            // Not a simple polygon: fan what is left.
            let Some((&apex, rest)) = ring.split_first() else {
                return triangles;
            };
            for pair in rest.windows(2) {
                if let [b, c] = pair {
                    triangles.push(Triangle {
                        corners: [apex, *b, *c],
                    });
                }
            }
            return triangles;
        };
        triangles.push(ear.corners);
        ring.remove(ear.at);
    }
    if let [a, b, c] = *ring.as_slice() {
        triangles.push(Triangle { corners: [a, b, c] });
    }
    triangles
}

/// A corner of the ring that can be cut off, and the triangle it makes.
struct Ear {
    at: usize,
    corners: Triangle,
}

/// The ring's corners before, at and after position `k`.
fn corners_at(ring: &[usize], k: usize) -> Option<Triangle> {
    let n = ring.len();
    Some(Triangle {
        corners: [
            *ring.get((k + n - 1) % n)?,
            *ring.get(k)?,
            *ring.get((k + 1) % n)?,
        ],
    })
}

/// Whether a corner is convex and no other point of the ring is inside its triangle.
fn is_ear(points: &[DVec2], ring: &[usize], triangle: Triangle, clockwise: bool) -> bool {
    let at = |i: usize| points.get(i).copied().unwrap_or(DVec2::ZERO);
    let [ia, ib, ic] = triangle.corners;
    let a = at(ia);
    let b = at(ib);
    let c = at(ic);
    let turn = (b - a).perp_dot(c - b);
    let convex = if clockwise { turn > 0.0 } else { turn < 0.0 };
    if !convex {
        return false;
    }
    let inside = |p: DVec2| {
        let d1 = (b - a).perp_dot(p - a);
        let d2 = (c - b).perp_dot(p - b);
        let d3 = (a - c).perp_dot(p - c);
        let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(negative && positive)
    };
    !ring
        .iter()
        .filter(|&&i| i != ia && i != ib && i != ic)
        .any(|&i| inside(at(i)))
}

/// Strokes an outline: a quad per segment, extended by half the width at both ends so corners
/// close, with the anti-aliasing fringe on both sides.
fn stroke_outline(
    outline: &Outline,
    stroke: Stroke,
    zoom: f64,
    offset: DVec2,
    out: &mut Vec<VectorVertex>,
) {
    let points = &outline.points;
    if points.is_empty() {
        return;
    }
    let mut segments: Vec<Segment> = points
        .windows(2)
        .filter_map(|pair| match pair {
            [from, to] => Some(Segment {
                from: *from,
                to: *to,
            }),
            _ => None,
        })
        .collect();
    if outline.closed
        && points.len() > 2
        && let (Some(&last), Some(&first)) = (points.last(), points.first())
    {
        segments.push(Segment {
            from: last,
            to: first,
        });
    }
    let dash = stroke.dash.map_or([0.0; 3], |d| [d.on, d.off, d.phase]);
    let mut along = 0.0f64;
    for segment in segments {
        let delta = segment.to - segment.from;
        let length = delta.length();
        let direction = if length > 0.0 {
            delta / length
        } else {
            DVec2::X
        };
        let normal = DVec2::new(-direction.y, direction.x);
        let width = f64::from(stroke.width);
        // A cosmetic stroke grows in device pixels; a world stroke grows in world pixels and
        // gets only the anti-aliasing fringe in device pixels.
        let half_px = if stroke.cosmetic {
            width * 0.5
        } else {
            width * 0.5 * zoom
        };
        let grow_world = if stroke.cosmetic { 0.0 } else { width * 0.5 };
        let grow_px = if stroke.cosmetic { half_px + 0.5 } else { 0.5 };
        let ends = [
            Corner {
                at: segment.from - direction * grow_world,
                extrude: -direction * grow_px,
                along: along - grow_world - grow_px / zoom,
            },
            Corner {
                at: segment.to + direction * grow_world,
                extrude: direction * grow_px,
                along: along + length + grow_world + grow_px / zoom,
            },
        ];
        let corner = |end: &Corner, side: f64| VectorVertex {
            pos: [
                (end.at + normal * side * grow_world).x as f32,
                (end.at + normal * side * grow_world).y as f32,
            ],
            extrude: [
                (end.extrude + normal * side * grow_px + offset).x as f32,
                (end.extrude + normal * side * grow_px + offset).y as f32,
            ],
            color: stroke.color.to_array(),
            edge: side as f32,
            half_width: half_px as f32,
            along: end.along as f32,
            dash,
        };
        let [start, end] = &ends;
        let quad = [
            corner(start, -1.0),
            corner(end, -1.0),
            corner(start, 1.0),
            corner(start, 1.0),
            corner(end, -1.0),
            corner(end, 1.0),
        ];
        out.extend_from_slice(&quad);
        along += length;
    }
}

/// One straight piece of an outline.
#[derive(Copy, Clone)]
struct Segment {
    from: DVec2,
    to: DVec2,
}

/// An end of a stroked segment: where it is, how far it grows in device pixels, and how far
/// along the outline it is.
struct Corner {
    at: DVec2,
    extrude: DVec2,
    along: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(points: &[DVec2], triangles: &[Triangle]) -> f64 {
        triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.corners.map(|i| points[i]);
                ((b - a).perp_dot(c - a) * 0.5).abs()
            })
            .sum()
    }

    #[test]
    fn concave_polygons_triangulate_to_their_area() {
        // An L: 3×3 minus the 2×2 top-right.
        let l = [
            DVec2::new(0.0, 0.0),
            DVec2::new(1.0, 0.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(3.0, 2.0),
            DVec2::new(3.0, 3.0),
            DVec2::new(0.0, 3.0),
        ];
        let triangles = triangulate(&l);
        assert_eq!(triangles.len(), 4);
        assert!((area(&l, &triangles) - 5.0).abs() < 1e-9);
        let reversed: Vec<DVec2> = l.iter().rev().copied().collect();
        assert!((area(&reversed, &triangulate(&reversed)) - 5.0).abs() < 1e-9);
        // Repeated and closing points are ignored; fewer than three corners draw nothing.
        let square = [
            DVec2::ZERO,
            DVec2::X,
            DVec2::X,
            DVec2::ONE,
            DVec2::Y,
            DVec2::ZERO,
        ];
        assert!((area(&square, &triangulate(&square)) - 1.0).abs() < 1e-9);
        assert!(triangulate(&[DVec2::ZERO, DVec2::X]).is_empty());
    }

    #[test]
    fn strokes_are_quads_with_a_fringe_and_fills_follow_the_shape() {
        let item = VectorItem::rect(DRect::from_min_size(DVec2::ZERO, DVec2::new(10.0, 4.0)))
            .stroke(Color::BLACK, 2.0)
            .fill(Color::WHITE);
        let mut out = Vec::new();
        tessellate(&item, 3.0, &mut out);
        // Two fill triangles, four stroke quads.
        assert_eq!(out.len(), 6 + 4 * 6);
        let stroke = &out[6..];
        // Cosmetic: positions on the outline, the width in the extrusion (1 px + 0.5 fringe).
        assert!(
            stroke
                .iter()
                .all(|v| v.half_width == 1.0 && v.edge.abs() == 1.0)
        );
        assert!(stroke.iter().all(|v| {
            let e = glam::Vec2::from(v.extrude);
            (e.length() - 1.5).abs() < 1e-5 || (e.length() - 1.5 * 2f32.sqrt()).abs() < 1e-4
        }));
        // A dashed polyline counts its length along the way.
        let mut dashed = Vec::new();
        let line = VectorItem::polyline(vec![
            DVec2::ZERO,
            DVec2::new(5.0, 0.0),
            DVec2::new(5.0, 5.0),
        ])
        .stroke(Color::BLACK, 1.0)
        .dashed(Dash {
            on: 2.0,
            off: 2.0,
            phase: 0.0,
        });
        tessellate(&line, 1.0, &mut dashed);
        assert_eq!(dashed.len(), 12);
        assert!(dashed.iter().all(|v| v.dash == [2.0, 2.0, 0.0]));
        assert!(dashed[6..].iter().any(|v| v.along > 9.0));
        // Ellipses get more points as they grow on screen.
        let small = ellipse_points(DRect::from_min_size(DVec2::ZERO, DVec2::splat(4.0)), 1.0);
        let big = ellipse_points(DRect::from_min_size(DVec2::ZERO, DVec2::splat(400.0)), 1.0);
        assert!(small.len() == 16 && big.len() > 200);
    }
}
