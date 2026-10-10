//! Editing the nodes of polygons and polylines, as Tiled's Edit Polygons tool does
//! (`reference/tiled/src/tiled/editpolygontool.cpp`: dragging nodes, Split Segment on a
//! double-click, and its menu's Delete Nodes, Join Nodes, Split Segments and Delete Segment, with
//! `SplitPolyline` from `tiled/changepolygon.cpp`), and at the ends of polylines as its Insert
//! Polygon and Polyline tool does (`tiled/createpolygonobjecttool.cpp`: extending a polyline,
//! connecting two, closing one into a polygon).
//!
//! Nodes are named by [`Node`]. A polygon or polyline keeps its points relative to its position,
//! as the model does, and no edit moves the object itself (Tiled's do not either). Positions are
//! pixel space where a node is drawn, the caller having removed layer offsets and parallax, so
//! on a turned object or an isometric map they follow the screen.
//!
//! A polygon keeps at least three nodes and a polyline two: removing nodes from one that would
//! leave it fewer removes the object. (Tiled's Delete Nodes removes an object only when fewer
//! than two nodes remain, so it can leave a polygon of two nodes, which encloses nothing.) A
//! join that would leave a polygon fewer than three nodes leaves it as it is.
//!
//! Every node is checked before anything changes, so an edit that names a node that does not
//! exist changes nothing. Reports list the objects an edit touched, in the order their nodes
//! were given: those whose points changed, those it removed (no longer in the map) and those it
//! made, each after the object it came from.

use std::collections::BTreeSet;
use std::sync::Arc;

use bt_core::{
    Map, MapObject, ObjectId, ObjectLayer, ObjectOverrides, Segment, Shape, closed_segments,
    open_segments,
};
use glam::DVec2;

use crate::error::EditError;
use crate::ids::NewIds;
use crate::report::ObjectsReport;

use super::edit::{remove_objects_in, rewrite_objects_in};
use super::find::{
    distinct, find_object, object_index_in, object_layer_among, object_layer_mut, objects_by_id,
    objects_in, on_layers_holding,
};
use super::node::Node;
use super::space::ObjectSpace;

/// The fewest nodes a polygon keeps.
const POLYGON_NODES: usize = 3;

/// The fewest nodes a polyline keeps.
const POLYLINE_NODES: usize = 2;

/// Moves nodes of `map` by `delta` pixels as they are drawn: on a turned object a node moves
/// as far and the same way on screen as the pointer, not along the object's own axes (Tiled's
/// node drag). Nodes of several objects, on several layers, move together; each node moves
/// once however often it is named. Reports the objects whose points changed.
pub fn move_nodes(map: &mut Map, nodes: &[Node], delta: DVec2) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_by_id(map, &ids)?, nodes)?;
    on_layers_holding(map, &ids, |objects, space, held| {
        move_nodes_in(objects, space, &nodes_on(nodes, held), delta)
    })
}

/// Moves nodes of a list, as [`move_nodes`] does.
pub fn move_nodes_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    nodes: &[Node],
    delta: DVec2,
) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_in(objects, &ids)?, nodes)?;
    if delta == DVec2::ZERO {
        return Ok(ObjectsReport::default());
    }
    let named = named(nodes);
    rewrite_objects_in(objects, &ids, |object| {
        let (Some(indices), Ok(points)) = (indices_of(&named, object.id), points_of(object)) else {
            return object.clone();
        };
        let step = local_delta(space, object, delta);
        let moved = points
            .iter()
            .enumerate()
            .map(|(index, &p)| {
                if indices.contains(&index) {
                    p + step
                } else {
                    p
                }
            })
            .collect();
        with_points(object, moved)
    })
}

/// The point a moved selection of nodes of `map` snaps by: the least x and the least y of where
/// the nodes are drawn, in pixel space (Tiled's align position for nodes). With
/// [`snap_delta`](crate::snap_delta) it gives the snapped move. `None` for no nodes.
pub fn nodes_anchor(map: &Map, nodes: &[Node]) -> Result<Option<DVec2>, EditError> {
    let space = ObjectSpace::of_map(map);
    let found = objects_by_id(map, &objects_of(nodes))?;
    anchor_of(&space, &found, nodes)
}

/// The point a moved selection of nodes of a list snaps by, as [`nodes_anchor`] gives it.
pub fn nodes_anchor_in(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    nodes: &[Node],
) -> Result<Option<DVec2>, EditError> {
    let found = objects_in(objects, &objects_of(nodes))?;
    anchor_of(space, &found, nodes)
}

/// The least corner of where `nodes` are drawn.
fn anchor_of(
    space: &ObjectSpace,
    objects: &[&MapObject],
    nodes: &[Node],
) -> Result<Option<DVec2>, EditError> {
    check_nodes(objects, nodes)?;
    let drawn = nodes.iter().filter_map(|node| {
        let object = objects.iter().find(|o| o.id == node.object)?;
        let point = points_of(object).ok()?.get(node.index)?;
        Some(space.frame_of(object).to_pixels(*point))
    });
    Ok(drawn.reduce(DVec2::min))
}

/// The node of the polygons and polylines of a list with `ids` drawn nearest to `point` (pixel
/// space), when one is within `tolerance` of it on screen: the node an Edit Polygons handle
/// there picks. Other objects among `ids` have no nodes and are passed over; of nodes as near,
/// the first in the order of `ids` and of their points wins.
pub fn node_at(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    point: DVec2,
    tolerance: f64,
) -> Result<Option<Node>, EditError> {
    let screen = space.geometry().pixel_to_screen(point);
    let mut nearest: Option<Picked> = None;
    for object in objects_in(objects, ids)? {
        let Ok(points) = points_of(object) else {
            continue;
        };
        let frame = space.frame_of(object);
        for (index, &p) in points.iter().enumerate() {
            let distance = frame.to_screen(p).distance(screen);
            nearest = nearer(
                nearest,
                Picked {
                    node: Node {
                        object: object.id,
                        index,
                    },
                    distance,
                },
                tolerance,
            );
        }
    }
    Ok(nearest.map(|picked| picked.node))
}

/// The segment of the polygons and polylines of a list with `ids` drawn nearest to `point`
/// (pixel space), when one is within `tolerance` of it on screen: the segment a double-click
/// there splits ([`insert_node`]). A polygon's closing segment counts; a polyline has none.
pub fn segment_at(
    objects: &ObjectLayer,
    space: &ObjectSpace,
    ids: &[ObjectId],
    point: DVec2,
    tolerance: f64,
) -> Result<Option<Node>, EditError> {
    let screen = space.geometry().pixel_to_screen(point);
    let mut nearest: Option<Picked> = None;
    for object in objects_in(objects, ids)? {
        let Ok(points) = points_of(object) else {
            continue;
        };
        let frame = space.frame_of(object);
        let drawn: Vec<DVec2> = points.iter().map(|&p| frame.to_screen(p)).collect();
        let segments: Vec<Segment> = if is_closed(object) {
            closed_segments(&drawn).collect()
        } else {
            open_segments(&drawn).collect()
        };
        for (index, segment) in segments.into_iter().enumerate() {
            nearest = nearer(
                nearest,
                Picked {
                    node: Node {
                        object: object.id,
                        index,
                    },
                    distance: segment.distance_to(screen),
                },
                tolerance,
            );
        }
    }
    Ok(nearest.map(|picked| picked.node))
}

