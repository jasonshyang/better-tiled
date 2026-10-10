//! The order objects are drawn and listed in: the draw order of a list, and Tiled's commands that
//! change it (`reference/tiled/src/tiled/raiselowerhelper.cpp` for Raise, Lower, Raise to Top and
//! Lower to Bottom; `MapDocument::moveObjectsUp` and `moveObjectsDown` in `mapdocument.cpp` for
//! the Objects view's Move Up and Down; `ObjectGroup::moveObjects` in
//! `reference/tiled/src/libtiled/objectgroup.cpp`).

use std::collections::HashSet;
use std::sync::Arc;

use bt_core::{DrawOrder, Map, MapGeometry, MapObject, ObjectId, ObjectLayer};

use crate::error::EditError;
use crate::report::ObjectsReport;

use super::find::{ids_by_layer, indices_in, object_layer, on_layers_holding};
use super::space::ObjectSpace;

/// The objects of a list in the order they are drawn, bottom first: the list's order, or on a
/// top-down list by where they are drawn on screen from the top, objects at the same height
/// keeping their list order. The renderer sorts by the same key, so this is also the order hit
/// tests see.
pub fn render_order<'a>(objects: &'a ObjectLayer, geometry: &MapGeometry) -> Vec<&'a MapObject> {
    let mut order: Vec<&MapObject> = objects.objects.iter().collect();
    if objects.draw_order == DrawOrder::TopDown {
        order.sort_by(|a, b| {
            let a = geometry.pixel_to_screen(a.pos).y;
            let b = geometry.pixel_to_screen(b.pos).y;
            a.total_cmp(&b)
        });
    }
    order
}

/// Raise Object: each run of the objects among those sharing space with them moves its top
/// object just above the next object it overlaps, as Tiled's `RaiseLowerHelper` does, on every
/// layer holding some. Layers must draw in list order.
pub fn raise_objects(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    in_index_order(map, ids)?;
    on_layers_holding(map, ids, |objects, space, held| {
        raise_objects_in(objects, space, held)
    })
}

/// Raise Object on one list, whatever its draw order (a tile's collision shapes are edited in
/// list order).
pub fn raise_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let related = related(objects, space, &indices);
    let selected = positions_among(&related, &indices);
    Ok(rearranged(objects, &indices, |order| {
        // From the top, so lower runs keep their places.
        for run in runs(&selected).iter().rev() {
            let (Some(&moving), Some(&above)) = (related.get(run.last), related.get(run.last + 1))
            else {
                continue;
            };
            if let (Some(from), Some(target)) = (place_of(order, moving), place_of(order, above)) {
                move_run(order, from, target + 1, 1);
            }
        }
    }))
}

/// Lower Object: each run moves its bottom object just below the previous object it overlaps,
/// on every layer holding some. Layers must draw in list order.
pub fn lower_objects(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    in_index_order(map, ids)?;
    on_layers_holding(map, ids, |objects, space, held| {
        lower_objects_in(objects, space, held)
    })
}

/// Lower Object on one list, whatever its draw order.
pub fn lower_objects_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let related = related(objects, space, &indices);
    let selected = positions_among(&related, &indices);
    Ok(rearranged(objects, &indices, |order| {
        for run in runs(&selected) {
            let Some(below_position) = run.first.checked_sub(1) else {
                continue;
            };
            let (Some(&moving), Some(&below)) =
                (related.get(run.first), related.get(below_position))
            else {
                continue;
            };
            if let (Some(from), Some(target)) = (place_of(order, moving), place_of(order, below)) {
                move_run(order, from, target, 1);
            }
        }
    }))
}

/// Raise Object to Top: the objects go to the top of their layers, keeping their order. Layers
/// must draw in list order.
pub fn raise_objects_to_top(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    in_index_order(map, ids)?;
    on_layers_holding(map, ids, |objects, _, held| {
        raise_objects_to_top_in(objects, held)
    })
}

/// Raise Object to Top on one list, whatever its draw order.
pub fn raise_objects_to_top_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let plan = runs(&sorted(&indices));
    Ok(rearranged(objects, &indices, |order| {
        let mut to = order.len();
        for run in plan.iter().rev() {
            if run.last + 1 != to {
                move_run(order, run.first, to, run.len());
            }
            to -= run.len();
        }
    }))
}

/// Lower Object to Bottom: the objects go to the bottom of their layers, keeping their order.
/// Layers must draw in list order.
pub fn lower_objects_to_bottom(
    map: &mut Map,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    in_index_order(map, ids)?;
    on_layers_holding(map, ids, |objects, _, held| {
        lower_objects_to_bottom_in(objects, held)
    })
}

