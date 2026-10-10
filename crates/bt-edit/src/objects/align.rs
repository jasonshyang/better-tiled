//! Aligning and distributing objects (Tiled has no such commands; these follow what drawing
//! programs do). The deltas are pure functions of bounds in any space, so the editor can align
//! what the screen shows on isometric maps, where screen rectangles are not pixel rectangles.

use bt_core::{Map, ObjectId, ObjectLayer};
use bt_utils::DRect;
use glam::DVec2;

use crate::axis::Axis;
use crate::error::EditError;
use crate::report::ObjectsReport;

use super::bounds::object_bounds;
use super::edit::{ObjectMove, move_each, move_each_in};
use super::find::{objects_by_id, objects_in};
use super::space::ObjectSpace;

/// The edge or middle objects are aligned on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    /// Left edges.
    Left,
    /// Middles across.
    HorizontalCenter,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Middles down.
    VerticalCenter,
    /// Bottom edges.
    Bottom,
}

/// How far each of `bounds` moves to put its edge or middle on that of `to`, in the order of
/// `bounds` and in their space. Bounds that start inside `to` stay inside it.
pub fn align_deltas(bounds: &[DRect], align: Align, to: DRect) -> Vec<DVec2> {
    let middle = |r: &DRect| (r.min + r.max) * 0.5;
    bounds
        .iter()
        .map(|b| match align {
            Align::Left => DVec2::new(to.min.x - b.min.x, 0.0),
            Align::HorizontalCenter => DVec2::new(middle(&to).x - middle(b).x, 0.0),
            Align::Right => DVec2::new(to.max.x - b.max.x, 0.0),
            Align::Top => DVec2::new(0.0, to.min.y - b.min.y),
            Align::VerticalCenter => DVec2::new(0.0, middle(&to).y - middle(b).y),
            Align::Bottom => DVec2::new(0.0, to.max.y - b.max.y),
        })
        .collect()
}

/// How far each of `bounds` moves to space them evenly along `axis`, in the order of `bounds`:
/// ordered by their middles, the first and the last stay, and the others move so the gaps
/// between neighbours are equal (gaps below zero when they cannot fit side by side). When they
/// fit, everything stays inside the rectangle holding them all. Fewer than three never move.
pub fn distribute_deltas(bounds: &[DRect], axis: Axis) -> Vec<DVec2> {
    let mut deltas = vec![DVec2::ZERO; bounds.len()];
    if bounds.len() < 3 {
        return deltas;
    }
    let start = |r: &DRect| axis.along(r.min);
    let end = |r: &DRect| axis.along(r.max);
    let mut order: Vec<usize> = (0..bounds.len()).collect();
    order.sort_by(|&a, &b| {
        let middle = |i: usize| bounds.get(i).map_or(0.0, |r| start(r) + end(r));
        middle(a).total_cmp(&middle(b))
    });
    let (Some(first), Some(last)) = (
        order.first().and_then(|&i| bounds.get(i)),
        order.last().and_then(|&i| bounds.get(i)),
    ) else {
        return deltas;
    };
    let inner = order.get(1..order.len() - 1).unwrap_or_default();
    let occupied: f64 = inner
        .iter()
        .filter_map(|&i| bounds.get(i))
        .map(|r| end(r) - start(r))
        .sum();
    let gaps = (bounds.len() - 1) as f64;
    let gap = (start(last) - end(first) - occupied) / gaps;
    let mut cursor = end(first) + gap;
    for &index in inner {
        if let (Some(r), Some(delta)) = (bounds.get(index), deltas.get_mut(index)) {
            *delta = axis.vector(cursor - start(r));
            cursor += end(r) - start(r) + gap;
        }
    }
    deltas
}

/// Aligns objects of `map` on an edge or middle of `to` (pixel space), by their bounds (see
/// [`object_bounds`]). Reports the objects that moved, in the order given. On isometric maps
/// pixel rectangles are not what the screen shows; the editor lines up screen bounds with
/// [`align_deltas`] instead.
pub fn align_objects(
    map: &mut Map,
    ids: &[ObjectId],
    align: Align,
    to: DRect,
) -> Result<ObjectsReport, EditError> {
    let moves = {
        let space = ObjectSpace::of_map(map);
        let objects = objects_by_id(map, ids)?;
        let bounds: Vec<DRect> = objects.iter().map(|o| object_bounds(&space, o)).collect();
        moves_of(
            objects.iter().map(|o| o.id),
            align_deltas(&bounds, align, to),
        )
    };
    move_each(map, &moves)
}

/// Aligns objects of a list, as [`align_objects`] does.
pub fn align_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    align: Align,
    to: DRect,
) -> Result<ObjectsReport, EditError> {
    let moves = {
        let chosen = objects_in(objects, ids)?;
        let bounds: Vec<DRect> = chosen.iter().map(|o| object_bounds(space, o)).collect();
        moves_of(
            chosen.iter().map(|o| o.id),
            align_deltas(&bounds, align, to),
        )
    };
    move_each_in(objects, &moves)
}