/// A node or segment a point picks, and how far from it it is drawn.
#[derive(Copy, Clone)]
struct Picked {
    node: Node,
    distance: f64,
}

/// The nearer of what was picked so far and a candidate within `tolerance`; the earlier one
/// when they are as near.
fn nearer(best: Option<Picked>, candidate: Picked, tolerance: f64) -> Option<Picked> {
    if candidate.distance > tolerance {
        return best;
    }
    match best {
        Some(best) if best.distance <= candidate.distance => Some(best),
        _ => Some(candidate),
    }
}

/// Inserts a node on the segment of a polygon or polyline of `map` that starts at `segment`,
/// at the point of it nearest to `near` (pixel space), found in the object's own pixels as
/// Tiled's Split Segment finds it. Returns the new node, the one after `segment`.
pub fn insert_node(map: &mut Map, segment: Node, near: DVec2) -> Result<Node, EditError> {
    on_layer_of(map, segment.object, |objects, space| {
        insert_node_in(objects, space, segment, near)
    })
}

/// Inserts a node on a segment of a polygon or polyline of a list, as [`insert_node`] does.
pub fn insert_node_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    segment: Node,
    near: DVec2,
) -> Result<Node, EditError> {
    let object = object_in(objects, segment.object)?;
    let points = points_of(object)?;
    let piece = segment_of(points, segment.index, is_closed(object))
        .ok_or(EditError::NoSuchNode(segment))?;
    let at = piece.nearest_point(space.frame_of(object).from_pixels(near));
    let index = segment.index + 1;
    let mut inserted = points.to_vec();
    inserted.insert(index.min(inserted.len()), at);
    let changed = with_points(object, inserted);
    replace_object_in(objects, changed)?;
    Ok(Node {
        object: segment.object,
        index,
    })
}

/// The segment of `points` starting at `index`: to the next point, or for a closed outline's
/// last point back to the first.
fn segment_of(points: &[DVec2], index: usize, closed: bool) -> Option<Segment> {
    let from = *points.get(index)?;
    let to = match points.get(index + 1) {
        Some(&to) => to,
        None if closed => *points.first()?,
        None => return None,
    };
    Some(Segment { from, to })
}

/// Removes nodes of `map` (Tiled's Delete Nodes). A polygon left with fewer than three nodes,
/// or a polyline with fewer than two, is removed. Reports the objects whose nodes went, the
/// removed ones among them.
pub fn remove_nodes(map: &mut Map, nodes: &[Node]) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_by_id(map, &ids)?, nodes)?;
    on_layers_holding(map, &ids, |objects, _, held| {
        remove_nodes_in(objects, &nodes_on(nodes, held))
    })
}

/// Removes nodes of a list, as [`remove_nodes`] does.
pub fn remove_nodes_in(
    objects: &mut ObjectLayer,
    nodes: &[Node],
) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    let found = objects_in(objects, &ids)?;
    check_nodes(&found, nodes)?;
    let named = named(nodes);
    // The objects that would be left too few nodes go.
    let emptied: Vec<ObjectId> = found
        .iter()
        .filter(|object| {
            let left = points_of(object)
                .map_or(0, |points| points.len())
                .saturating_sub(indices_of(&named, object.id).map_or(0, |indices| indices.len()));
            left < fewest_nodes(object)
        })
        .map(|object| object.id)
        .collect();
    let reshaped = rewrite_objects_in(objects, &ids, |object| {
        let (Some(indices), Ok(points)) = (indices_of(&named, object.id), points_of(object)) else {
            return object.clone();
        };
        if emptied.contains(&object.id) {
            return object.clone();
        }
        let kept = points
            .iter()
            .enumerate()
            .filter(|(index, _)| !indices.contains(index))
            .map(|(_, &p)| p)
            .collect();
        with_points(object, kept)
    })?;
    remove_objects_in(objects, &emptied)?;
    Ok(ObjectsReport {
        changed: ids
            .into_iter()
            .filter(|id| reshaped.changed.contains(id) || emptied.contains(id))
            .collect(),
    })
}

/// Joins nodes of `map` (Tiled's Join Nodes): each run of neighbouring nodes named on an object
/// becomes one node at their average, where the run starts. On a polygon a run through its last
/// node goes on through its first, and joins there. An object of fewer than three nodes, one
/// with all its nodes named, and a polygon the join would leave fewer than three nodes are left
/// as they are. Reports the objects whose points changed.
pub fn join_nodes(map: &mut Map, nodes: &[Node]) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_by_id(map, &ids)?, nodes)?;
    on_layers_holding(map, &ids, |objects, _, held| {
        join_nodes_in(objects, &nodes_on(nodes, held))
    })
}

/// Joins nodes of a list, as [`join_nodes`] does.
pub fn join_nodes_in(
    objects: &mut ObjectLayer,
    nodes: &[Node],
) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_in(objects, &ids)?, nodes)?;
    let named = named(nodes);
    rewrite_objects_in(objects, &ids, |object| {
        let (Some(indices), Ok(points)) = (indices_of(&named, object.id), points_of(object)) else {
            return object.clone();
        };
        let joined = joined(points, &indices, is_closed(object));
        if joined.len() < fewest_nodes(object) {
            return object.clone();
        }
        with_points(object, joined)
    })
}

/// `points` with each run of neighbouring `indices` joined into one point at their average,
/// where the run starts (Tiled's `joinPolygonNodes`).
fn joined(points: &[DVec2], indices: &BTreeSet<usize>, closed: bool) -> Vec<DVec2> {
    let count = points.len();
    if count < 3 || indices.len() >= count {
        return points.to_vec();
    }
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &index in indices {
        match runs.last_mut() {
            Some(run) if run.last() == Some(&(index.wrapping_sub(1))) => run.push(index),
            _ => runs.push(vec![index]),
        }
    }
    // On a polygon the run through the last node goes on with the one from the first.
    let wraps = closed
        && runs.len() > 1
        && runs.first().and_then(|run| run.first()) == Some(&0)
        && runs.last().and_then(|run| run.last()) == Some(&(count - 1));
    if wraps
        && let Some(last) = runs.pop()
        && let Some(first) = runs.first_mut()
    {
        first.extend(last);
    }
    let mut joined = Vec::with_capacity(count);
    for (index, &point) in points.iter().enumerate() {
        match runs.iter().find(|run| run.contains(&index)) {
            None => joined.push(point),
            Some(run) if run.iter().min() == Some(&index) => {
                let sum: DVec2 = run.iter().filter_map(|&i| points.get(i)).copied().sum();
                joined.push(sum / run.len() as f64);
            }
            Some(_) => {}
        }
    }
    joined
}

