//! Resizing objects with Select Objects' handles (Tiled's `ObjectSelectionTool::updateHandles`,
//! `updateResizingItems` and `updateResizingSingleItem`,
//! `reference/tiled/src/tiled/objectselectiontool.cpp`, and what `TransformMapObjects` marks as
//! changed, `tiled/transformmapobjects.cpp`).
//!
//! One object resizes in its own frame (`bt_core::ObjectFrame`): a shape in pixel space, so a
//! rectangle on an isometric map stays a rectangle of pixels, a tile or text object on screen,
//! and a turned object along its own sides, its handles turned with it. Dragging a handle of a
//! rectangle, ellipse, capsule, text or tile object puts that side or corner where the pointer
//! is while the opposite one stays. Resizing from another origin (the centre, or a point the
//! user moved the origin to), keeping the proportions, or resizing a polygon or polyline scales
//! instead, by how far the handle is from the origin now against where it started, a polygon's
//! points with it. Several objects scale together on screen by one factor, so they keep their
//! proportions and their places against each other. Points only move.
//!
//! The box a handle sits on is what Tiled resizes by: a shape's box in pixel space, the box of a
//! polygon's points, a tile object's image placed by its alignment and tile offset, a text
//! object's box on screen; for several objects the screen rectangle holding each one's box
//! turned with it (the points of a turned polygon). Two differences for tile objects: the tile
//! offset scales with the image, so a dragged side follows the pointer exactly (Tiled moves the
//! box without the offset, so the side jumps by it); and one without a size is resized from its
//! drawn image (Tiled gives it a box of no size at its position).
//!
//! Sizes never become negative or nothing. A side dragged past the opposite one stops at 1 % of
//! the extent it had, the floor Tiled puts on every scale factor (Tiled lets a dragged side stop
//! on the opposite one, at a size of zero). A side without an extent, such as both sides of a
//! rectangle without a size, grows from nothing when dragged out and keeps none otherwise.
//! Where Tiled divides by a box side of no length (a flat polygon dragged by a corner), the
//! side keeps its scale instead of taking an arbitrary one.
//!
//! Positions are pixel space, the same for every layer: the caller removes layer offsets and
//! parallax (Tiled adds each layer's offset). Snapping the dragged handle is the caller's too, by
//! `snap`, as Tiled snaps it in pixel space.

use std::collections::HashMap;

use bt_core::{Map, MapObject, ObjectId, ObjectLayer, ObjectOverrides, Shape};
use bt_utils::DRect;
use glam::DVec2;

use crate::error::EditError;
use crate::report::ObjectsReport;

use super::edit::rewrite_objects_in;
use super::find::{objects_by_id, objects_in, on_layers_holding};
use super::space::ObjectSpace;

/// The least a scale factor can be, as a share of what it scales: Tiled's floor, which keeps
/// objects from shrinking to nothing.
const MIN_SCALE: f64 = 0.01;

/// How close a dragged handle must be to where it was to count as not moved: far below anything
/// visible, above the rounding of going from a frame to pixels and back.
const UNMOVED: f64 = 1e-9;

/// One of the eight handles Select Objects resizes with: a corner of the resize quad, or the
/// middle of one of its sides.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResizeHandle {
    /// The top-left corner.
    TopLeft,
    /// The middle of the top side.
    Top,
    /// The top-right corner.
    TopRight,
    /// The middle of the left side.
    Left,
    /// The middle of the right side.
    Right,
    /// The bottom-left corner.
    BottomLeft,
    /// The middle of the bottom side.
    Bottom,
    /// The bottom-right corner.
    BottomRight,
}

impl ResizeHandle {
    /// Every handle.
    pub const ALL: [ResizeHandle; 8] = [
        ResizeHandle::TopLeft,
        ResizeHandle::Top,
        ResizeHandle::TopRight,
        ResizeHandle::Left,
        ResizeHandle::Right,
        ResizeHandle::BottomLeft,
        ResizeHandle::Bottom,
        ResizeHandle::BottomRight,
    ];

    /// The handle across from this one: the side or corner that stays where it is.
    pub fn opposite(self) -> ResizeHandle {
        match self {
            ResizeHandle::TopLeft => ResizeHandle::BottomRight,
            ResizeHandle::Top => ResizeHandle::Bottom,
            ResizeHandle::TopRight => ResizeHandle::BottomLeft,
            ResizeHandle::Left => ResizeHandle::Right,
            ResizeHandle::Right => ResizeHandle::Left,
            ResizeHandle::BottomLeft => ResizeHandle::TopRight,
            ResizeHandle::Bottom => ResizeHandle::Top,
            ResizeHandle::BottomRight => ResizeHandle::TopLeft,
        }
    }

    /// Where the handle sits on a resize quad (see [`resize_quad`]), whose corners are its
    /// top-left, top-right, bottom-right and bottom-left in that order.
    pub fn on(self, quad: [DVec2; 4]) -> DVec2 {
        let [top_left, top_right, bottom_right, bottom_left] = quad;
        let middle = |a: DVec2, b: DVec2| (a + b) * 0.5;
        match self {
            ResizeHandle::TopLeft => top_left,
            ResizeHandle::Top => middle(top_left, top_right),
            ResizeHandle::TopRight => top_right,
            ResizeHandle::Left => middle(top_left, bottom_left),
            ResizeHandle::Right => middle(top_right, bottom_right),
            ResizeHandle::BottomLeft => bottom_left,
            ResizeHandle::Bottom => middle(bottom_left, bottom_right),
            ResizeHandle::BottomRight => bottom_right,
        }
    }

    /// Where the handle sits on a rectangle in its own space.
    fn on_rect(self, rect: DRect) -> DVec2 {
        let middle = (rect.min + rect.max) * 0.5;
        DVec2::new(
            if self.moves_left() {
                rect.min.x
            } else if self.moves_right() {
                rect.max.x
            } else {
                middle.x
            },
            if self.moves_top() {
                rect.min.y
            } else if self.moves_bottom() {
                rect.max.y
            } else {
                middle.y
            },
        )
    }

    fn moves_left(self) -> bool {
        matches!(
            self,
            ResizeHandle::TopLeft | ResizeHandle::Left | ResizeHandle::BottomLeft
        )
    }

    fn moves_right(self) -> bool {
        matches!(
            self,
            ResizeHandle::TopRight | ResizeHandle::Right | ResizeHandle::BottomRight
        )
    }

    fn moves_top(self) -> bool {
        matches!(
            self,
            ResizeHandle::TopLeft | ResizeHandle::Top | ResizeHandle::TopRight
        )
    }

    fn moves_bottom(self) -> bool {
        matches!(
            self,
            ResizeHandle::BottomLeft | ResizeHandle::Bottom | ResizeHandle::BottomRight
        )
    }

