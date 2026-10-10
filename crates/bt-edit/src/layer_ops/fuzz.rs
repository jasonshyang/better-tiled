//! Random sequences of layer commands: the map stays valid after every one, reports agree with
//! the tree, and a refused command changes nothing.

use bt_core::{Layer, LayerId, Map, ObjectId};
use bt_test_data::Fuzz;
use bt_utils::IRect;
use bt_utils::rng::Rng;
use glam::{DVec2, IVec2, UVec2};

use super::*;
use crate::error::EditError;
use crate::region::TileRegion;
use crate::report::LayersReport;
use crate::testutil::nested;

/// A few of the map's layers, chosen at random (sometimes none, sometimes a missing one).
fn some_layers(map: &Map, rng: &mut Rng) -> Vec<LayerId> {
    let all = layer_order(map);
    let mut picked: Vec<LayerId> = all
        .iter()
        .copied()
        .filter(|_| rng.below(3) == Some(0))
        .collect();
    if rng.below(20) == Some(0) {
        picked.push(LayerId(9999));
    }
    picked
}

fn one_layer(map: &Map, rng: &mut Rng) -> Option<LayerId> {
    let all = layer_order(map);
    let index = rng.below(all.len() as u64)?;
    all.get(index as usize).copied()
}

fn some_objects(map: &Map, rng: &mut Rng) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    map.for_each_layer(|layer, _| {
        if let Some(objects) = layer.as_object_layer() {
            ids.extend(objects.objects.iter().map(|o| o.id));
        }
    });
    ids.retain(|_| rng.below(2) == Some(0));
    ids
}

fn tile_layers(map: &Map) -> Vec<LayerId> {
    let mut ids = Vec::new();
    map.for_each_layer(|layer, _| {
        if layer.as_tile_layer().is_some() {
            ids.push(layer.id);
        }
    });
    ids
}

fn random_region(rng: &mut Rng) -> TileRegion {
    let x = rng.below(5).unwrap_or(0) as i32 - 1;
    let y = rng.below(4).unwrap_or(0) as i32 - 1;
    let w = rng.below(4).unwrap_or(0) as i32;
    let h = rng.below(3).unwrap_or(0) as i32;
    TileRegion::from_rect(IRect { x, y, w, h })
}

/// A command run on the map: what it was, and what it gave.
struct Ran {
    command: String,
    result: Result<LayersReport, EditError>,
}

impl Ran {
    fn new(command: String, result: Result<LayersReport, EditError>) -> Ran {
        Ran { command, result }
    }
}

/// Runs one random command.
fn random_command(map: &mut Map, rng: &mut Rng) -> Ran {
    let kinds = [
        LayerType::Tile,
        LayerType::Object,
        LayerType::Image,
        LayerType::Group,
    ];
    match rng.below(14).unwrap_or(0) {
        0 => {
            let kind = kinds[rng.below(4).unwrap_or(0) as usize];
            let above = one_layer(map, rng).filter(|_| rng.below(4) != Some(0));
            let result = new_layer(map, kind, above).map(|id| LayersReport {
                created: vec![id],
                ..LayersReport::default()
            });
            Ran::new(format!("new {kind:?} above {above:?}"), result)
        }
        1 => {
            let ids = some_layers(map, rng);
            let suffix = rng.below(2) == Some(0);
            Ran::new(
                format!("duplicate {ids:?}"),
                duplicate_layers(map, &ids, suffix),
            )
        }
        2 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("remove {ids:?}"), remove_layers(map, &ids))
        }
        3 => {
            let ids = some_layers(map, rng);
            let parent = one_layer(map, rng).filter(|_| rng.below(3) != Some(0));
            let places = children(map, parent).map_or(0, <[Layer]>::len);
            let index = rng.below(places as u64 + 2).unwrap_or(0) as usize;
            let result = move_layers(map, &ids, parent, index);
            Ran::new(format!("move {ids:?} to {parent:?} at {index}"), result)
        }
        4 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("raise {ids:?}"), raise_layers(map, &ids))
        }
        5 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("lower {ids:?}"), lower_layers(map, &ids))
        }
        6 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("group {ids:?}"), group_layers(map, &ids))
        }
        7 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("ungroup {ids:?}"), ungroup_layers(map, &ids))
        }
        8 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("merge {ids:?}"), merge_layers_down(map, &ids))
        }
        9 => {
            let mut layers = tile_layers(map);
            layers.retain(|_| rng.below(2) == Some(0));
            let region = random_region(rng);
            let source = LayerViaSource::Tiles {
                layers: &layers,
                region: &region,
            };
            let result = if rng.below(2) == Some(0) {
                layer_via_copy(map, source)
            } else {
                layer_via_cut(map, source)
            };
            Ran::new(format!("tiles via {layers:?} {region:?}"), result)
        }
        10 => {
            let objects = some_objects(map, rng);
            let result = if rng.below(2) == Some(0) {
                layer_via_copy(map, LayerViaSource::Objects(&objects))
            } else {
                layer_via_cut(map, LayerViaSource::Objects(&objects))
            };
            Ran::new(format!("objects via {objects:?}"), result)
        }
        11 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("toggle visible {ids:?}"), toggle_visible(map, &ids))
        }
        12 => {
            let ids = some_layers(map, rng);
            Ran::new(format!("toggle locked {ids:?}"), toggle_locked(map, &ids))
        }
        _ => {
            let ids = some_layers(map, rng);
            let delta = DVec2::new(
                rng.below(40).unwrap_or(0) as f64 - 20.0,
                rng.below(40).unwrap_or(0) as f64 - 20.0,
            );
            Ran::new(
                format!("offset {ids:?}"),
                offset_layers(map, &ids, delta, |p| p),
            )
        }
    }
}

#[test]
fn random_layer_commands_keep_the_map_valid() {
    let fuzz = Fuzz::from_env(0x1A7E_25ED, 200);
    let mut changes = 0usize;
    let mut commands = 0usize;
    for case in 0..fuzz.cases {
        let mut rng = Rng::new(fuzz.case_seed(case));
        let mut map = nested();
        let mut done = Vec::new();
        for _ in 0..12 {
            let before = map.clone();
            let Ran { command, result } = random_command(&mut map, &mut rng);
            done.push(command);
            commands += 1;
            let context = || format!("case {case} ({}) after {done:?}", fuzz.reproduce());
            match result {
                Ok(report) => {
                    for id in &report.created {
                        assert!(map.layer(*id).is_some(), "made {id}: {}", context());
                    }
                    for id in &report.removed {
                        assert!(map.layer(*id).is_none(), "removed {id}: {}", context());
                    }
                    if report.is_empty() {
                        assert_eq!(map, before, "an empty report: {}", context());
                    } else {
                        changes += 1;
                    }
                }
                Err(_) => assert_eq!(map, before, "a refusal: {}", context()),
            }
            assert_eq!(map.validate(), [], "{}", context());
            map.for_each_layer(|layer, _| {
                if let Some(tiles) = layer.as_tile_layer() {
                    assert_eq!(tiles.size(), UVec2::new(4, 3), "{}", context());
                    assert!(
                        tiles
                            .cells()
                            .all(|placed| placed.at.cmpge(IVec2::ZERO).all()),
                        "{}",
                        context()
                    );
                }
            });
        }
    }
    assert!(
        changes * 2 > commands,
        "most commands should change something: {changes} of {commands}"
    );
}