/// Splits the segments between neighbouring nodes named on each object of `map` (Tiled's
/// Split Segments): a node goes in the middle of each, the closing segment of a polygon
/// included when its first and last nodes are named. Reports the objects whose points changed.
pub fn split_segments(map: &mut Map, nodes: &[Node]) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_by_id(map, &ids)?, nodes)?;
    on_layers_holding(map, &ids, |objects, _, held| {
        split_segments_in(objects, &nodes_on(nodes, held))
    })
}

/// Splits segments of a list, as [`split_segments`] does.
pub fn split_segments_in(
    objects: &mut ObjectLayer,
    nodes: &[Node],
) -> Result<ObjectsReport, EditError> {
    let ids = objects_of(nodes);
    check_nodes(&objects_in(objects, &ids)?, nodes)?;
    let named = named(nodes);
    rewrite_objects_in(objects, &ids, |object| {
        let (Some(indices), Ok(points)) = (indices_of(&named, object.id), points_of(object)) else {
            return object.clone();
        };
        let mut split = Vec::with_capacity(points.len() * 2);
        let mut previous: Option<DVec2> = None;
        for (index, &point) in points.iter().enumerate() {
            if let Some(before) = previous
                && index > 0
                && indices.contains(&index)
                && indices.contains(&(index - 1))
            {
                split.push((before + point) * 0.5);
            }
            split.push(point);
            previous = Some(point);
        }
        let last = points.len().saturating_sub(1);
        if is_closed(object)
            && points.len() > 1
            && indices.contains(&0)
            && indices.contains(&last)
            && let (Some(&first), Some(&end)) = (points.first(), points.last())
        {
            split.push((end + first) * 0.5);
        }
        with_points(object, split)
    })
}

/// Deletes the segment of a polygon or polyline of `map` that starts at `segment` (Tiled's
/// Delete Segment). A polygon opens there into a polyline from the segment's far node round to
/// its near one. A polyline loses the end node of an end segment (and goes when one node would
/// be left); cut in the middle, it splits in two: it keeps the nodes up to the segment, and a
/// copy of it with the map's next id, right after it in its layer, takes the rest. Reports the
/// object, and then the copy.
pub fn delete_segment(map: &mut Map, segment: Node) -> Result<ObjectsReport, EditError> {
    let mut ids = NewIds::from_map(map);
    let report = on_layer_of(map, segment.object, |objects, _| {
        delete_segment_in(objects, segment, &mut ids)
    })?;
    ids.store_in(map);
    Ok(report)
}

/// Deletes a segment of a polygon or polyline of a list, as [`delete_segment`] does, a split
/// polyline's second part taking the next of `ids`.
pub fn delete_segment_in(
    objects: &mut ObjectLayer,
    segment: Node,
    ids: &mut NewIds,
) -> Result<ObjectsReport, EditError> {
    let object = object_in(objects, segment.object)?;
    let points = points_of(object)?;
    let count = points.len();
    segment_of(points, segment.index, is_closed(object)).ok_or(EditError::NoSuchNode(segment))?;
    if is_closed(object) {
        let start = segment.index + 1;
        let opened = match (points.get(start..), points.get(..start)) {
            (Some(after), Some(before)) => [after, before].concat(),
            _ => points.to_vec(),
        };
        let open = reshaped(object, Shape::Polyline(opened));
        return replace_object_in(objects, open);
    }
    if segment.index == 0 {
        return remove_nodes_in(objects, &[segment]);
    }
    if segment.index + 2 == count {
        let last = Node {
            object: segment.object,
            index: count - 1,
        };
        return remove_nodes_in(objects, &[last]);
    }
    let (Some(kept), Some(rest)) = (
        points.get(..=segment.index),
        points.get(segment.index + 1..),
    ) else {
        return Err(EditError::NoSuchNode(segment));
    };
    let first = reshaped(object, Shape::Polyline(kept.to_vec()));
    let mut second = reshaped(object, Shape::Polyline(rest.to_vec()));
    second.id = ids.take()?;
    let at = object_index_in(objects, segment.object)?;
    let report = ObjectsReport {
        changed: vec![first.id, second.id],
    };
    let list = Arc::make_mut(&mut objects.objects);
    if let Some(slot) = list.get_mut(at) {
        *slot = first;
    }
    list.insert((at + 1).min(list.len()), second);
    Ok(report)
}

/// Extends a polyline of `map` from one of its ends (Tiled's Extend Polyline): a node at `at`
/// (pixel space) goes before its first node when `end` is that node, else after its last.
/// Returns the new end node; a node where the end already is changes nothing, and the end
/// itself is returned (Tiled ignores such a click).
pub fn extend_polyline(map: &mut Map, end: Node, at: DVec2) -> Result<Node, EditError> {
    on_layer_of(map, end.object, |objects, space| {
        extend_polyline_in(objects, space, end, at)
    })
}

/// Extends a polyline of a list, as [`extend_polyline`] does.
pub fn extend_polyline_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    end: Node,
    at: DVec2,
) -> Result<Node, EditError> {
    let object = object_in(objects, end.object)?;
    let points = polyline_points(object)?;
    let current = *points.get(end.index).ok_or(EditError::NoSuchNode(end))?;
    check_end(points, end)?;
    let local = space.frame_of(object).from_pixels(at);
    if local == current {
        return Ok(end);
    }
    let (index, extended) = if end.index == 0 {
        (0, [&[local], points].concat())
    } else {
        (points.len(), [points, &[local]].concat())
    };
    let changed = with_points(object, extended);
    replace_object_in(objects, changed)?;
    Ok(Node {
        object: end.object,
        index,
    })
}

/// Connects two polylines of `map` at their ends (Tiled's Connect Polylines): the polyline of
/// `end` takes the other's nodes, joined at the two ends named, keeping its id, position and
/// everything else, and the other is removed (from whichever layer holds it). The other's nodes
/// stay where they are drawn. Naming the two ends of one polyline closes it into a polygon
/// (Tiled's click on the other end), which takes three nodes. Reports the polyline, and then
/// the one removed.
pub fn connect_polylines(
    map: &mut Map,
    end: Node,
    other: Node,
) -> Result<ObjectsReport, EditError> {
    let (connected, kept_on, removed_from) = {
        let space = ObjectSpace::of_map(map);
        let kept = find_object(map, end.object)?;
        let joined = find_object(map, other.object)?;
        let connected = connection(&space, kept.object, end, joined.object, other)?;
        (connected, kept.layer, joined.layer)
    };
    let mut changed = vec![end.object];
    if other.object != end.object {
        remove_objects_in(object_layer_mut(map, removed_from)?, &[other.object])?;
        changed.push(other.object);
    }
    replace_object_in(object_layer_mut(map, kept_on)?, connected)?;
    Ok(ObjectsReport { changed })
}

