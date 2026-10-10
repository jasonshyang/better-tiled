//! Arranging layers: removing them, dragging them into place (into and out of groups), raising
//! and lowering them a step, grouping and ungrouping them (Tiled's `MapDocument::removeLayers`,
//! `moveLayersUp`, `moveLayersDown`, `groupLayers` and `ungroupLayers`,
//! `reference/tiled/src/tiled/mapdocument.cpp`; `MoveLayer`, `tiled/movelayer.cpp`;
//! `ReparentLayers`, `tiled/reparentlayers.cpp`; `LayerModel::dropMimeData`,
//! `tiled/layermodel.cpp`).

use std::collections::VecDeque;

use bt_core::{Layer, LayerId, LayerKind, Map};

use super::create::numbered_name;
use super::tree::{
    LayerLocation, check_layers, children, distinct, holds_or_is, in_layer_order, insert_layer,
    is_group, locate, reparent_layers, take_layer, tree_shape,
};
use crate::error::EditError;
use crate::ids::take_layer_id;
use crate::report::LayersReport;

/// Remove Layers: takes the layers out of the map, a group with everything in it. Reports the
/// layers removed; a layer inside a removed group goes with it and is not listed.
pub fn remove_layers(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let mut report = LayersReport::default();
    for id in distinct(ids) {
        // Gone with a group removed before it.
        if map.layer(id).is_none() {
            continue;
        }
        take_layer(map, id)?;
        report.removed.push(id);
    }
    Ok(report)
}

/// Drag and drop: moves the layers into the group `parent` (the map's top level for `None`) at
/// `index`, counted among the group's layers before the move, bottom first. The layers go one
/// after the other in Tiled's layer order, so they keep the order they are drawn in, wherever
/// they came from (Tiled's `ReparentLayers`). A group cannot go into itself or into a group
/// inside it. Reports the layers moved; nothing when they were there already.
pub fn move_layers(
    map: &mut Map,
    ids: &[LayerId],
    parent: Option<LayerId>,
    index: usize,
) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let places = children(map, parent)?.len();
    if index > places {
        return Err(EditError::NoSuchPlace { parent, index });
    }
    if let Some(target) = parent
        && let Some(outer) = ids.iter().find(|id| holds_or_is(map, **id, target))
    {
        return Err(EditError::IntoItself(*outer));
    }
    let before = tree_shape(map);
    let moved = reparent_layers(map, ids, parent, index)?;
    if tree_shape(map) == before {
        return Ok(LayersReport::default());
    }
    Ok(LayersReport {
        changed: moved,
        ..LayersReport::default()
    })
}

/// Which way [`raise_layers`] and [`lower_layers`] step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Step {
    Up,
    Down,
}

/// Raise Layers: each layer moves one step up, topmost first: past the layer above it, into
/// the bottom of a group above it, or out of its group to just above it (Tiled's `MoveLayer`).
/// When one of them is the topmost layer of the map none moves, as in Tiled, and the report is
/// empty.
pub fn raise_layers(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let mut order = in_layer_order(map, ids);
    order.reverse();
    step_layers(map, order, Step::Up)
}

/// Lower Layers: each layer moves one step down, bottommost first: past the layer below it, into
/// the top of a group below it, or out of its group to just below it. When one of them is the
/// bottom layer of the map none moves, and the report is empty.
pub fn lower_layers(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let order = in_layer_order(map, ids);
    step_layers(map, order, Step::Down)
}

/// Moves each layer of `order` one step, unless one of them cannot move.
fn step_layers(map: &mut Map, order: Vec<LayerId>, step: Step) -> Result<LayersReport, EditError> {
    for &id in &order {
        if !can_step(map, id, step)? {
            return Ok(LayersReport::default());
        }
    }
    for &id in &order {
        step_layer(map, id, step)?;
    }
    Ok(LayersReport {
        changed: order,
        ..LayersReport::default()
    })
}