    /// Whether the handle moves its side up or down only (Tiled's horizontal resizing limit).
    fn moves_vertically_only(self) -> bool {
        matches!(self, ResizeHandle::Top | ResizeHandle::Bottom)
    }

    /// Whether the handle moves its side left or right only (Tiled's vertical resizing limit).
    fn moves_horizontally_only(self) -> bool {
        matches!(self, ResizeHandle::Left | ResizeHandle::Right)
    }
}

/// A drag of a resize handle: the handle, where it is dragged to, what stays where it is and
/// whether the proportions are kept. Made by [`Resize::handle_to`]; the other methods change it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Resize {
    handle: ResizeHandle,
    to: DVec2,
    origin: ResizeOrigin,
    keep_proportions: bool,
}

/// What stays where it is while a handle is dragged.
#[derive(Copy, Clone, Debug, PartialEq)]
enum ResizeOrigin {
    /// The opposite handle.
    Opposite,
    /// The middle of the quad.
    Centre,
    /// A point, in pixel space.
    Point(DVec2),
}

impl Resize {
    /// `handle` dragged to `to` (pixel space), the opposite handle staying where it is: Tiled's
    /// plain drag.
    pub fn handle_to(handle: ResizeHandle, to: DVec2) -> Resize {
        Resize {
            handle,
            to,
            origin: ResizeOrigin::Opposite,
            keep_proportions: false,
        }
    }

    /// Resizes around the middle of the quad instead: Tiled's Shift, with the origin where it
    /// starts.
    pub fn from_centre(self) -> Resize {
        Resize {
            origin: ResizeOrigin::Centre,
            ..self
        }
    }

    /// Resizes around `origin` (pixel space) instead: Tiled's Shift, with the origin moved there.
    pub fn from_point(self, origin: DVec2) -> Resize {
        Resize {
            origin: ResizeOrigin::Point(origin),
            ..self
        }
    }

    /// Keeps the object's proportions: Tiled's Ctrl (⌘ on macOS). Several objects always keep
    /// theirs.
    pub fn keeping_proportions(self) -> Resize {
        Resize {
            keep_proportions: true,
            ..self
        }
    }
}

/// The quad the resize handles of the objects of `map` with `ids` sit on, its corners in pixel
/// space in the order top-left, top-right, bottom-right, bottom-left: one object's box turned
/// with it, or the screen rectangle holding several objects' boxes (a parallelogram in pixel
/// space on isometric and oblique maps). A point's quad has no size; Tiled shows no handles
/// for a point alone. `None` for no ids.
pub fn resize_quad(map: &Map, ids: &[ObjectId]) -> Result<Option<[DVec2; 4]>, EditError> {
    let space = ObjectSpace::of_map(map);
    Ok(quad_of(&space, &objects_by_id(map, ids)?))
}

/// The quad the resize handles of objects of a list sit on, as [`resize_quad`] gives it.
pub fn resize_quad_in(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<Option<[DVec2; 4]>, EditError> {
    Ok(quad_of(space, &objects_in(objects, ids)?))
}

/// Resizes the objects of `map` with `ids` by a drag of a handle of their [`resize_quad`], as
/// Tiled's Select Objects does (see the module's documentation). The result depends only on the
/// objects as they are and on the drag, so a tool resizes the objects it held when the drag
/// began. Template instances mark their size, and a polygon's or polyline's points, as their
/// own. Reports the objects that changed, in the order given.
pub fn resize_objects(
    map: &mut Map,
    ids: &[ObjectId],
    resize: Resize,
) -> Result<ObjectsReport, EditError> {
    let resized: HashMap<ObjectId, MapObject> = {
        let space = ObjectSpace::of_map(map);
        resized(&space, &objects_by_id(map, ids)?, resize)
    };
    on_layers_holding(map, ids, |objects, _, held| {
        rewrite_objects_in(objects, held, |object| {
            resized
                .get(&object.id)
                .cloned()
                .unwrap_or_else(|| object.clone())
        })
    })
}

/// Resizes objects of a list, as [`resize_objects`] does.
pub fn resize_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    resize: Resize,
) -> Result<ObjectsReport, EditError> {
    let resized = resized(space, &objects_in(objects, ids)?, resize);
    rewrite_objects_in(objects, ids, |object| {
        resized
            .get(&object.id)
            .cloned()
            .unwrap_or_else(|| object.clone())
    })
}

/// The objects as a drag resizes them, by id.
fn resized(
    space: &ObjectSpace,
    objects: &[&MapObject],
    resize: Resize,
) -> HashMap<ObjectId, MapObject> {
    let changed = match objects {
        [] => Vec::new(),
        [object] => vec![resized_alone(space, object, resize)],
        _ => resized_together(space, objects, resize),
    };
    changed.into_iter().map(|o| (o.id, o)).collect()
}

/// The quad of the handles of `objects`, in pixel space.
fn quad_of(space: &ObjectSpace, objects: &[&MapObject]) -> Option<[DVec2; 4]> {
    match objects {
        [] => None,
        [object] => {
            let frame = space.frame_of(object);
            Some(corners(own_box(space, object)).map(|c| frame.to_pixels(c)))
        }
        _ => {
            let geometry = space.geometry();
            let rect = screen_box(space, objects)?;
            Some(corners(rect).map(|c| geometry.screen_to_pixel(c)))
        }
    }
}

/// The corners of a rectangle: top-left, top-right, bottom-right, bottom-left.
fn corners(rect: DRect) -> [DVec2; 4] {
    [
        rect.min,
        DVec2::new(rect.max.x, rect.min.y),
        rect.max,
        DVec2::new(rect.min.x, rect.max.y),
    ]
}

/// The box an object resizes by, in its frame: a tile object's image, the box of a polygon's or
/// polyline's points (the position when it has none), else the box of its size from its
/// position.
fn own_box(space: &ObjectSpace, object: &MapObject) -> DRect {
    if let Some(image) = space.tile_box(object) {
        return image;
    }
    match &object.shape {
        Shape::Polygon(points) | Shape::Polyline(points) => {
            DRect::from_points(points.iter().copied()).unwrap_or_default()
        }
        Shape::Rectangle | Shape::Ellipse | Shape::Capsule | Shape::Point | Shape::Text(_) => {
            sized_box(object.size)
        }
    }
}

/// The box of `size` from the origin, whichever way it extends.
fn sized_box(size: DVec2) -> DRect {
    DRect {
        min: size.min(DVec2::ZERO),
        max: size.max(DVec2::ZERO),
    }
}

/// The size a resize scales: a tile object's image size when it has no size of its own (it is
/// drawn at that size), else its size.
fn scaled_size(space: &ObjectSpace, object: &MapObject) -> DVec2 {
    match space.tile_box(object) {
        Some(image) if !(object.size.x > 0.0 && object.size.y > 0.0) => image.size(),
        _ => object.size,
    }
}