/// Connects two polylines of a list, as [`connect_polylines`] does.
pub fn connect_polylines_in(
    objects: &mut ObjectLayer,
    space: &ObjectSpace,
    end: Node,
    other: Node,
) -> Result<ObjectsReport, EditError> {
    let kept = object_in(objects, end.object)?;
    let joined = object_in(objects, other.object)?;
    let connected = connection(space, kept, end, joined, other)?;
    let mut changed = vec![end.object];
    if other.object != end.object {
        remove_objects_in(objects, &[other.object])?;
        changed.push(other.object);
    }
    replace_object_in(objects, connected)?;
    Ok(ObjectsReport { changed })
}

/// What the polyline `kept` becomes when its node `end` is connected to the node `other` of
/// `joined` (Tiled's `joinPolygons`): the other's points, in this one's frame, after its last
/// node when `end` is that node, else before its first; reversed as needed so the two named
/// ends meet. When both are the same polyline, it closes into a polygon.
fn connection(
    space: &ObjectSpace,
    kept: &MapObject,
    end: Node,
    joined: &MapObject,
    other: Node,
) -> Result<MapObject, EditError> {
    let points = polyline_points(kept)?;
    let others = polyline_points(joined)?;
    check_end(points, end)?;
    check_end(others, other)?;
    if end.object == other.object {
        if end.index == other.index {
            return Err(EditError::NotAnEnd(other));
        }
        if points.len() < POLYGON_NODES {
            return Err(EditError::TooFewNodes(kept.id));
        }
        return Ok(reshaped(kept, Shape::Polygon(points.to_vec())));
    }
    let from = space.frame_of(joined);
    let to = space.frame_of(kept);
    let mut moved: Vec<DVec2> = others
        .iter()
        .map(|&p| to.from_pixels(from.to_pixels(p)))
        .collect();
    let at_end = end.index + 1 == points.len();
    let other_at_end = other.index + 1 == others.len();
    if at_end == other_at_end {
        moved.reverse();
    }
    let connected = if at_end {
        [points, &moved].concat()
    } else {
        [&moved, points].concat()
    };
    Ok(with_points(kept, connected))
}

/// Checks that `node` is the first or last of `points`.
fn check_end(points: &[DVec2], node: Node) -> Result<(), EditError> {
    if node.index >= points.len() {
        return Err(EditError::NoSuchNode(node));
    }
    if node.index != 0 && node.index + 1 != points.len() {
        return Err(EditError::NotAnEnd(node));
    }
    Ok(())
}

/// The ids of the objects `nodes` are on, each once, in the order first named.
fn objects_of(nodes: &[Node]) -> Vec<ObjectId> {
    let ids: Vec<ObjectId> = nodes.iter().map(|node| node.object).collect();
    distinct(&ids)
}

/// The nodes on the objects with `held` ids.
fn nodes_on(nodes: &[Node], held: &[ObjectId]) -> Vec<Node> {
    nodes
        .iter()
        .copied()
        .filter(|node| held.contains(&node.object))
        .collect()
}

/// The places named on each object, each once and in order.
struct Named {
    object: ObjectId,
    indices: BTreeSet<usize>,
}

/// `nodes` grouped by object.
fn named(nodes: &[Node]) -> Vec<Named> {
    let mut groups: Vec<Named> = Vec::new();
    for node in nodes {
        match groups.iter_mut().find(|group| group.object == node.object) {
            Some(group) => {
                group.indices.insert(node.index);
            }
            None => groups.push(Named {
                object: node.object,
                indices: BTreeSet::from([node.index]),
            }),
        }
    }
    groups
}

/// The places named on the object with `id`.
fn indices_of(named: &[Named], id: ObjectId) -> Option<BTreeSet<usize>> {
    named
        .iter()
        .find(|group| group.object == id)
        .map(|group| group.indices.clone())
}

/// Checks that every node names a point of a polygon or polyline among `objects`.
fn check_nodes(objects: &[&MapObject], nodes: &[Node]) -> Result<(), EditError> {
    for node in nodes {
        let object = objects
            .iter()
            .find(|o| o.id == node.object)
            .ok_or(EditError::NoSuchObject(node.object))?;
        if node.index >= points_of(object)?.len() {
            return Err(EditError::NoSuchNode(*node));
        }
    }
    Ok(())
}

/// The object with `id` in a list.
fn object_in(objects: &ObjectLayer, id: ObjectId) -> Result<&MapObject, EditError> {
    objects_in(objects, &[id])?
        .into_iter()
        .next()
        .ok_or(EditError::NoSuchObject(id))
}

/// The points of a polygon or polyline; `NotAPolygon` for any other object.
fn points_of(object: &MapObject) -> Result<&[DVec2], EditError> {
    match &object.shape {
        Shape::Polygon(points) | Shape::Polyline(points) if object.tile.is_none() => Ok(points),
        _ => Err(EditError::NotAPolygon(object.id)),
    }
}

/// The points of a polyline; `NotAPolyline` for any other object.
fn polyline_points(object: &MapObject) -> Result<&[DVec2], EditError> {
    match &object.shape {
        Shape::Polyline(points) if object.tile.is_none() => Ok(points),
        _ => Err(EditError::NotAPolyline(object.id)),
    }
}

/// Whether the object's outline closes: a polygon's.
fn is_closed(object: &MapObject) -> bool {
    matches!(object.shape, Shape::Polygon(_))
}

/// The fewest nodes the object keeps.
fn fewest_nodes(object: &MapObject) -> usize {
    if is_closed(object) {
        POLYGON_NODES
    } else {
        POLYLINE_NODES
    }
}

/// How far a node of `object` moves in the object's own pixels when it moves by `delta` on
/// screen (both measured in pixel space): the same on an unturned object, turned back on a
/// turned one.
fn local_delta(space: &ObjectSpace, object: &MapObject, delta: DVec2) -> DVec2 {
    if object.rotation == 0.0 {
        return delta;
    }
    let geometry = space.geometry();
    let frame = space.frame_of(object);
    let screen = geometry.pixel_to_screen(delta) - geometry.pixel_to_screen(DVec2::ZERO);
    frame.from_screen(frame.to_screen(DVec2::ZERO) + screen)
}

/// `object` with `points` for its points, its shape's kind kept.
fn with_points(object: &MapObject, points: Vec<DVec2>) -> MapObject {
    let shape = if is_closed(object) {
        Shape::Polygon(points)
    } else {
        Shape::Polyline(points)
    };
    reshaped(object, shape)
}

/// `object` with `shape`, which a template instance marks as its own when it changed.
fn reshaped(object: &MapObject, shape: Shape) -> MapObject {
    let mut changed = MapObject {
        shape,
        ..object.clone()
    };
    if changed.template.is_some() && changed.shape != object.shape {
        changed.overrides |= ObjectOverrides::SHAPE;
    }
    changed
}

/// Puts `object` in place of the object with its id in a list. Reports it when it changed.
fn replace_object_in(
    objects: &mut ObjectLayer,
    object: MapObject,
) -> Result<ObjectsReport, EditError> {
    let id = object.id;
    rewrite_objects_in(objects, &[id], |_| object.clone())
}