/// Lower Object to Bottom on one list, whatever its draw order.
pub fn lower_objects_to_bottom_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let plan = runs(&sorted(&indices));
    Ok(rearranged(objects, &indices, |order| {
        let mut to = 0;
        for run in &plan {
            if run.first != to {
                move_run(order, run.first, to, run.len());
            }
            to += run.len();
        }
    }))
}

/// The Objects view's Move Up: each run of the objects moves one place up its layer's list,
/// past the object above it, on any layer (the list is what the view shows).
pub fn move_objects_up(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, _, held| {
        move_objects_up_in(objects, held)
    })
}

/// Move Up on one list.
pub fn move_objects_up_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let plan = runs(&sorted(&indices));
    Ok(rearranged(objects, &indices, |order| {
        let len = order.len();
        for run in plan.iter().rev() {
            let to = run.first + run.len() + 1;
            if to <= len {
                move_run(order, run.first, to, run.len());
            }
        }
    }))
}

/// The Objects view's Move Down: each run of the objects moves one place down its layer's
/// list, on any layer.
pub fn move_objects_down(map: &mut Map, ids: &[ObjectId]) -> Result<ObjectsReport, EditError> {
    on_layers_holding(map, ids, |objects, _, held| {
        move_objects_down_in(objects, held)
    })
}

/// Move Down on one list.
pub fn move_objects_down_in(
    objects: &mut ObjectLayer,
    ids: &[ObjectId],
) -> Result<ObjectsReport, EditError> {
    let indices = indices_in(objects, ids)?;
    let plan = runs(&sorted(&indices));
    Ok(rearranged(objects, &indices, |order| {
        for run in &plan {
            if let Some(to) = run.first.checked_sub(1) {
                move_run(order, run.first, to, run.len());
            }
        }
    }))
}

/// Refuses when a layer holding some of `ids` draws top-down, where list order cannot raise or
/// lower anything (Tiled offers the commands only on layers drawn in list order).
fn in_index_order(map: &Map, ids: &[ObjectId]) -> Result<(), EditError> {
    for group in ids_by_layer(map, ids)? {
        if object_layer(map, group.layer)?.draw_order != DrawOrder::Index {
            return Err(EditError::NotIndexOrdered(group.layer));
        }
    }
    Ok(())
}

/// Reorders a list: `arrange` rearranges the places of its objects, bottom first. Reports the
/// objects at `indices` whose place changed, in the order of `indices`; an unchanged order
/// leaves the list shared.
fn rearranged(
    objects: &mut ObjectLayer,
    indices: &[usize],
    arrange: impl FnOnce(&mut Vec<usize>),
) -> ObjectsReport {
    let mut order: Vec<usize> = (0..objects.objects.len()).collect();
    arrange(&mut order);
    let mut now_at = vec![usize::MAX; order.len()];
    for (place, &index) in order.iter().enumerate() {
        if let Some(slot) = now_at.get_mut(index) {
            *slot = place;
        }
    }
    let moved: Vec<ObjectId> = indices
        .iter()
        .filter(|&&index| now_at.get(index) != Some(&index))
        .filter_map(|&index| objects.objects.get(index).map(|o| o.id))
        .collect();
    if moved.is_empty() {
        return ObjectsReport::default();
    }
    let list = Arc::make_mut(&mut objects.objects);
    let mut slots: Vec<Option<MapObject>> = std::mem::take(list).into_iter().map(Some).collect();
    *list = order
        .iter()
        .filter_map(|&index| slots.get_mut(index).and_then(Option::take))
        .collect();
    ObjectsReport { changed: moved }
}

/// The positions in `related` of the places in `indices`, sorted.
fn positions_among(related: &[usize], indices: &[usize]) -> Vec<usize> {
    let selected: HashSet<usize> = indices.iter().copied().collect();
    related
        .iter()
        .enumerate()
        .filter(|(_, index)| selected.contains(index))
        .map(|(position, _)| position)
        .collect()
}