/// The screen rectangle holding the boxes of `objects`, each turned with its object; the points
/// of a polygon or polyline turned with it (Tiled's `objectBounds`).
fn screen_box(space: &ObjectSpace, objects: &[&MapObject]) -> Option<DRect> {
    objects
        .iter()
        .flat_map(|object| {
            let frame = space.frame_of(object);
            let points: Vec<DVec2> = match &object.shape {
                Shape::Polygon(points) | Shape::Polyline(points)
                    if object.tile.is_none() && !points.is_empty() =>
                {
                    points.clone()
                }
                _ => corners(own_box(space, object)).to_vec(),
            };
            points.into_iter().map(move |p| frame.to_screen(p))
        })
        .fold(None, |rect: Option<DRect>, p| {
            Some(match rect {
                None => DRect { min: p, max: p },
                Some(r) => DRect {
                    min: r.min.min(p),
                    max: r.max.max(p),
                },
            })
        })
}

/// Whether Tiled resizes `object` by moving the dragged side to the pointer, rather than by
/// scaling (Tiled's `canResizeAbsolute`).
fn moves_its_sides(object: &MapObject) -> bool {
    object.tile.is_some()
        || matches!(
            object.shape,
            Shape::Rectangle | Shape::Ellipse | Shape::Capsule | Shape::Text(_)
        )
}

/// `object` resized alone, in its frame (Tiled's `updateResizingSingleItem`).
fn resized_alone(space: &ObjectSpace, object: &MapObject, resize: Resize) -> MapObject {
    let frame = space.frame_of(object);
    let bounds = own_box(space, object);
    let handle = resize.handle.on_rect(bounds);
    let to = if resize.to.distance(frame.to_pixels(handle)) <= UNMOVED {
        handle
    } else {
        frame.from_pixels(resize.to)
    };
    let mut resized = object.clone();
    let place = match resize.origin {
        ResizeOrigin::Opposite if !resize.keep_proportions && moves_its_sides(object) => {
            moved_sides(space, object, &mut resized, bounds, resize.handle, to)
        }
        origin => {
            let origin = match origin {
                ResizeOrigin::Opposite => resize.handle.opposite().on_rect(bounds),
                ResizeOrigin::Centre => (bounds.min + bounds.max) * 0.5,
                ResizeOrigin::Point(point) => frame.from_pixels(point),
            };
            let factor = scale_factors(
                resize.handle,
                handle - origin,
                to - origin,
                resize.keep_proportions,
            );
            if factor == DVec2::ONE {
                return object.clone();
            }
            scale_shape(&mut resized, scaled_size(space, object), factor);
            origin - origin * factor
        }
    };
    resized.pos = frame.to_pixels(place);
    mark_overrides(object, &mut resized);
    resized
}

/// Moves the sides of `object`'s box `bounds` that `handle` moves to `to` (its frame), the
/// opposite ones staying; sets the size of `resized` and returns where its position goes, in
/// the frame. A dragged side stops short of the opposite one by 1 % of the extent it had.
fn moved_sides(
    space: &ObjectSpace,
    object: &MapObject,
    resized: &mut MapObject,
    bounds: DRect,
    handle: ResizeHandle,
    to: DVec2,
) -> DVec2 {
    let floor = bounds.size() * MIN_SCALE;
    let mut dragged = bounds;
    if handle.moves_left() {
        dragged.min.x = to.x.min(bounds.max.x - floor.x);
    }
    if handle.moves_right() {
        dragged.max.x = to.x.max(bounds.min.x + floor.x);
    }
    if handle.moves_top() {
        dragged.min.y = to.y.min(bounds.max.y - floor.y);
    }
    if handle.moves_bottom() {
        dragged.max.y = to.y.max(bounds.min.y + floor.y);
    }
    let across = dragged.min.x != bounds.min.x || dragged.max.x != bounds.max.x;
    let down = dragged.min.y != bounds.min.y || dragged.max.y != bounds.max.y;
    if !across && !down {
        return DVec2::ZERO;
    }
    let mut size = scaled_size(space, object);
    if across {
        size.x = dragged.size().x;
    }
    if down {
        size.y = dragged.size().y;
    }
    resized.size = size;
    // The box at the new size, from the position as it is: the position moves by as much as
    // the box's corner must.
    let placed = own_box(space, resized);
    DVec2::new(
        if across {
            dragged.min.x - placed.min.x
        } else {
            0.0
        },
        if down {
            dragged.min.y - placed.min.y
        } else {
            0.0
        },
    )
}

/// How a handle dragged from `start` to `now` (both from the origin) scales each axis: Tiled's
/// factors with their floor of 1 %. A side handle scales its own axis only, or both alike when
/// keeping the proportions; a corner scales each axis by its own factor, or both by the smaller
/// when keeping the proportions. An axis whose start has no length cannot be scaled by the drag
/// and keeps its scale.
fn scale_factors(handle: ResizeHandle, start: DVec2, now: DVec2, keep: bool) -> DVec2 {
    let x = factor(start.x, now.x);
    let y = factor(start.y, now.y);
    if handle.moves_vertically_only() {
        let y = y.unwrap_or(1.0);
        DVec2::new(if keep { y } else { 1.0 }, y)
    } else if handle.moves_horizontally_only() {
        let x = x.unwrap_or(1.0);
        DVec2::new(x, if keep { x } else { 1.0 })
    } else if keep {
        DVec2::splat(smaller(x, y))
    } else {
        DVec2::new(x.unwrap_or(1.0), y.unwrap_or(1.0))
    }
}

/// The uniform factor a handle dragged from `start` to `now` (both from the origin) scales
/// several objects by: its axis's factor for a side handle, the smaller of the two for a corner
/// (Tiled's `updateResizingItems`).
fn uniform_factor(handle: ResizeHandle, start: DVec2, now: DVec2) -> f64 {
    let x = factor(start.x, now.x);
    let y = factor(start.y, now.y);
    if handle.moves_vertically_only() {
        y.unwrap_or(1.0)
    } else if handle.moves_horizontally_only() {
        x.unwrap_or(1.0)
    } else {
        smaller(x, y)
    }
}

/// How far `now` is along an axis against `start`, at least 1 %; `None` when `start` has no
/// length (or the result is not a number).
fn factor(start: f64, now: f64) -> Option<f64> {
    if start == 0.0 {
        return None;
    }
    let factor = (now / start).max(MIN_SCALE);
    factor.is_finite().then_some(factor)
}

/// The smaller of two factors, either of which may be missing; 1 when both are.
fn smaller(a: Option<f64>, b: Option<f64>) -> f64 {
    match (a, b) {
        (Some(a), Some(b)) => a.min(b),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => 1.0,
    }
}