/// Spaces objects of `map` evenly along `axis` by their bounds (see [`distribute_deltas`]).
/// Reports the objects that moved, in the order given.
pub fn distribute_objects(
    map: &mut Map,
    ids: &[ObjectId],
    axis: Axis,
) -> Result<ObjectsReport, EditError> {
    let moves = {
        let space = ObjectSpace::of_map(map);
        let objects = objects_by_id(map, ids)?;
        let bounds: Vec<DRect> = objects.iter().map(|o| object_bounds(&space, o)).collect();
        moves_of(
            objects.iter().map(|o| o.id),
            distribute_deltas(&bounds, axis),
        )
    };
    move_each(map, &moves)
}

/// Spaces objects of a list evenly, as [`distribute_objects`] does.
pub fn distribute_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    axis: Axis,
) -> Result<ObjectsReport, EditError> {
    let moves = {
        let chosen = objects_in(objects, ids)?;
        let bounds: Vec<DRect> = chosen.iter().map(|o| object_bounds(space, o)).collect();
        moves_of(
            chosen.iter().map(|o| o.id),
            distribute_deltas(&bounds, axis),
        )
    };
    move_each_in(objects, &moves)
}

/// The moves of objects that move.
fn moves_of(ids: impl Iterator<Item = ObjectId>, deltas: Vec<DVec2>) -> Vec<ObjectMove> {
    ids.zip(deltas)
        .filter(|(_, delta)| *delta != DVec2::ZERO)
        .map(|(id, delta)| ObjectMove { id, delta })
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::MapObject;
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::MapBuilder;
    use bt_utils::rng::Rng;
    use glam::UVec2;

    use super::*;
    use crate::objects::bounds::objects_bounds;
    use crate::objects::find::find_object;
    use crate::testutil::{assert_valid, rect_object};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> DRect {
        DRect::from_min_size(DVec2::new(x, y), DVec2::new(w, h))
    }

    fn holds(outer: DRect, inner: DRect) -> bool {
        let slack = 1e-9;
        inner.min.x >= outer.min.x - slack
            && inner.min.y >= outer.min.y - slack
            && inner.max.x <= outer.max.x + slack
            && inner.max.y <= outer.max.y + slack
    }

    #[test]
    fn edges_and_middles_line_up() {
        let bounds = [rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 5.0, 4.0, 20.0)];
        let to = rect(-5.0, -5.0, 40.0, 40.0);
        assert_eq!(
            align_deltas(&bounds, Align::Left, to),
            [DVec2::new(-5.0, 0.0), DVec2::new(-25.0, 0.0)]
        );
        assert_eq!(
            align_deltas(&bounds, Align::HorizontalCenter, to),
            [DVec2::new(10.0, 0.0), DVec2::new(-7.0, 0.0)]
        );
        assert_eq!(
            align_deltas(&bounds, Align::Bottom, to),
            [DVec2::new(0.0, 25.0), DVec2::new(0.0, 10.0)]
        );
        assert_eq!(
            align_deltas(&bounds, Align::VerticalCenter, to),
            [DVec2::new(0.0, 10.0), DVec2::new(0.0, 0.0)]
        );
    }

    #[test]
    fn distributing_evens_the_gaps_between_the_ends() {
        // By middles: 0..10 stays first and 90..100 last; the two between fit with gaps of 20.
        let bounds = [
            rect(90.0, 0.0, 10.0, 10.0),
            rect(15.0, 0.0, 10.0, 10.0),
            rect(0.0, 0.0, 10.0, 10.0),
            rect(31.0, 50.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&bounds, Axis::Horizontal);
        assert_eq!(
            deltas,
            [
                DVec2::ZERO,
                DVec2::new(15.0, 0.0),
                DVec2::ZERO,
                DVec2::new(29.0, 0.0)
            ]
        );
        assert_eq!(
            distribute_deltas(&bounds[..2], Axis::Horizontal),
            [DVec2::ZERO, DVec2::ZERO]
        );
        let down = distribute_deltas(
            &[
                rect(0.0, 0.0, 5.0, 5.0),
                rect(0.0, 40.0, 5.0, 5.0),
                rect(0.0, 6.0, 5.0, 5.0),
            ],
            Axis::Vertical,
        );
        assert_eq!(down, [DVec2::ZERO, DVec2::ZERO, DVec2::new(0.0, 14.0)]);
    }

    #[test]
    fn align_and_distribute_keep_bounds_inside_the_target() {
        let fuzz = Fuzz::from_env(0x00A1_1916, 400);
        let aligns = [
            Align::Left,
            Align::HorizontalCenter,
            Align::Right,
            Align::Top,
            Align::VerticalCenter,
            Align::Bottom,
        ];
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let count = 1 + rng.below(6).unwrap() as usize;
            let bounds: Vec<DRect> = (0..count)
                .map(|_| {
                    rect(
                        rng.next_f64() * 200.0 - 100.0,
                        rng.next_f64() * 200.0 - 100.0,
                        rng.next_f64() * 60.0,
                        rng.next_f64() * 60.0,
                    )
                })
                .collect();
            let union = crate::objects::bounds::union_of(bounds.iter().copied()).unwrap();
            let context = || format!("case {case} ({}): {bounds:?}", fuzz.reproduce());
            for align in aligns {
                let target = union.expanded(DVec2::splat(rng.next_f64() * 20.0));
                for (b, d) in bounds.iter().zip(align_deltas(&bounds, align, target)) {
                    let moved = b.translated(d);
                    assert!(holds(target, moved), "{align:?}, {}", context());
                    let across =
                        matches!(align, Align::Left | Align::HorizontalCenter | Align::Right);
                    assert_eq!(if across { d.y } else { d.x }, 0.0, "{}", context());
                }
            }
            for axis in [Axis::Horizontal, Axis::Vertical] {
                let deltas = distribute_deltas(&bounds, axis);
                let moved: Vec<DRect> = bounds
                    .iter()
                    .zip(&deltas)
                    .map(|(b, d)| b.translated(*d))
                    .collect();
                let across = axis.vector(1.0);
                assert!(
                    deltas.iter().all(|d| *d == across * axis.along(*d)),
                    "only along the axis: {}",
                    context()
                );
                if count < 3 {
                    continue;
                }
                let mut order: Vec<usize> = (0..count).collect();
                order.sort_by(|&a, &b| {
                    let middle = |r: &DRect| axis.along(r.min) + axis.along(r.max);
                    middle(&bounds[a]).total_cmp(&middle(&bounds[b]))
                });
                let (first, last) = (order[0], order[count - 1]);
                assert_eq!(deltas[first], DVec2::ZERO, "{}", context());
                assert_eq!(deltas[last], DVec2::ZERO, "{}", context());
                let gaps: Vec<f64> = order
                    .windows(2)
                    .map(|w| axis.along(moved[w[1]].min) - axis.along(moved[w[0]].max))
                    .collect();
                assert!(
                    gaps.iter().all(|g| (g - gaps[0]).abs() < 1e-9),
                    "equal gaps {gaps:?}: {}",
                    context()
                );
                if gaps[0] >= 0.0 {
                    for r in &moved {
                        assert!(holds(union, *r), "{axis:?} {r:?}: {}", context());
                    }
                }
            }
        }
    }

    #[test]
    fn objects_align_and_distribute_across_layers() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(16))
            .object_layer(
                "a",
                vec![
                    rect_object(0.0, 0.0, 10.0, 10.0),
                    rect_object(50.0, 30.0, 20.0, 10.0),
                ],
            )
            .object_layer("b", vec![rect_object(100.0, 5.0, 10.0, 10.0)])
            .build()
            .unwrap();
        let all = [ObjectId(1), ObjectId(2), ObjectId(3)];
        let union = objects_bounds(&map, &all).unwrap().unwrap();
        let report = align_objects(&mut map, &all, Align::Top, union).unwrap();
        assert_eq!(report.changed, [ObjectId(2), ObjectId(3)]);
        let pos = |map: &Map, id| find_object(map, ObjectId(id)).unwrap().object.pos;
        assert_eq!(
            [pos(&map, 1), pos(&map, 2), pos(&map, 3)],
            [DVec2::ZERO, DVec2::new(50.0, 0.0), DVec2::new(100.0, 0.0)]
        );
        // 0..10 and 100..110 stay; the 20 wide one goes to 45..65.
        let report = distribute_objects(&mut map, &all, Axis::Horizontal).unwrap();
        assert_eq!(report.changed, [ObjectId(2)]);
        assert_eq!(pos(&map, 2), DVec2::new(45.0, 0.0));
        assert_valid(&map);
        let mut shapes = map.layers[0].as_object_layer().unwrap().clone();
        let space = ObjectSpace::of_map(&map);
        align_objects_in(
            &mut shapes,
            &space,
            &[ObjectId(1), ObjectId(2)],
            Align::Right,
            rect(0.0, 0.0, 80.0, 80.0),
        )
        .unwrap();
        assert_eq!(
            shapes
                .objects
                .iter()
                .map(|o: &MapObject| o.pos)
                .collect::<Vec<_>>(),
            [DVec2::new(70.0, 0.0), DVec2::new(60.0, 0.0)]
        );
        assert_eq!(
            distribute_objects_in(&mut shapes, &space, &[ObjectId(9)], Axis::Vertical),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
    }
}