/// The places of the objects sharing space with the selected ones, bottom first, the selected
/// ones included: Tiled's related objects, the shown objects whose shapes meet the selection's.
fn related(objects: &ObjectLayer, space: &ObjectSpace, indices: &[usize]) -> Vec<usize> {
    let selected: HashSet<usize> = indices.iter().copied().collect();
    let outlines: Vec<_> = objects
        .objects
        .iter()
        .map(|object| space.hit_outline(object))
        .collect();
    let chosen: Vec<_> = outlines
        .iter()
        .enumerate()
        .filter(|(index, _)| selected.contains(index))
        .map(|(_, outline)| outline)
        .collect();
    objects
        .objects
        .iter()
        .zip(&outlines)
        .enumerate()
        .filter(|(index, (object, outline))| {
            selected.contains(index)
                || (object.visible && chosen.iter().any(|c| c.touches(outline)))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Where the object first at `index` is now.
fn place_of(order: &[usize], index: usize) -> Option<usize> {
    order.iter().position(|&i| i == index)
}

/// The places, sorted.
fn sorted(indices: &[usize]) -> Vec<usize> {
    let mut places = indices.to_vec();
    places.sort_unstable();
    places
}

/// Places `first..=last` of a list.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Run {
    first: usize,
    last: usize,
}

impl Run {
    /// How many places the run holds.
    fn len(self) -> usize {
        self.last - self.first + 1
    }
}

/// The runs of consecutive places in a sorted list (Tiled's `RangeSet`).
fn runs(sorted: &[usize]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for &place in sorted {
        match runs.last_mut() {
            Some(run) if run.last + 1 == place => run.last = place,
            _ => runs.push(Run {
                first: place,
                last: place,
            }),
        }
    }
    runs
}

/// Tiled's `ObjectGroup::moveObjects`: moves `count` items from `from` to before `to`, a place
/// in the list before the move. Moving onto itself, or out of the list, changes nothing.
fn move_run<T>(list: &mut Vec<T>, from: usize, to: usize, count: usize) {
    let Some(end) = from.checked_add(count) else {
        return;
    };
    if count == 0 || to == from || to == end || end > list.len() || to > list.len() {
        return;
    }
    if to > from && to < end {
        return;
    }
    let moving: Vec<T> = list.drain(from..end).collect();
    let at = if to > from { to - count } else { to };
    list.splice(at..at, moving);
}

#[cfg(test)]
mod tests {
    use bt_core::{Layer, LayerId, LayerKind, Orientation};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;
    use crate::testutil::{assert_valid, ids_of, rect_object};

    fn ids(list: &[u32]) -> Vec<ObjectId> {
        list.iter().map(|&id| ObjectId(id)).collect()
    }

    /// One index-ordered layer of objects 1 to 5: 1, 2 and 4 overlap at the left, 3 and 5 sit
    /// apart at the right.
    fn stack() -> Map {
        let objects = vec![
            rect_object(0.0, 0.0, 10.0, 10.0),
            rect_object(5.0, 5.0, 10.0, 10.0),
            rect_object(100.0, 0.0, 10.0, 10.0),
            rect_object(2.0, 2.0, 4.0, 4.0),
            rect_object(200.0, 0.0, 10.0, 10.0),
        ];
        let layer = Layer::new(
            LayerId(0),
            "stack",
            LayerKind::Object(ObjectLayer {
                draw_order: DrawOrder::Index,
                objects: Arc::new(objects),
                ..ObjectLayer::default()
            }),
        );
        MapBuilder::new(UVec2::splat(16), UVec2::splat(16))
            .layer(layer)
            .build()
            .unwrap()
    }

    fn order(map: &Map) -> Vec<u32> {
        ids_of(map.layers[0].as_object_layer().unwrap())
    }

    #[test]
    fn raising_and_lowering_step_over_the_next_overlapping_object() {
        let mut map = stack();
        // 1 sits under 2 and 4; raising it passes 2 only, not the separate 3.
        let report = raise_objects(&mut map, &ids(&[1])).unwrap();
        assert_eq!(report.changed, ids(&[1]));
        assert_eq!(order(&map), [2, 1, 3, 4, 5]);
        raise_objects(&mut map, &ids(&[1])).unwrap();
        assert_eq!(order(&map), [2, 3, 4, 1, 5], "past 4, the next it overlaps");
        assert!(
            raise_objects(&mut map, &ids(&[1]))
                .unwrap()
                .changed
                .is_empty(),
            "already above everything it overlaps"
        );
        lower_objects(&mut map, &ids(&[1])).unwrap();
        assert_eq!(order(&map), [2, 3, 1, 4, 5], "just below 4");
        lower_objects(&mut map, &ids(&[1])).unwrap();
        assert_eq!(order(&map), [1, 2, 3, 4, 5], "just below 2");
        assert_valid(&map);
    }

    #[test]
    fn runs_rise_and_sink_together() {
        let mut map = stack();
        raise_objects_to_top(&mut map, &ids(&[2, 1, 4])).unwrap();
        assert_eq!(order(&map), [3, 5, 1, 2, 4], "in their order");
        lower_objects_to_bottom(&mut map, &ids(&[2, 4])).unwrap();
        assert_eq!(order(&map), [2, 4, 3, 5, 1]);
        assert!(
            lower_objects_to_bottom(&mut map, &ids(&[2]))
                .unwrap()
                .changed
                .is_empty()
        );
        move_objects_up(&mut map, &ids(&[2, 4, 1])).unwrap();
        assert_eq!(
            order(&map),
            [3, 2, 4, 5, 1],
            "the run past 3; 1 is on top already"
        );
        move_objects_down(&mut map, &ids(&[2, 5])).unwrap();
        assert_eq!(order(&map), [2, 3, 5, 4, 1]);
        assert_valid(&map);
    }

    #[test]
    fn top_down_layers_neither_raise_nor_lower() {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .object_layer(
                "things",
                vec![
                    rect_object(0.0, 0.0, 8.0, 8.0),
                    rect_object(2.0, 2.0, 8.0, 8.0),
                ],
            )
            .build()
            .unwrap();
        assert_eq!(
            raise_objects(&mut map, &ids(&[1])),
            Err(EditError::NotIndexOrdered(LayerId(1)))
        );
        assert_eq!(
            lower_objects_to_bottom(&mut map, &ids(&[2])),
            Err(EditError::NotIndexOrdered(LayerId(1)))
        );
        move_objects_up(&mut map, &ids(&[1])).unwrap();
        assert_eq!(order(&map), [2, 1], "the list still moves");
    }

    #[test]
    fn top_down_order_is_by_screen_height_with_ties_in_list_order() {
        let objects = ObjectLayer {
            objects: Arc::new(vec![
                MapObject {
                    id: ObjectId(1),
                    ..rect_object(0.0, 30.0, 4.0, 4.0)
                },
                MapObject {
                    id: ObjectId(2),
                    ..rect_object(50.0, 10.0, 4.0, 4.0)
                },
                MapObject {
                    id: ObjectId(3),
                    ..rect_object(0.0, 10.0, 4.0, 4.0)
                },
                MapObject {
                    id: ObjectId(4),
                    ..rect_object(9.0, 10.0, 4.0, 4.0)
                },
            ]),
            ..ObjectLayer::default()
        };
        let flat = MapGeometry::new(
            Orientation::Orthogonal,
            glam::IVec2::splat(16),
            glam::IVec2::splat(8),
        );
        let drawn: Vec<u32> = render_order(&objects, &flat)
            .iter()
            .map(|o| o.id.0)
            .collect();
        assert_eq!(drawn, [2, 3, 4, 1], "2, 3 and 4 tie and keep their order");
        // On an isometric map, screen height is x + y in pixels: (0, 30) is lower than
        // (50, 10), and (0, 10) is the highest.
        let iso = MapGeometry::new(
            Orientation::Isometric,
            glam::IVec2::new(32, 16),
            glam::IVec2::splat(8),
        );
        let drawn: Vec<u32> = render_order(&objects, &iso)
            .iter()
            .map(|o| o.id.0)
            .collect();
        assert_eq!(drawn, [3, 4, 1, 2]);
        let listed = ObjectLayer {
            draw_order: DrawOrder::Index,
            ..objects.clone()
        };
        let drawn: Vec<u32> = render_order(&listed, &flat)
            .iter()
            .map(|o| o.id.0)
            .collect();
        assert_eq!(drawn, [1, 2, 3, 4]);
    }

    #[test]
    fn tiled_moves_runs_by_its_rules() {
        let mut list = vec![0, 1, 2, 3, 4];
        move_run(&mut list, 1, 4, 2);
        assert_eq!(list, [0, 3, 1, 2, 4], "up: before the old place 4");
        move_run(&mut list, 2, 0, 2);
        assert_eq!(list, [1, 2, 0, 3, 4]);
        let unchanged = list.clone();
        move_run(&mut list, 1, 2, 2);
        move_run(&mut list, 1, 3, 2);
        move_run(&mut list, 4, 6, 1);
        assert_eq!(list, unchanged, "onto itself, or out of the list");
        assert_eq!(
            runs(&[0, 1, 3, 5, 6, 7]),
            [
                Run { first: 0, last: 1 },
                Run { first: 3, last: 3 },
                Run { first: 5, last: 7 }
            ]
        );
    }

    #[test]
    fn collision_shapes_reorder_in_their_own_list() {
        let mut shapes = ObjectLayer {
            objects: Arc::new(vec![
                MapObject {
                    id: ObjectId(1),
                    ..rect_object(0.0, 0.0, 8.0, 8.0)
                },
                MapObject {
                    id: ObjectId(2),
                    ..rect_object(4.0, 4.0, 8.0, 8.0)
                },
            ]),
            ..ObjectLayer::default()
        };
        let tileset = bt_test_data::fixtures::Atlas::builder()
            .build_embedded()
            .unwrap();
        let space = ObjectSpace::of_tile(&tileset.tileset, bt_core::TileId(0));
        raise_objects_in(&mut shapes, &space, &ids(&[1])).unwrap();
        assert_eq!(
            ids_of(&shapes),
            [2, 1],
            "a collision group's draw order does not matter"
        );
        lower_objects_in(&mut shapes, &space, &ids(&[1])).unwrap();
        assert_eq!(ids_of(&shapes), [1, 2]);
        assert_eq!(
            raise_objects_in(&mut shapes, &space, &ids(&[3])),
            Err(EditError::NoSuchObject(ObjectId(3)))
        );
    }
}