/// Scales `object`'s size (from `size`) and a polygon's or polyline's points by `factor`.
fn scale_shape(object: &mut MapObject, size: DVec2, factor: DVec2) {
    object.size = size * factor;
    if let Shape::Polygon(points) | Shape::Polyline(points) = &mut object.shape {
        for point in points.iter_mut() {
            *point *= factor;
        }
    }
}

/// Several objects scaled together on screen (Tiled's `updateResizingItems`): their positions
/// around the origin, their sizes and a polygon's or polyline's points by one factor.
fn resized_together(space: &ObjectSpace, objects: &[&MapObject], resize: Resize) -> Vec<MapObject> {
    let geometry = space.geometry();
    let Some(rect) = screen_box(space, objects) else {
        return Vec::new();
    };
    let handle = resize.handle.on_rect(rect);
    let origin = match resize.origin {
        ResizeOrigin::Opposite => resize.handle.opposite().on_rect(rect),
        ResizeOrigin::Centre => (rect.min + rect.max) * 0.5,
        ResizeOrigin::Point(point) => geometry.pixel_to_screen(point),
    };
    let to = geometry.pixel_to_screen(resize.to);
    let scale = uniform_factor(resize.handle, handle - origin, to - origin);
    if scale == 1.0 || to.distance(handle) <= UNMOVED {
        return Vec::new();
    }
    objects
        .iter()
        .map(|object| {
            let mut resized = (*object).clone();
            let screen = geometry.pixel_to_screen(object.pos);
            resized.pos = geometry.screen_to_pixel(origin + (screen - origin) * scale);
            scale_shape(
                &mut resized,
                scaled_size(space, object),
                DVec2::splat(scale),
            );
            mark_overrides(object, &mut resized);
            resized
        })
        .collect()
}