/// Runs a list edit on the object layer of `map` holding the object with `id`, in the map's
/// space.
fn on_layer_of<T>(
    map: &mut Map,
    id: ObjectId,
    edit: impl FnOnce(&mut ObjectLayer, &ObjectSpace) -> Result<T, EditError>,
) -> Result<T, EditError> {
    let layer = find_object(map, id)?.layer;
    let space = ObjectSpace::from_parts(map.geometry(), &map.tilesets);
    edit(object_layer_among(&mut map.layers, layer)?, &space)
}

#[cfg(test)]
mod tests {
    use bt_core::{FileRef, Orientation, TileId};
    use bt_test_data::Fuzz;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use bt_utils::rng::Rng;
    use glam::{IVec2, UVec2};

    use super::*;
    use crate::testutil::{assert_near, assert_valid, ids_of, rect_object};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn node(object: u32, index: usize) -> Node {
        Node {
            object: ObjectId(object),
            index,
        }
    }

    fn shaped(shape: Shape, x: f64, y: f64) -> MapObject {
        MapObject {
            shape,
            ..rect_object(x, y, 0.0, 0.0)
        }
    }

    /// A square polygon (1), a straight polyline of five nodes (2) and a rectangle (3) on one
    /// layer, and a polyline of two nodes (4) on another.
    fn sample() -> Map {
        let square = vec![v(0.0, 0.0), v(20.0, 0.0), v(20.0, 20.0), v(0.0, 20.0)];
        let line = (0..5).map(|i| v(f64::from(i) * 10.0, 0.0)).collect();
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "shapes",
                vec![
                    shaped(Shape::Polygon(square), 10.0, 10.0),
                    shaped(Shape::Polyline(line), 100.0, 0.0),
                    rect_object(0.0, 50.0, 10.0, 10.0),
                ],
            )
            .object_layer(
                "more",
                vec![shaped(
                    Shape::Polyline(vec![v(0.0, 0.0), v(0.0, 10.0)]),
                    0.0,
                    100.0,
                )],
            )
            .build()
            .unwrap()
    }

    fn object(map: &Map, id: u32) -> MapObject {
        find_object(map, ObjectId(id)).unwrap().object.clone()
    }

    fn points(map: &Map, id: u32) -> Vec<DVec2> {
        points_of(&object(map, id)).unwrap().to_vec()
    }

    fn exists(map: &Map, id: u32) -> bool {
        find_object(map, ObjectId(id)).is_ok()
    }

    #[track_caller]
    fn assert_points_near(actual: &[DVec2], expected: &[DVec2]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert_near(*a, *e);
        }
    }

    #[test]
    fn nodes_move_as_dragged() {
        let mut map = sample();
        let named = [node(1, 1), node(1, 2), node(4, 0), node(1, 1)];
        let report = move_nodes(&mut map, &named, v(5.0, -2.0)).unwrap();
        assert_eq!(report.changed, [ObjectId(1), ObjectId(4)]);
        assert_eq!(
            points(&map, 1),
            [v(0.0, 0.0), v(25.0, -2.0), v(25.0, 18.0), v(0.0, 20.0)],
            "each node once"
        );
        assert_eq!(points(&map, 4), [v(5.0, -2.0), v(0.0, 10.0)]);
        assert_eq!(object(&map, 1).pos, v(10.0, 10.0), "the object stays");
        assert_valid(&map);
        assert!(
            move_nodes(&mut map, &[node(1, 0)], DVec2::ZERO)
                .unwrap()
                .changed
                .is_empty()
        );
        let before = map.clone();
        assert_eq!(
            move_nodes(&mut map, &[node(1, 0), node(1, 4)], v(1.0, 1.0)),
            Err(EditError::NoSuchNode(node(1, 4)))
        );
        assert_eq!(
            move_nodes(&mut map, &[node(3, 0)], v(1.0, 1.0)),
            Err(EditError::NotAPolygon(ObjectId(3)))
        );
        assert_eq!(
            move_nodes(&mut map, &[node(4, 0), node(9, 0)], v(1.0, 1.0)),
            Err(EditError::NoSuchObject(ObjectId(9)))
        );
        assert_eq!(map, before, "nothing moves when a node is wrong");
    }

    #[test]
    fn nodes_of_turned_objects_follow_the_screen() {
        let map = sample();
        let space = ObjectSpace::of_map(&map);
        // A polyline turned a quarter clockwise is drawn downwards from its position.
        let mut list = ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(1),
                rotation: 90.0,
                ..shaped(Shape::Polyline(vec![v(0.0, 0.0), v(10.0, 0.0)]), 0.0, 0.0)
            }]),
            ..ObjectLayer::default()
        };
        move_nodes_in(&mut list, &space, &[node(1, 1)], v(5.0, 0.0)).unwrap();
        let moved = list.objects[0].clone();
        assert_points_near(points_of(&moved).unwrap(), &[v(0.0, 0.0), v(10.0, -5.0)]);
        assert_near(
            space.frame_of(&moved).to_pixels(v(10.0, -5.0)),
            v(5.0, 10.0),
        );
        // A double-click beside the drawn line puts a node on it under the pointer.
        assert_eq!(
            segment_at(&list, &space, &[ObjectId(1)], v(3.0, 5.0), 2.0),
            Ok(Some(node(1, 0)))
        );
        let inserted = insert_node_in(&mut list, &space, node(1, 0), v(2.5, 5.0)).unwrap();
        assert_eq!(inserted, node(1, 1));
        let drawn = space
            .frame_of(&list.objects[0])
            .to_pixels(points_of(&list.objects[0]).unwrap()[1]);
        assert_near(drawn, v(2.5, 5.0));
        // On an isometric map, nodes of an unturned shape move by the pixels given.
        let iso = MapBuilder::new(UVec2::new(64, 32), UVec2::splat(8))
            .orientation(Orientation::Isometric)
            .build()
            .unwrap();
        let space = ObjectSpace::of_map(&iso);
        move_nodes_in(&mut list, &space, &[node(1, 0)], v(3.0, 4.0)).unwrap();
        assert_near(
            points_of(&list.objects[0]).unwrap()[0],
            rotated_back(v(3.0, 4.0)),
        );
    }

    /// `v` in the own pixels of an object turned a quarter clockwise on an isometric grid of
    /// 64 × 32 tiles: the move on screen turned back.
    fn rotated_back(v: DVec2) -> DVec2 {
        let iso = MapBuilder::new(UVec2::new(64, 32), UVec2::splat(8))
            .orientation(Orientation::Isometric)
            .build()
            .unwrap();
        let geometry = iso.geometry();
        let screen = geometry.pixel_to_screen(v) - geometry.pixel_to_screen(DVec2::ZERO);
        let back = bt_core::rotated(screen, -90.0);
        geometry.screen_to_pixel(geometry.pixel_to_screen(DVec2::ZERO) + back)
    }

    #[test]
    fn moved_nodes_snap_by_their_least_corner() {
        let map = sample();
        // Node 2 of the square is drawn at (30, 30), node 1 of the short line at (0, 110).
        assert_eq!(
            nodes_anchor(&map, &[node(1, 2), node(4, 1)]),
            Ok(Some(v(0.0, 30.0)))
        );
        assert_eq!(nodes_anchor(&map, &[]), Ok(None));
        assert_eq!(
            nodes_anchor(&map, &[node(4, 2)]),
            Err(EditError::NoSuchNode(node(4, 2)))
        );
        let layer = map.layers[0].as_object_layer().unwrap();
        assert_eq!(
            nodes_anchor_in(layer, &ObjectSpace::of_map(&map), &[node(2, 3)]),
            Ok(Some(v(130.0, 0.0)))
        );
    }

    #[test]
    fn handles_and_segments_are_picked_on_screen() {
        let map = sample();
        let space = ObjectSpace::of_map(&map);
        let layer = map.layers[0].as_object_layer().unwrap();
        let all = [ObjectId(1), ObjectId(2), ObjectId(3)];
        // The square is drawn from (10, 10) to (30, 30).
        assert_eq!(
            node_at(layer, &space, &all, v(29.0, 29.0), 2.0),
            Ok(Some(node(1, 2)))
        );
        assert_eq!(node_at(layer, &space, &all, v(20.0, 20.0), 2.0), Ok(None));
        assert_eq!(
            segment_at(layer, &space, &all, v(31.0, 20.0), 2.0),
            Ok(Some(node(1, 1)))
        );
        assert_eq!(
            segment_at(layer, &space, &all, v(9.5, 15.0), 2.0),
            Ok(Some(node(1, 3))),
            "the closing side"
        );
        assert_eq!(
            segment_at(layer, &space, &all, v(5.0, 55.0), 2.0),
            Ok(None),
            "a rectangle has no segments"
        );
        assert_eq!(
            segment_at(layer, &space, &[ObjectId(2)], v(135.0, 1.0), 2.0),
            Ok(Some(node(2, 3)))
        );
        assert_eq!(
            segment_at(layer, &space, &[ObjectId(7)], v(0.0, 0.0), 2.0),
            Err(EditError::NoSuchObject(ObjectId(7)))
        );
    }

    #[test]
    fn a_double_click_splits_the_segment_under_it() {
        let mut map = sample();
        assert_eq!(
            insert_node(&mut map, node(1, 1), v(31.0, 20.0)),
            Ok(node(1, 2))
        );
        assert_eq!(
            points(&map, 1),
            [
                v(0.0, 0.0),
                v(20.0, 0.0),
                v(20.0, 10.0),
                v(20.0, 20.0),
                v(0.0, 20.0)
            ]
        );
        // The closing side of a polygon takes its node at the end.
        assert_eq!(
            insert_node(&mut map, node(1, 4), v(9.0, 25.0)),
            Ok(node(1, 5))
        );
        assert_eq!(points(&map, 1).last(), Some(&v(0.0, 15.0)));
        // Past an end the node goes on the end.
        assert_eq!(
            insert_node(&mut map, node(2, 0), v(90.0, 3.0)),
            Ok(node(2, 1))
        );
        assert_eq!(points(&map, 2)[1], v(0.0, 0.0));
        // A polyline's last node starts no segment.
        assert_eq!(
            insert_node(&mut map, node(2, 5), v(0.0, 0.0)),
            Err(EditError::NoSuchNode(node(2, 5)))
        );
        assert_eq!(
            insert_node(&mut map, node(3, 0), v(0.0, 0.0)),
            Err(EditError::NotAPolygon(ObjectId(3)))
        );
        assert_valid(&map);
    }

    #[test]
    fn removed_nodes_leave_enough_or_take_the_object() {
        let mut map = sample();
        let report = remove_nodes(&mut map, &[node(1, 1), node(2, 0), node(2, 4)]).unwrap();
        assert_eq!(report.changed, [ObjectId(1), ObjectId(2)]);
        assert_eq!(points(&map, 1), [v(0.0, 0.0), v(20.0, 20.0), v(0.0, 20.0)]);
        assert_eq!(points(&map, 2), [v(10.0, 0.0), v(20.0, 0.0), v(30.0, 0.0)]);
        // A polygon left fewer than three nodes goes, and a polyline left fewer than two.
        let report = remove_nodes(&mut map, &[node(1, 0), node(4, 1), node(2, 2)]).unwrap();
        assert_eq!(report.changed, [ObjectId(1), ObjectId(4), ObjectId(2)]);
        assert!(!exists(&map, 1) && !exists(&map, 4));
        assert_eq!(points(&map, 2), [v(10.0, 0.0), v(20.0, 0.0)]);
        assert_valid(&map);
        let before = map.clone();
        assert_eq!(
            remove_nodes(&mut map, &[node(2, 0), node(2, 2)]),
            Err(EditError::NoSuchNode(node(2, 2)))
        );
        assert_eq!(map, before);
    }

    #[test]
    fn joined_nodes_meet_at_their_average() {
        let line = [
            v(0.0, 0.0),
            v(10.0, 0.0),
            v(20.0, 0.0),
            v(30.0, 0.0),
            v(40.0, 0.0),
        ];
        let set = |indices: &[usize]| indices.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            joined(&line, &set(&[1, 2]), false),
            [v(0.0, 0.0), v(15.0, 0.0), v(30.0, 0.0), v(40.0, 0.0)]
        );
        assert_eq!(
            joined(&line, &set(&[0, 1, 3, 4]), false),
            [v(5.0, 0.0), v(20.0, 0.0), v(35.0, 0.0)],
            "each run on its own"
        );
        assert_eq!(
            joined(&line, &set(&[0, 1, 4]), true),
            [v(50.0 / 3.0, 0.0), v(20.0, 0.0), v(30.0, 0.0)],
            "on a polygon the last node's run goes on through the first, and joins there"
        );
        assert_eq!(joined(&line, &set(&[0, 1, 2, 3, 4]), true), line, "all");
        assert_eq!(joined(&line, &set(&[2]), false), line, "one node");
        assert_eq!(
            joined(&line[..2], &set(&[0, 1]), false),
            line[..2],
            "too few"
        );
        let mut map = sample();
        assert!(
            join_nodes(&mut map, &[node(1, 0), node(1, 1), node(1, 3)])
                .unwrap()
                .changed
                .is_empty(),
            "a polygon of two nodes would be left"
        );
        let report =
            join_nodes(&mut map, &[node(1, 0), node(1, 1), node(2, 1), node(2, 2)]).unwrap();
        assert_eq!(report.changed, [ObjectId(1), ObjectId(2)]);
        assert_eq!(points(&map, 1), [v(10.0, 0.0), v(20.0, 20.0), v(0.0, 20.0)]);
        assert_eq!(
            points(&map, 2),
            [v(0.0, 0.0), v(15.0, 0.0), v(30.0, 0.0), v(40.0, 0.0)]
        );
        assert_valid(&map);
    }

    #[test]
    fn split_segments_take_a_node_between_named_neighbours() {
        let mut map = sample();
        let named = [node(2, 0), node(2, 1), node(2, 2), node(1, 0), node(1, 3)];
        let report = split_segments(&mut map, &named).unwrap();
        assert_eq!(report.changed, [ObjectId(2), ObjectId(1)]);
        assert_eq!(
            points(&map, 2),
            [
                v(0.0, 0.0),
                v(5.0, 0.0),
                v(10.0, 0.0),
                v(15.0, 0.0),
                v(20.0, 0.0),
                v(30.0, 0.0),
                v(40.0, 0.0)
            ]
        );
        // The square's first and last nodes are neighbours across its closing side.
        assert_eq!(
            points(&map, 1),
            [
                v(0.0, 0.0),
                v(20.0, 0.0),
                v(20.0, 20.0),
                v(0.0, 20.0),
                v(0.0, 10.0)
            ]
        );
        assert!(
            split_segments(&mut map, &[node(1, 0), node(1, 2)])
                .unwrap()
                .changed
                .is_empty(),
            "nodes that are not neighbours split nothing"
        );
    }

    #[test]
    fn deleted_segments_open_polygons_and_split_polylines() {
        let mut map = sample();
        // The square opens at its second side, from that side's far corner round to its near one.
        assert_eq!(
            delete_segment(&mut map, node(1, 1)).unwrap().changed,
            [ObjectId(1)]
        );
        assert_eq!(
            object(&map, 1).shape,
            Shape::Polyline(vec![v(20.0, 20.0), v(0.0, 20.0), v(0.0, 0.0), v(20.0, 0.0)])
        );
        // Cut in the middle, a polyline splits: a copy with the next id, after it, takes the rest.
        let next = map.next_object_id;
        let report = delete_segment(&mut map, node(2, 1)).unwrap();
        assert_eq!(report.changed, [ObjectId(2), ObjectId(next)]);
        assert_eq!(points(&map, 2), [v(0.0, 0.0), v(10.0, 0.0)]);
        assert_eq!(
            points(&map, next),
            [v(20.0, 0.0), v(30.0, 0.0), v(40.0, 0.0)]
        );
        assert_eq!(object(&map, next).pos, v(100.0, 0.0));
        assert_eq!(map.next_object_id, next + 1);
        assert_eq!(
            ids_of(map.layers[0].as_object_layer().unwrap()),
            [1, 2, next, 3]
        );
        assert_valid(&map);
        // An end segment takes its end node with it; a polyline of two goes.
        delete_segment(&mut map, node(next, 1)).unwrap();
        assert_eq!(points(&map, next), [v(20.0, 0.0), v(30.0, 0.0)]);
        delete_segment(&mut map, node(next, 0)).unwrap();
        assert!(!exists(&map, next));
        // A polygon opened at its closing side keeps its nodes in order.
        let mut fresh = sample();
        delete_segment(&mut fresh, node(1, 3)).unwrap();
        assert_eq!(
            object(&fresh, 1).shape,
            Shape::Polyline(vec![v(0.0, 0.0), v(20.0, 0.0), v(20.0, 20.0), v(0.0, 20.0)])
        );
        assert_eq!(
            delete_segment(&mut fresh, node(2, 4)),
            Err(EditError::NoSuchNode(node(2, 4))),
            "a polyline's last node starts no segment"
        );
    }

    #[test]
    fn template_instances_mark_their_points_as_their_own() {
        let mut map = sample();
        crate::objects::find::object_mut(&mut map, ObjectId(2))
            .unwrap()
            .template = Some(FileRef::new("line.tx", "/line.tx"));
        let next = map.next_object_id;
        delete_segment(&mut map, node(2, 2)).unwrap();
        assert_eq!(object(&map, 2).overrides, ObjectOverrides::SHAPE);
        assert_eq!(object(&map, next).overrides, ObjectOverrides::SHAPE);
        assert_eq!(object(&map, next).template, object(&map, 2).template);
        move_nodes(&mut map, &[node(1, 0)], v(1.0, 0.0)).unwrap();
        assert!(object(&map, 1).overrides.is_empty(), "not an instance");
    }

    #[test]
    fn polylines_extend_from_either_end() {
        let mut map = sample();
        assert_eq!(
            extend_polyline(&mut map, node(4, 1), v(5.0, 120.0)),
            Ok(node(4, 2))
        );
        assert_eq!(points(&map, 4), [v(0.0, 0.0), v(0.0, 10.0), v(5.0, 20.0)]);
        assert_eq!(
            extend_polyline(&mut map, node(4, 0), v(-5.0, 95.0)),
            Ok(node(4, 0))
        );
        assert_eq!(
            points(&map, 4),
            [v(-5.0, -5.0), v(0.0, 0.0), v(0.0, 10.0), v(5.0, 20.0)]
        );
        let before = map.clone();
        assert_eq!(
            extend_polyline(&mut map, node(4, 3), v(5.0, 120.0)),
            Ok(node(4, 3)),
            "a click on the end itself"
        );
        assert_eq!(map, before);
        assert_eq!(
            extend_polyline(&mut map, node(4, 1), v(0.0, 0.0)),
            Err(EditError::NotAnEnd(node(4, 1)))
        );
        assert_eq!(
            extend_polyline(&mut map, node(1, 0), v(0.0, 0.0)),
            Err(EditError::NotAPolyline(ObjectId(1)))
        );
        assert_eq!(
            extend_polyline(&mut map, node(4, 9), v(0.0, 0.0)),
            Err(EditError::NoSuchNode(node(4, 9)))
        );
        assert_valid(&map);
    }

    #[test]
    fn polylines_connect_at_the_ends_named() {
        let map = sample();
        let space = ObjectSpace::of_map(&map);
        // b is turned a quarter clockwise: it is drawn from (100, 0) to (90, 0).
        let list = || ObjectLayer {
            objects: Arc::new(vec![
                MapObject {
                    id: ObjectId(1),
                    ..shaped(Shape::Polyline(vec![v(0.0, 0.0), v(10.0, 0.0)]), 0.0, 0.0)
                },
                MapObject {
                    id: ObjectId(2),
                    rotation: 90.0,
                    ..shaped(Shape::Polyline(vec![v(0.0, 0.0), v(0.0, 10.0)]), 100.0, 0.0)
                },
            ]),
            ..ObjectLayer::default()
        };
        let cases = [
            (
                1,
                0,
                [v(0.0, 0.0), v(10.0, 0.0), v(100.0, 0.0), v(90.0, 0.0)],
            ),
            (
                1,
                1,
                [v(0.0, 0.0), v(10.0, 0.0), v(90.0, 0.0), v(100.0, 0.0)],
            ),
            (
                0,
                1,
                [v(100.0, 0.0), v(90.0, 0.0), v(0.0, 0.0), v(10.0, 0.0)],
            ),
            (
                0,
                0,
                [v(90.0, 0.0), v(100.0, 0.0), v(0.0, 0.0), v(10.0, 0.0)],
            ),
        ];
        for (end, other, expected) in cases {
            let mut objects = list();
            let report =
                connect_polylines_in(&mut objects, &space, node(1, end), node(2, other)).unwrap();
            assert_eq!(report.changed, [ObjectId(1), ObjectId(2)]);
            assert_eq!(ids_of(&objects), [1]);
            assert_points_near(points_of(&objects.objects[0]).unwrap(), &expected);
        }
        let mut objects = list();
        assert_eq!(
            connect_polylines_in(&mut objects, &space, node(1, 1), node(1, 0)),
            Err(EditError::TooFewNodes(ObjectId(1))),
            "two nodes cannot close"
        );
        assert_eq!(
            connect_polylines_in(&mut objects, &space, node(1, 1), node(1, 1)),
            Err(EditError::NotAnEnd(node(1, 1)))
        );
    }

    #[test]
    fn polylines_connect_across_layers_and_close() {
        let mut map = sample();
        // The long line's end (drawn at (140, 0)) takes the short line from its start.
        let report = connect_polylines(&mut map, node(2, 4), node(4, 0)).unwrap();
        assert_eq!(report.changed, [ObjectId(2), ObjectId(4)]);
        assert!(!exists(&map, 4));
        assert_eq!(points(&map, 2).len(), 7);
        assert_eq!(points(&map, 2)[5..], [v(-100.0, 100.0), v(-100.0, 110.0)]);
        assert_valid(&map);
        // The two ends of one polyline close it into a polygon.
        let report = connect_polylines(&mut map, node(2, 6), node(2, 0)).unwrap();
        assert_eq!(report.changed, [ObjectId(2)]);
        assert!(matches!(object(&map, 2).shape, Shape::Polygon(ref p) if p.len() == 7));
        let before = map.clone();
        assert_eq!(
            connect_polylines(&mut map, node(2, 0), node(1, 0)),
            Err(EditError::NotAPolyline(ObjectId(2)))
        );
        assert_eq!(map, before);
        let mut fresh = sample();
        assert_eq!(
            connect_polylines(&mut fresh, node(2, 2), node(4, 0)),
            Err(EditError::NotAnEnd(node(2, 2)))
        );
        assert_eq!(fresh, sample());
    }

    #[test]
    fn a_tiles_collision_shapes_edit_their_nodes_in_its_space() {
        let atlas = Atlas::builder().tile_size(32, 32).build_embedded().unwrap();
        let space = ObjectSpace::of_tile(&atlas.tileset, TileId(0));
        let mut shapes = ObjectLayer {
            objects: Arc::new(vec![MapObject {
                id: ObjectId(3),
                ..shaped(
                    Shape::Polyline(vec![v(0.0, 0.0), v(10.0, 0.0), v(20.0, 0.0), v(30.0, 0.0)]),
                    1.0,
                    1.0,
                )
            }]),
            ..ObjectLayer::default()
        };
        let mut ids = NewIds::after_highest(&shapes);
        let report = delete_segment_in(&mut shapes, node(3, 1), &mut ids).unwrap();
        assert_eq!(report.changed, [ObjectId(3), ObjectId(4)]);
        assert_eq!(ids_of(&shapes), [3, 4]);
        move_nodes_in(&mut shapes, &space, &[node(4, 0)], v(0.0, 5.0)).unwrap();
        assert_eq!(
            points_of(&shapes.objects[1]).unwrap(),
            [v(20.0, 5.0), v(30.0, 0.0)]
        );
    }

    #[test]
    fn random_node_edits_keep_enough_nodes_and_the_map_valid() {
        let fuzz = Fuzz::from_env(0x0D0D_E5ED, 200);
        let mut refused = 0;
        let mut edited = 0;
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let random_points = |rng: &mut Rng| -> Vec<DVec2> {
                (0..1 + rng.below(6).unwrap())
                    .map(|_| v(rng.next_f64() * 60.0 - 30.0, rng.next_f64() * 60.0 - 30.0))
                    .collect()
            };
            let shapes: Vec<MapObject> = (0..4)
                .map(|_| {
                    let points = random_points(&mut rng);
                    let shape = if rng.below(2) == Some(0) {
                        Shape::Polygon(points)
                    } else {
                        Shape::Polyline(points)
                    };
                    MapObject {
                        rotation: if rng.below(2) == Some(0) { 0.0 } else { 30.0 },
                        ..shaped(shape, rng.next_f64() * 100.0, rng.next_f64() * 100.0)
                    }
                })
                .collect();
            let (first, second) = shapes.split_at(2);
            let mut map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(8))
                .orientation(if rng.below(2) == Some(0) {
                    Orientation::Orthogonal
                } else {
                    Orientation::Isometric
                })
                .skew(IVec2::ZERO)
                .object_layer("a", first.to_vec())
                .object_layer("b", second.to_vec())
                .build()
                .unwrap();
            for step in 0..10 {
                let mut ids = Vec::new();
                map.for_each_layer(|layer, _| {
                    if let Some(objects) = layer.as_object_layer() {
                        ids.extend(objects.objects.iter().map(|o| o.id));
                    }
                });
                if ids.is_empty() {
                    break;
                }
                let pick = |rng: &mut Rng| Node {
                    object: ids[rng.below(ids.len() as u64).unwrap() as usize],
                    index: rng.below(7).unwrap() as usize,
                };
                let nodes: Vec<Node> = (0..1 + rng.below(4).unwrap())
                    .map(|_| pick(&mut rng))
                    .collect();
                let other = pick(&mut rng);
                let at = v(rng.next_f64() * 100.0, rng.next_f64() * 100.0);
                let before = map.clone();
                let result = match rng.below(8).unwrap() {
                    0 => move_nodes(&mut map, &nodes, at - v(50.0, 50.0)),
                    1 => remove_nodes(&mut map, &nodes),
                    2 => join_nodes(&mut map, &nodes),
                    3 => split_segments(&mut map, &nodes),
                    4 => delete_segment(&mut map, nodes[0]),
                    5 => insert_node(&mut map, nodes[0], at).map(|_| ObjectsReport::default()),
                    6 => extend_polyline(&mut map, nodes[0], at).map(|_| ObjectsReport::default()),
                    _ => connect_polylines(&mut map, nodes[0], other),
                };
                let context = format!(
                    "case {case} ({}), step {step}: {result:?}",
                    fuzz.reproduce()
                );
                if result.is_err() {
                    refused += 1;
                    assert_eq!(map, before, "a refused edit changed the map: {context}");
                } else if map != before {
                    edited += 1;
                }
                assert!(map.validate().is_empty(), "{:?}: {context}", map.validate());
                // Nothing that had enough nodes is left with too few.
                map.for_each_layer(|layer, _| {
                    for object in layer.as_object_layer().map_or(&[][..], |o| &o.objects[..]) {
                        let had = before
                            .object(object.id)
                            .and_then(|o| points_of(o).ok().map(<[DVec2]>::len));
                        let has = points_of(object).map_or(0, <[DVec2]>::len);
                        if had.is_some_and(|had| had >= fewest_nodes(object)) {
                            assert!(has >= fewest_nodes(object), "{object:?}: {context}");
                        }
                    }
                });
            }
        }
        assert!(refused > 0, "some edits should be refused");
        assert!(edited > fuzz.cases, "only {edited} edits changed anything");
    }
}