/// Whether the layer can move a step: it is in a group, or not at the end of the map's layers
/// (Tiled's `MoveLayer::canMoveUp` and `canMoveDown`).
fn can_step(map: &Map, id: LayerId, step: Step) -> Result<bool, EditError> {
    let at = locate(map, id)?;
    if at.parent.is_some() {
        return Ok(true);
    }
    Ok(match step {
        Step::Up => at.index + 1 < map.layers.len(),
        Step::Down => at.index > 0,
    })
}

/// One step of Tiled's `MoveLayer`.
fn step_layer(map: &mut Map, id: LayerId, step: Step) -> Result<(), EditError> {
    let at = locate(map, id)?;
    let siblings = children(map, at.parent)?;
    let neighbour = match step {
        Step::Up => siblings.get(at.index + 1),
        Step::Down => at.index.checked_sub(1).and_then(|i| siblings.get(i)),
    };
    let to = match neighbour {
        // Into a group next to it, at the end that faces the layer.
        Some(Layer {
            id: group,
            kind: LayerKind::Group(layers),
            ..
        }) => LayerLocation {
            parent: Some(*group),
            index: match step {
                Step::Up => 0,
                Step::Down => layers.len(),
            },
        },
        // Past the layer next to it: once the layer is out, that layer is where it was.
        Some(_) => LayerLocation {
            parent: at.parent,
            index: match step {
                Step::Up => at.index + 1,
                Step::Down => at.index - 1,
            },
        },
        // Out of its group, next to the group.
        None => {
            let Some(group) = at.parent else {
                return Ok(());
            };
            let outer = locate(map, group)?;
            LayerLocation {
                parent: outer.parent,
                index: match step {
                    Step::Up => outer.index + 1,
                    Step::Down => outer.index,
                },
            }
        }
    };
    let taken = take_layer(map, id)?;
    insert_layer(map, to, taken.layer)
}

/// Group Layers: a new group, named "Group N" after the groups the map has (the next free
/// number), goes just above the first layer given, in its group, and the layers move into it,
/// keeping their order. A group cannot go into a new group made inside itself. Reports the
/// group made and the layers moved; nothing for no layers.
pub fn group_layers(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let Some(&first) = ids.first() else {
        return Ok(LayersReport::default());
    };
    let at = locate(map, first)?;
    if let Some(parent) = at.parent
        && let Some(outer) = ids.iter().find(|id| holds_or_is(map, **id, parent))
    {
        return Err(EditError::IntoItself(*outer));
    }
    let name = numbered_name(map, "Group", |layer| {
        matches!(layer.kind, LayerKind::Group(_))
    });
    let group = take_layer_id(map)?;
    let place = LayerLocation {
        parent: at.parent,
        index: at.index + 1,
    };
    insert_layer(
        map,
        place,
        Layer::new(group, name, LayerKind::Group(Vec::new())),
    )?;
    let moved = reparent_layers(map, ids, Some(group), 0)?;
    Ok(LayersReport {
        created: vec![group],
        changed: moved,
        ..LayersReport::default()
    })
}