/// Marks what a resize changed on a template instance as its own: the size, and the points.
fn mark_overrides(object: &MapObject, resized: &mut MapObject) {
    if resized.template.is_none() {
        return;
    }
    if resized.size != object.size {
        resized.overrides |= ObjectOverrides::SIZE;
    }
    if resized.shape != object.shape {
        resized.overrides |= ObjectOverrides::SHAPE;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Alignment, Cell, FileRef, Orientation, TextData, TileId, TilesetSlot};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::rng::Rng;
    use glam::{IVec2, UVec2};

    use super::*;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_near, assert_valid, random_object, rect_object};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn tile(id: u32) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(id))
    }

    /// A map of 32 × 16 cells in `orientation` with the fixture atlas of 16-pixel tiles, moved
    /// by `offset` and aligned as given.
    fn map_of(orientation: Orientation, offset: IVec2, alignment: Alignment) -> Map {
        let mut tileset = Atlas::builder()
            .tile_offset(offset)
            .build_embedded()
            .unwrap()
            .tileset
            .as_ref()
            .clone();
        tileset.object_alignment = alignment;
        MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
            .orientation(orientation)
            .skew(IVec2::new(6, 0))
            .tileset(Arc::new(tileset))
            .build()
            .unwrap()
    }

    fn square_map() -> Map {
        map_of(Orientation::Orthogonal, IVec2::ZERO, Alignment::Unspecified)
    }

    /// `object` alone in a list, resized by `resize` in `map`'s space.
    fn resized_one(map: &Map, object: &MapObject, resize: Resize) -> MapObject {
        let space = ObjectSpace::of_map(map);
        let mut list = ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(1),
                ..object.clone()
            }]),
            ..ObjectLayer::default()
        };
        resize_objects_in(&mut list, &space, &[ObjectId(1)], resize).unwrap();
        list.objects[0].clone()
    }

    /// The resize quad of `object` alone in `map`'s space.
    fn quad_of_one(map: &Map, object: &MapObject) -> [DVec2; 4] {
        quad_of(&ObjectSpace::of_map(map), &[object]).unwrap()
    }

    #[track_caller]
    fn assert_placed(object: &MapObject, pos: DVec2, size: DVec2) {
        assert_near(object.pos, pos);
        assert_near(object.size, size);
    }

    #[test]
    fn rectangles_follow_each_handle_as_tileds_do() {
        use ResizeHandle::*;
        let map = square_map();
        let rect = rect_object(10.0, 20.0, 30.0, 10.0);
        assert_eq!(
            quad_of_one(&map, &rect),
            [v(10.0, 20.0), v(40.0, 20.0), v(40.0, 30.0), v(10.0, 30.0)]
        );
        assert_eq!(Right.on(quad_of_one(&map, &rect)), v(40.0, 25.0));
        let drag = |handle, x, y| resized_one(&map, &rect, Resize::handle_to(handle, v(x, y)));
        assert_placed(&drag(BottomRight, 50.0, 40.0), v(10.0, 20.0), v(40.0, 20.0));
        assert_placed(&drag(TopLeft, 0.0, 0.0), v(0.0, 0.0), v(40.0, 30.0));
        assert_placed(&drag(TopRight, 60.0, 10.0), v(10.0, 10.0), v(50.0, 20.0));
        assert_placed(&drag(BottomLeft, 5.0, 35.0), v(5.0, 20.0), v(35.0, 15.0));
        assert_placed(&drag(Top, 999.0, 15.0), v(10.0, 15.0), v(30.0, 15.0));
        assert_placed(&drag(Bottom, -50.0, 45.0), v(10.0, 20.0), v(30.0, 25.0));
        assert_placed(&drag(Left, 0.0, 999.0), v(0.0, 20.0), v(40.0, 10.0));
        assert_placed(&drag(Right, 45.0, -999.0), v(10.0, 20.0), v(35.0, 10.0));
        for handle in ResizeHandle::ALL {
            let expected = if handle.moves_left() { 25.0 } else { 10.0 };
            assert_eq!(drag(handle, 25.0, 25.0).pos.x, expected, "{handle:?}");
        }
        let ellipse = MapObject {
            shape: Shape::Ellipse,
            ..rect.clone()
        };
        let resized = resized_one(&map, &ellipse, Resize::handle_to(TopLeft, v(0.0, 0.0)));
        assert_placed(&resized, v(0.0, 0.0), v(40.0, 30.0));
        assert_eq!(resized.shape, Shape::Ellipse);
        let capsule = MapObject {
            shape: Shape::Capsule,
            ..rect
        };
        assert_placed(
            &resized_one(&map, &capsule, Resize::handle_to(Right, v(70.0, 0.0))),
            v(10.0, 20.0),
            v(60.0, 10.0),
        );
    }

    #[test]
    fn dragged_sides_stop_short_of_the_opposite_one() {
        use ResizeHandle::*;
        let map = square_map();
        let rect = rect_object(10.0, 20.0, 30.0, 10.0);
        let drag = |handle, x, y| resized_one(&map, &rect, Resize::handle_to(handle, v(x, y)));
        assert_placed(&drag(Right, 0.0, 25.0), v(10.0, 20.0), v(0.3, 10.0));
        assert_placed(&drag(Left, 100.0, 25.0), v(39.7, 20.0), v(0.3, 10.0));
        assert_placed(
            &drag(BottomRight, -100.0, -100.0),
            v(10.0, 20.0),
            v(0.3, 0.1),
        );
        assert_placed(&drag(TopLeft, 100.0, 100.0), v(39.7, 29.9), v(0.3, 0.1));
        // A rectangle without a size grows from its position, and keeps no size along an axis
        // dragged the wrong way.
        let marker = rect_object(10.0, 20.0, 0.0, 0.0);
        let drag = |handle, x, y| resized_one(&map, &marker, Resize::handle_to(handle, v(x, y)));
        assert_placed(&drag(BottomRight, 15.0, 27.0), v(10.0, 20.0), v(5.0, 7.0));
        assert_placed(&drag(BottomRight, 5.0, 27.0), v(10.0, 20.0), v(0.0, 7.0));
        assert_placed(&drag(TopLeft, 4.0, 12.0), v(4.0, 12.0), v(6.0, 8.0));
    }

    #[test]
    fn proportions_and_origins_scale_instead() {
        use ResizeHandle::*;
        let map = square_map();
        let rect = rect_object(10.0, 20.0, 30.0, 10.0);
        let drag = |resize| resized_one(&map, &rect, resize);
        let keep = |handle, x, y| Resize::handle_to(handle, v(x, y)).keeping_proportions();
        // A corner scales both sides by the smaller factor.
        assert_placed(
            &drag(keep(BottomRight, 70.0, 25.0)),
            v(10.0, 20.0),
            v(15.0, 5.0),
        );
        assert_placed(
            &drag(keep(BottomRight, 70.0, 45.0)),
            v(10.0, 20.0),
            v(60.0, 20.0),
        );
        // A side scales the other way alike, around the middle of the opposite side.
        assert_placed(&drag(keep(Right, 70.0, 0.0)), v(10.0, 15.0), v(60.0, 20.0));
        assert_placed(&drag(keep(Top, 0.0, 10.0)), v(-5.0, 10.0), v(60.0, 20.0));
        // From the centre, both sides move.
        let centre = |handle, x, y| Resize::handle_to(handle, v(x, y)).from_centre();
        assert_placed(&drag(centre(Right, 50.0, 0.0)), v(0.0, 20.0), v(50.0, 10.0));
        assert_placed(
            &drag(centre(BottomRight, 55.0, 35.0)),
            v(-5.0, 15.0),
            v(60.0, 20.0),
        );
        assert_placed(
            &drag(centre(BottomRight, 55.0, 40.0).keeping_proportions()),
            v(-5.0, 15.0),
            v(60.0, 20.0),
        );
        // From a point the origin was moved to.
        let around = Resize::handle_to(BottomRight, v(70.0, 40.0)).from_point(v(10.0, 20.0));
        assert_placed(&drag(around), v(10.0, 20.0), v(60.0, 20.0));
        let around = Resize::handle_to(Right, v(70.0, 0.0)).from_point(v(-20.0, 0.0));
        assert_placed(&drag(around), v(25.0, 20.0), v(45.0, 10.0));
        // Scaling never goes below 1 %.
        assert_placed(
            &drag(keep(BottomRight, -500.0, -500.0)),
            v(10.0, 20.0),
            v(0.3, 0.1),
        );
        // A rectangle without a size has nothing to scale.
        let marker = rect_object(10.0, 20.0, 0.0, 0.0);
        let kept = resized_one(&map, &marker, keep(BottomRight, 50.0, 50.0));
        assert_eq!(
            kept,
            MapObject {
                id: ObjectId(1),
                ..marker
            }
        );
    }

    #[test]
    fn turned_objects_resize_along_their_own_sides() {
        let map = square_map();
        let bar = MapObject {
            rotation: 90.0,
            ..rect_object(100.0, 100.0, 40.0, 10.0)
        };
        // Turned a quarter clockwise, the bar hangs down and to the left of its position.
        assert_eq!(
            quad_of_one(&map, &bar),
            [
                v(100.0, 100.0),
                v(100.0, 140.0),
                v(90.0, 140.0),
                v(90.0, 100.0)
            ]
        );
        let drag = |handle, x, y| resized_one(&map, &bar, Resize::handle_to(handle, v(x, y)));
        assert_placed(
            &drag(ResizeHandle::BottomRight, 80.0, 160.0),
            v(100.0, 100.0),
            v(60.0, 20.0),
        );
        assert_placed(
            &drag(ResizeHandle::TopLeft, 110.0, 90.0),
            v(110.0, 90.0),
            v(50.0, 20.0),
        );
        // Its right side is at the bottom on screen: dragging it down lengthens the bar.
        let longer = drag(ResizeHandle::Right, 0.0, 150.0);
        assert_placed(&longer, v(100.0, 100.0), v(50.0, 10.0));
        assert_eq!(longer.rotation, 90.0);
    }

    #[test]
    fn polygons_and_polylines_scale_their_points() {
        use ResizeHandle::*;
        let map = square_map();
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(20.0, 0.0), v(0.0, 10.0)]),
            ..rect_object(10.0, 10.0, 0.0, 0.0)
        };
        let doubled = Shape::Polygon(vec![v(0.0, 0.0), v(40.0, 0.0), v(0.0, 20.0)]);
        let drag = |object: &MapObject, resize| resized_one(&map, object, resize);
        let corner = drag(&polygon, Resize::handle_to(BottomRight, v(50.0, 30.0)));
        assert_eq!((corner.pos, &corner.shape), (v(10.0, 10.0), &doubled));
        let corner = drag(&polygon, Resize::handle_to(TopLeft, v(-10.0, 0.0)));
        assert_eq!((corner.pos, &corner.shape), (v(-10.0, 0.0), &doubled));
        let wider = drag(&polygon, Resize::handle_to(Right, v(50.0, 99.0)));
        assert_eq!(wider.pos, v(10.0, 10.0));
        assert_eq!(
            wider.shape,
            Shape::Polygon(vec![v(0.0, 0.0), v(40.0, 0.0), v(0.0, 10.0)])
        );
        let kept = drag(
            &polygon,
            Resize::handle_to(Right, v(50.0, 99.0)).keeping_proportions(),
        );
        assert_eq!((kept.pos, &kept.shape), (v(10.0, 5.0), &doubled));
        // Points away from the position scale around the box's corner too.
        let off = MapObject {
            shape: Shape::Polyline(vec![v(5.0, 5.0), v(15.0, 5.0), v(5.0, 15.0)]),
            ..rect_object(0.0, 0.0, 0.0, 0.0)
        };
        let scaled = drag(&off, Resize::handle_to(BottomRight, v(25.0, 25.0)));
        assert_eq!(scaled.pos, v(-5.0, -5.0));
        assert_eq!(
            scaled.shape,
            Shape::Polyline(vec![v(10.0, 10.0), v(30.0, 10.0), v(10.0, 30.0)])
        );
        // A flat line keeps its scale across rather than collapsing, as Tiled's would to 1 %.
        let flat = MapObject {
            shape: Shape::Polyline(vec![v(0.0, 0.0), v(20.0, 0.0)]),
            ..rect_object(0.0, 0.0, 0.0, 0.0)
        };
        let stretched = drag(
            &flat,
            Resize::handle_to(BottomRight, v(40.0, -5.0)).keeping_proportions(),
        );
        assert_eq!(
            stretched.shape,
            Shape::Polyline(vec![v(0.0, 0.0), v(40.0, 0.0)])
        );
        assert_eq!(stretched.pos, v(0.0, 0.0));
    }

    #[test]
    fn points_only_move() {
        let map = square_map();
        let point = MapObject {
            shape: Shape::Point,
            ..rect_object(10.0, 10.0, 0.0, 0.0)
        };
        let drag = Resize::handle_to(ResizeHandle::BottomRight, v(30.0, 30.0));
        assert_eq!(
            resized_one(&map, &point, drag).pos,
            v(10.0, 10.0),
            "alone, a point has nothing to resize"
        );
        let around = resized_one(&map, &point, drag.from_point(v(0.0, 0.0)));
        assert_eq!((around.pos, around.size), (v(30.0, 30.0), DVec2::ZERO));
    }

    #[test]
    fn tile_objects_resize_upright_by_their_image() {
        use ResizeHandle::*;
        let map = square_map();
        let object = MapObject {
            tile: Some(tile(3)),
            ..rect_object(100.0, 100.0, 32.0, 32.0)
        };
        // Bottom-left: the image stands on the position.
        assert_eq!(
            quad_of_one(&map, &object),
            [
                v(100.0, 68.0),
                v(132.0, 68.0),
                v(132.0, 100.0),
                v(100.0, 100.0)
            ]
        );
        let drag = |object: &MapObject, map: &Map, handle, x, y| {
            resized_one(map, object, Resize::handle_to(handle, v(x, y)))
        };
        assert_placed(
            &drag(&object, &map, Top, 0.0, 52.0),
            v(100.0, 100.0),
            v(32.0, 48.0),
        );
        assert_placed(
            &drag(&object, &map, Left, 90.0, 0.0),
            v(90.0, 100.0),
            v(42.0, 32.0),
        );
        assert_placed(
            &drag(&object, &map, Bottom, 0.0, 110.0),
            v(100.0, 110.0),
            v(32.0, 42.0),
        );
        // Centred, the position follows the middle of the image.
        let centred = map_of(Orientation::Orthogonal, IVec2::ZERO, Alignment::Center);
        assert_placed(
            &drag(&object, &centred, Right, 124.0, 0.0),
            v(104.0, 100.0),
            v(40.0, 32.0),
        );
        // With a tile offset, scaled with the image, the dragged side follows the pointer
        // exactly and the other stays.
        let moved = map_of(
            Orientation::Orthogonal,
            IVec2::new(4, -2),
            Alignment::Unspecified,
        );
        let before = quad_of_one(&moved, &object);
        assert_eq!(before[0], v(108.0, 64.0));
        let resized = drag(&object, &moved, Right, 160.0, 0.0);
        let after = quad_of_one(&moved, &resized);
        assert_near(after[0], before[0]);
        assert_near(after[2], v(160.0, before[2].y));
        // A tile object without a size is resized from its tile's size.
        let sizeless = MapObject {
            size: DVec2::ZERO,
            ..object.clone()
        };
        assert_placed(
            &drag(&sizeless, &map, Right, 120.0, 0.0),
            v(100.0, 100.0),
            v(20.0, 16.0),
        );
        assert_eq!(
            drag(&sizeless, &map, Right, 116.0, 0.0).size,
            DVec2::ZERO,
            "the side where it is changes nothing"
        );
        // On an isometric map the image stands upright: its sides move on screen.
        let iso = map_of(Orientation::Isometric, IVec2::ZERO, Alignment::Unspecified);
        let geometry = iso.geometry();
        let foot = geometry.pixel_to_screen(object.pos);
        let quad = quad_of_one(&iso, &object);
        assert_near(
            geometry.pixel_to_screen(Right.on(quad)),
            foot + v(16.0, -16.0),
        );
        let to = geometry.screen_to_pixel(foot + v(26.0, -16.0));
        let wider = resized_one(&iso, &object, Resize::handle_to(Right, to));
        assert_near(wider.size, v(42.0, 32.0));
        assert_near(geometry.pixel_to_screen(wider.pos), foot + v(5.0, 0.0));
    }

    #[test]
    fn text_resizes_upright() {
        let iso = map_of(Orientation::Isometric, IVec2::ZERO, Alignment::Unspecified);
        let geometry = iso.geometry();
        let text = MapObject {
            shape: Shape::Text(TextData::default()),
            ..rect_object(64.0, 32.0, 40.0, 20.0)
        };
        let corner = geometry.pixel_to_screen(text.pos);
        let to = geometry.screen_to_pixel(corner + v(60.0, 30.0));
        let resized = resized_one(
            &iso,
            &text,
            Resize::handle_to(ResizeHandle::BottomRight, to),
        );
        assert_near(resized.size, v(60.0, 30.0));
        assert_near(resized.pos, text.pos);
    }

    #[test]
    fn shapes_on_isometric_maps_resize_in_pixel_space() {
        let iso = map_of(Orientation::Isometric, IVec2::ZERO, Alignment::Unspecified);
        let cell = rect_object(32.0, 32.0, 32.0, 32.0);
        let quad = quad_of_one(&iso, &cell);
        assert_eq!(ResizeHandle::BottomRight.on(quad), v(64.0, 64.0));
        let resized = resized_one(
            &iso,
            &cell,
            Resize::handle_to(ResizeHandle::BottomRight, v(96.0, 80.0)),
        );
        assert_placed(&resized, v(32.0, 32.0), v(64.0, 48.0));
    }

    /// Two rectangles, a polygon and a point on one layer.
    fn group() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    rect_object(20.0, 0.0, 10.0, 10.0),
                    MapObject {
                        shape: Shape::Polygon(vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)]),
                        ..rect_object(0.0, 20.0, 0.0, 0.0)
                    },
                    MapObject {
                        shape: Shape::Point,
                        ..rect_object(30.0, 30.0, 0.0, 0.0)
                    },
                ],
            )
            .build()
            .unwrap()
    }

    fn all() -> [ObjectId; 4] {
        [ObjectId(1), ObjectId(2), ObjectId(3), ObjectId(4)]
    }

    fn object(map: &Map, id: u32) -> MapObject {
        find_object(map, ObjectId(id)).unwrap().object.clone()
    }

    #[test]
    fn several_objects_scale_together_on_screen() {
        let base = group();
        assert_eq!(
            resize_quad(&base, &all()).unwrap(),
            Some([v(0.0, 0.0), v(30.0, 0.0), v(30.0, 30.0), v(0.0, 30.0)])
        );
        let run = |resize| {
            let mut map = base.clone();
            let report = resize_objects(&mut map, &all(), resize).unwrap();
            assert_valid(&map);
            (map, report)
        };
        let (map, report) = run(Resize::handle_to(ResizeHandle::BottomRight, v(60.0, 60.0)));
        assert_eq!(report.changed, all());
        assert_placed(&object(&map, 1), v(0.0, 0.0), v(20.0, 20.0));
        assert_placed(&object(&map, 2), v(40.0, 0.0), v(20.0, 20.0));
        assert_eq!(object(&map, 3).pos, v(0.0, 40.0));
        assert_eq!(
            object(&map, 3).shape,
            Shape::Polygon(vec![v(0.0, 0.0), v(20.0, 0.0), v(0.0, 20.0)])
        );
        assert_placed(&object(&map, 4), v(60.0, 60.0), DVec2::ZERO);
        // One factor for all, the smaller of a corner's two, whatever the proportions setting.
        let (wide, _) = run(Resize::handle_to(ResizeHandle::BottomRight, v(60.0, 90.0)));
        assert_eq!(wide, map);
        let (kept, _) =
            run(Resize::handle_to(ResizeHandle::BottomRight, v(60.0, 90.0)).keeping_proportions());
        assert_eq!(kept, map);
        // A side scales everything alike around the middle of the opposite side.
        let (side, _) = run(Resize::handle_to(ResizeHandle::Right, v(45.0, -9.0)));
        assert_placed(&object(&side, 2), v(30.0, -7.5), v(15.0, 15.0));
        let (centred, _) =
            run(Resize::handle_to(ResizeHandle::BottomRight, v(45.0, 45.0)).from_centre());
        assert_placed(&object(&centred, 1), v(-15.0, -15.0), v(20.0, 20.0));
        // Dragging a handle to where it is changes nothing.
        let (same, report) = run(Resize::handle_to(ResizeHandle::Top, v(15.0, 0.0)));
        assert!(report.changed.is_empty());
        assert_eq!(same, base);
    }

    #[test]
    fn several_turned_objects_are_held_by_their_turned_boxes() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    MapObject {
                        rotation: 90.0,
                        ..rect_object(40.0, 0.0, 20.0, 10.0)
                    },
                    MapObject {
                        rotation: 45.0,
                        shape: Shape::Ellipse,
                        ..rect_object(0.0, 40.0, 10.0, 10.0)
                    },
                ],
            )
            .build()
            .unwrap();
        let quad = resize_quad(&map, &[ObjectId(1), ObjectId(2), ObjectId(3)])
            .unwrap()
            .unwrap();
        // The turned bar reaches 20 down; the turned ellipse's box (not the ellipse) reaches
        // 10√2 down from its position.
        assert_near(quad[0], v(-10.0 * 0.5f64.sqrt(), 0.0));
        assert_near(quad[2], v(40.0, 40.0 + 10.0 * 2f64.sqrt()));
    }

    #[test]
    fn several_objects_on_isometric_maps_scale_around_the_origin() {
        let mut map = map_of(Orientation::Isometric, IVec2::ZERO, Alignment::Unspecified);
        map.layers.push(bt_core::Layer::new(
            bt_core::LayerId(1),
            "things",
            bt_core::LayerKind::Object(ObjectLayer {
                objects: Arc::new(vec![
                    MapObject {
                        id: ObjectId(1),
                        ..rect_object(32.0, 32.0, 16.0, 16.0)
                    },
                    MapObject {
                        id: ObjectId(2),
                        shape: Shape::Point,
                        ..rect_object(96.0, 48.0, 0.0, 0.0)
                    },
                ]),
                ..ObjectLayer::default()
            }),
        ));
        map.next_layer_id = 2;
        map.next_object_id = 3;
        assert_valid(&map);
        let ids = [ObjectId(1), ObjectId(2)];
        let quad = resize_quad(&map, &ids).unwrap().unwrap();
        let origin = ResizeHandle::TopLeft.on(quad);
        let handle = ResizeHandle::BottomRight.on(quad);
        // Halfway along the diagonal from the origin: everything halves around the origin, in
        // pixel space as on screen.
        let before = map.clone();
        let halfway = Resize::handle_to(ResizeHandle::BottomRight, (origin + handle) * 0.5);
        resize_objects(&mut map, &ids, halfway).unwrap();
        for id in [1, 2] {
            let old = object(&before, id);
            let new = object(&map, id);
            assert_near(new.pos, origin + (old.pos - origin) * 0.5);
            assert_near(new.size, old.size * 0.5);
        }
    }

    #[test]
    fn template_instances_mark_what_a_resize_changes() {
        let map = square_map();
        let instance = MapObject {
            template: Some(FileRef::new("box.tx", "/box.tx")),
            ..rect_object(0.0, 0.0, 10.0, 10.0)
        };
        let resized = resized_one(
            &map,
            &instance,
            Resize::handle_to(ResizeHandle::Right, v(20.0, 5.0)),
        );
        assert_eq!(resized.overrides, ObjectOverrides::SIZE);
        let polygon = MapObject {
            shape: Shape::Polygon(vec![v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)]),
            ..instance.clone()
        };
        let resized = resized_one(
            &map,
            &polygon,
            Resize::handle_to(ResizeHandle::BottomRight, v(20.0, 20.0)),
        );
        assert_eq!(
            resized.overrides,
            ObjectOverrides::SIZE | ObjectOverrides::SHAPE
        );
        let moved = resized_one(
            &map,
            &instance,
            Resize::handle_to(ResizeHandle::Right, v(20.0, 5.0)).from_point(v(30.0, 0.0)),
        );
        assert_eq!(moved.pos, v(15.0, 0.0));
        assert_eq!(
            moved.overrides,
            ObjectOverrides::SIZE,
            "the position is always an instance's own"
        );
    }

    #[test]
    fn resizing_finds_its_objects_or_says_so() {
        let mut map = group();
        let drag = Resize::handle_to(ResizeHandle::Right, v(50.0, 0.0));
        assert_eq!(
            resize_objects(&mut map, &[ObjectId(1), ObjectId(9)], drag),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
        assert_eq!(map, group(), "nothing changes");
        assert_eq!(resize_quad(&map, &[]), Ok(None));
        assert!(
            resize_objects(&mut map, &[], drag)
                .unwrap()
                .changed
                .is_empty()
        );
    }

    #[test]
    fn a_tiles_collision_shapes_resize_in_its_space() {
        let tileset = Atlas::builder().tile_size(32, 32).build_embedded().unwrap();
        let space = ObjectSpace::of_tile(&tileset.tileset, TileId(0));
        let mut shapes = ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(1),
                ..rect_object(4.0, 4.0, 8.0, 8.0)
            }]),
            ..ObjectLayer::default()
        };
        let quad = resize_quad_in(&shapes, &space, &[ObjectId(1)])
            .unwrap()
            .unwrap();
        assert_eq!(ResizeHandle::BottomRight.on(quad), v(12.0, 12.0));
        resize_objects_in(
            &mut shapes,
            &space,
            &[ObjectId(1)],
            Resize::handle_to(ResizeHandle::BottomRight, v(28.0, 20.0)),
        )
        .unwrap();
        assert_placed(&shapes.objects[0], v(4.0, 4.0), v(24.0, 16.0));
    }

    /// A random grid of the three a resize treats differently, with tiles moved by an offset
    /// and aligned one of three ways.
    fn random_map(rng: &mut Rng) -> Map {
        let orientation = match rng.below(3) {
            Some(0) => Orientation::Orthogonal,
            Some(1) => Orientation::Isometric,
            _ => Orientation::Oblique,
        };
        let alignment = match rng.below(3) {
            Some(0) => Alignment::Unspecified,
            Some(1) => Alignment::Center,
            _ => Alignment::TopRight,
        };
        map_of(orientation, IVec2::new(3, -5), alignment)
    }

    #[test]
    fn dragged_corners_land_where_they_are_dragged() {
        let fuzz = Fuzz::from_env(0x2E51_2E0B, 300);
        let mut tried = 0;
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let map = random_map(&mut rng);
            let space = ObjectSpace::of_map(&map);
            let object = random_object(&mut rng);
            if !moves_its_sides(&object) || scaled_size(&space, &object) == DVec2::ZERO {
                continue;
            }
            tried += 1;
            let handle = [
                ResizeHandle::TopLeft,
                ResizeHandle::TopRight,
                ResizeHandle::BottomLeft,
                ResizeHandle::BottomRight,
            ][rng.below(4).unwrap() as usize];
            let frame = space.frame_of(&object);
            // Somewhere past the opposite corner on the handle's side.
            let fixed = handle.opposite().on_rect(own_box(&space, &object));
            let reach = v(1.0 + rng.next_f64() * 50.0, 1.0 + rng.next_f64() * 50.0);
            let away = v(
                if handle.moves_left() {
                    -reach.x
                } else {
                    reach.x
                },
                if handle.moves_top() {
                    -reach.y
                } else {
                    reach.y
                },
            );
            let to = frame.to_pixels(fixed + away);
            let before = quad_of_one(&map, &object);
            let resized = resized_one(&map, &object, Resize::handle_to(handle, to));
            let after = quad_of_one(&map, &resized);
            let context = || {
                format!(
                    "case {case} ({}): {handle:?} of {object:?} to {to}",
                    fuzz.reproduce()
                )
            };
            assert!(
                handle.on(after).distance(to) < 1e-6,
                "landed at {}: {}",
                handle.on(after),
                context()
            );
            assert!(
                handle
                    .opposite()
                    .on(after)
                    .distance(handle.opposite().on(before))
                    < 1e-6,
                "{}",
                context()
            );
            assert_eq!(resized.rotation, object.rotation, "{}", context());
        }
        assert!(tried > fuzz.cases / 4, "only {tried} cases tried");
    }

    #[test]
    fn random_resizes_keep_sizes_and_change_nothing_unmoved() {
        let fuzz = Fuzz::from_env(0x2E51_2E0C, 400);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let map = random_map(&mut rng);
            let space = ObjectSpace::of_map(&map);
            let objects: Vec<MapObject> = (0..1 + rng.below(3).unwrap())
                .map(|i| MapObject {
                    id: ObjectId(i as u32 + 1),
                    ..random_object(&mut rng)
                })
                .collect();
            let held: Vec<&MapObject> = objects.iter().collect();
            let quad = quad_of(&space, &held).unwrap();
            let handle = ResizeHandle::ALL[rng.below(8).unwrap() as usize];
            let start = handle.on(quad);
            let to = start
                + v(
                    rng.next_f64() * 200.0 - 100.0,
                    rng.next_f64() * 200.0 - 100.0,
                );
            let mut resize = Resize::handle_to(handle, to);
            if rng.below(3) == Some(0) {
                resize = resize.keeping_proportions();
            }
            match rng.below(4) {
                Some(0) => resize = resize.from_centre(),
                Some(1) => resize = resize.from_point(start + v(rng.next_f64() * 40.0 - 20.0, 7.0)),
                _ => {}
            }
            let ids: Vec<ObjectId> = objects.iter().map(|o| o.id).collect();
            let list = |objects: &[MapObject]| ObjectLayer {
                objects: Arc::new(objects.to_vec()),
                ..ObjectLayer::default()
            };
            let mut resized = list(&objects);
            let report = resize_objects_in(&mut resized, &space, &ids, resize).unwrap();
            let context = || {
                format!(
                    "case {case} ({}): {resize:?} of {objects:?}",
                    fuzz.reproduce()
                )
            };
            for (old, new) in objects.iter().zip(resized.objects.iter()) {
                let size = scaled_size(&space, old);
                let now = scaled_size(&space, new);
                assert!(
                    new.pos.is_finite() && now.is_finite(),
                    "{new:?}: {}",
                    context()
                );
                assert!(now.x >= 0.0 && now.y >= 0.0, "{new:?}: {}", context());
                if new != old {
                    assert!(
                        (size.x == 0.0 || now.x > 0.0) && (size.y == 0.0 || now.y > 0.0),
                        "{new:?} lost its size: {}",
                        context()
                    );
                    assert!(report.changed.contains(&new.id), "{}", context());
                }
                assert_eq!(new.rotation, old.rotation, "{}", context());
            }
            // Dragging the handle to where it is changes nothing.
            let mut unmoved = list(&objects);
            let report =
                resize_objects_in(&mut unmoved, &space, &ids, Resize::handle_to(handle, start))
                    .unwrap();
            assert!(report.changed.is_empty(), "{report:?}: {}", context());
        }
    }
}