/// Ungroup Layers: a group's layers move out of it to just above it, in order; a layer in a
/// group moves out of that group to just above it. A group left empty is removed (an empty group
/// given is removed). A top-level layer that is not a group stays. Reports the layers moved and
/// the groups removed.
pub fn ungroup_layers(map: &mut Map, ids: &[LayerId]) -> Result<LayersReport, EditError> {
    check_layers(map, ids)?;
    let mut todo: VecDeque<LayerId> = distinct(ids).into();
    let mut report = LayersReport::default();
    while let Some(id) = todo.pop_front() {
        let Some(layer) = map.layer(id) else {
            continue;
        };
        // A group gives up its layers; any other layer leaves its group.
        let leaving: Vec<LayerId> = match &layer.kind {
            LayerKind::Group(layers) => layers.iter().map(|inner| inner.id).collect(),
            LayerKind::Tile(_) | LayerKind::Object(_) | LayerKind::Image(_) => vec![id],
        };
        let group = if is_group(map, id) {
            id
        } else {
            match locate(map, id)?.parent {
                Some(parent) => parent,
                None => continue,
            }
        };
        let at = locate(map, group)?;
        if !leaving.is_empty() {
            let moved = reparent_layers(map, &leaving, at.parent, at.index + 1)?;
            report.changed.extend(moved);
        }
        if children(map, Some(group))?.is_empty() {
            take_layer(map, group)?;
            todo.retain(|other| *other != group);
            report.removed.push(group);
        }
    }
    let removed = report.removed.clone();
    report.changed = distinct(&report.changed);
    report.changed.retain(|id| !removed.contains(id));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{nested, tree};

    #[test]
    fn removing_a_group_takes_its_layers() {
        let mut map = nested();
        let report = remove_layers(&mut map, &[LayerId(5), LayerId(2), LayerId(3)]).unwrap();
        assert_eq!(report.removed, [LayerId(5), LayerId(2)]);
        assert_eq!(tree(&map), "1:ground 6:sky");
        assert_eq!(map.validate(), []);
        assert_eq!(
            remove_layers(&mut map, &[LayerId(1), LayerId(2)]),
            Err(EditError::NoSuchLayer(LayerId(2))),
            "nothing is removed when one is missing"
        );
        assert_eq!(tree(&map), "1:ground 6:sky");
    }

    #[test]
    fn dragging_into_and_out_of_groups() {
        let mut map = nested();
        // The ground into the props group, above the crates.
        let report = move_layers(&mut map, &[LayerId(1)], Some(LayerId(4)), 1).unwrap();
        assert_eq!(report.changed, [LayerId(1)]);
        assert_eq!(
            tree(&map),
            "2:walls[3:floor 4:props[5:crates 1:ground]] 6:sky"
        );
        // The crates and the sky to the top level's bottom: drawn crates first.
        move_layers(&mut map, &[LayerId(6), LayerId(5)], None, 0).unwrap();
        assert_eq!(
            tree(&map),
            "5:crates 6:sky 2:walls[3:floor 4:props[1:ground]]"
        );
        // Within one group, the place counts the layers before the move.
        move_layers(&mut map, &[LayerId(5)], None, 3).unwrap();
        assert_eq!(
            tree(&map),
            "6:sky 2:walls[3:floor 4:props[1:ground]] 5:crates"
        );
        assert_eq!(
            move_layers(&mut map, &[LayerId(5)], None, 3).unwrap(),
            LayersReport::default(),
            "already there"
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn a_group_never_goes_into_itself() {
        let mut map = nested();
        assert_eq!(
            move_layers(&mut map, &[LayerId(2)], Some(LayerId(4)), 0),
            Err(EditError::IntoItself(LayerId(2)))
        );
        assert_eq!(
            move_layers(&mut map, &[LayerId(4)], Some(LayerId(4)), 0),
            Err(EditError::IntoItself(LayerId(4)))
        );
        assert_eq!(
            move_layers(&mut map, &[LayerId(1)], Some(LayerId(3)), 0),
            Err(EditError::NotAGroup(LayerId(3)))
        );
        assert_eq!(
            move_layers(&mut map, &[LayerId(1)], Some(LayerId(4)), 2),
            Err(EditError::NoSuchPlace {
                parent: Some(LayerId(4)),
                index: 2
            })
        );
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates]] 6:sky"
        );
    }

    #[test]
    fn raising_steps_into_and_out_of_groups() {
        let mut map = nested();
        // The ground steps into the walls group from below, then up past the floor, into props.
        raise_layers(&mut map, &[LayerId(1)]).unwrap();
        assert_eq!(
            tree(&map),
            "2:walls[1:ground 3:floor 4:props[5:crates]] 6:sky"
        );
        raise_layers(&mut map, &[LayerId(1)]).unwrap();
        raise_layers(&mut map, &[LayerId(1)]).unwrap();
        assert_eq!(
            tree(&map),
            "2:walls[3:floor 4:props[1:ground 5:crates]] 6:sky"
        );
        // Past the crates, out of props, out of the walls, past the sky.
        let steps = [
            "2:walls[3:floor 4:props[5:crates 1:ground]] 6:sky",
            "2:walls[3:floor 4:props[5:crates] 1:ground] 6:sky",
            "2:walls[3:floor 4:props[5:crates]] 1:ground 6:sky",
            "2:walls[3:floor 4:props[5:crates]] 6:sky 1:ground",
        ];
        for step in steps {
            let report = raise_layers(&mut map, &[LayerId(1)]).unwrap();
            assert_eq!(report.changed, [LayerId(1)]);
            assert_eq!(tree(&map), step);
        }
        assert!(raise_layers(&mut map, &[LayerId(1)]).unwrap().is_empty());
        assert_eq!(
            raise_layers(&mut map, &[LayerId(5), LayerId(1)]).unwrap(),
            LayersReport::default(),
            "none moves when one cannot"
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn lowering_is_raising_backwards() {
        let mut map = nested();
        lower_layers(&mut map, &[LayerId(6)]).unwrap();
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates] 6:sky]"
        );
        lower_layers(&mut map, &[LayerId(6)]).unwrap();
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 4:props[5:crates 6:sky]]"
        );
        // Both leave props below it, the crates first: the sky then lands above them.
        let report = lower_layers(&mut map, &[LayerId(6), LayerId(5)]).unwrap();
        assert_eq!(report.changed, [LayerId(5), LayerId(6)], "bottommost first");
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[3:floor 5:crates 6:sky 4:props[]]"
        );
        lower_layers(&mut map, &[LayerId(3)]).unwrap();
        assert_eq!(
            tree(&map),
            "1:ground 3:floor 2:walls[5:crates 6:sky 4:props[]]"
        );
        lower_layers(&mut map, &[LayerId(3)]).unwrap();
        assert!(lower_layers(&mut map, &[LayerId(3)]).unwrap().is_empty());
        assert_eq!(
            tree(&map),
            "3:floor 1:ground 2:walls[5:crates 6:sky 4:props[]]"
        );
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn grouping_makes_a_group_above_the_first_layer() {
        let mut map = nested();
        let report = group_layers(&mut map, &[LayerId(6), LayerId(3)]).unwrap();
        assert_eq!(report.created, [LayerId(7)]);
        assert_eq!(report.changed, [LayerId(3), LayerId(6)]);
        assert_eq!(
            tree(&map),
            "1:ground 2:walls[4:props[5:crates]] 7:Group 3[3:floor 6:sky]",
            "named after the two groups there were"
        );
        // The walls cannot go into a group made inside props, which is inside the walls.
        assert_eq!(
            group_layers(&mut map, &[LayerId(5), LayerId(2)]),
            Err(EditError::IntoItself(LayerId(2)))
        );
        assert!(group_layers(&mut map, &[]).unwrap().is_empty());
        assert_eq!(map.validate(), []);
    }

    #[test]
    fn ungrouping_moves_layers_out_and_drops_empty_groups() {
        let mut map = nested();
        let report = ungroup_layers(&mut map, &[LayerId(2)]).unwrap();
        assert_eq!(report.changed, [LayerId(3), LayerId(4)]);
        assert_eq!(report.removed, [LayerId(2)]);
        assert_eq!(tree(&map), "1:ground 3:floor 4:props[5:crates] 6:sky");
        // A layer in a group leaves it; the group, now empty, goes.
        let report = ungroup_layers(&mut map, &[LayerId(5), LayerId(1)]).unwrap();
        assert_eq!(report.changed, [LayerId(5)]);
        assert_eq!(report.removed, [LayerId(4)]);
        assert_eq!(tree(&map), "1:ground 3:floor 5:crates 6:sky");
        assert!(ungroup_layers(&mut map, &[LayerId(1)]).unwrap().is_empty());
        assert_eq!(map.validate(), []);
    }
}
