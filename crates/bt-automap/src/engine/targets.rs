//! The layers a run reads and writes, by the names the rules use: map layers found as the run's
//! layer names say (Tiled's lookup by name by default), layers the run creates for outputs the
//! map lacks, and temporary layers that are dropped after the run.

use std::collections::HashMap;

use bt_core::{LayerId, Map, Properties};
use bt_utils::IRect;
use glam::UVec2;

use crate::engine::bind::{find_layer, is_locked, is_objects, is_tiles};
use crate::engine::grid::Grid;
use crate::engine::request::{LayerNames, Unbound};
use crate::problem::{Problem, ProblemKind};

/// Where a target comes from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TargetOrigin {
    /// A layer of the map.
    Existing(LayerId),
    /// A layer the run adds on top of the map if something is written to it.
    New,
    /// A layer that lives during the run only.
    Temporary,
}

/// A tile layer of a run.
#[derive(Clone, Debug)]
pub(crate) struct TileTarget {
    /// The name the rules use.
    pub(crate) name: String,
    /// Where it comes from.
    pub(crate) origin: TargetOrigin,
    /// Whether the layer or a group it is in is locked.
    pub(crate) locked: bool,
    /// The layer's size on a finite map: cells outside are never written and read as empty
    /// (or as the border cells say).
    pub(crate) size: Option<UVec2>,
    /// Its cells.
    pub(crate) grid: Grid,
    /// Its properties with the output layers' merged in, once a rule wrote it.
    pub(crate) properties: Option<Properties>,
}

/// An object layer of a run.
#[derive(Clone, Debug)]
pub(crate) struct ObjectTarget {
    /// The name the rules use.
    pub(crate) name: String,
    /// Where it comes from.
    pub(crate) origin: TargetOrigin,
    /// Whether the layer or a group it is in is locked.
    pub(crate) locked: bool,
    /// Its properties with the output layers' merged in, once a rule placed objects on it.
    pub(crate) properties: Option<Properties>,
    /// Whether some object was placed on it.
    pub(crate) placed: bool,
}

/// A layer the run creates, in the order they go on top of the map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Created {
    /// A tile layer, by its index among the run's tile targets.
    Tiles(usize),
    /// An object layer, by its index among the run's object targets.
    Objects(usize),
}

/// The layers of a run.
#[derive(Clone, Debug, Default)]
pub(crate) struct Targets {
    pub(crate) tiles: Vec<TileTarget>,
    pub(crate) objects: Vec<ObjectTarget>,
    tile_names: HashMap<String, usize>,
    object_names: HashMap<String, usize>,
    /// The layers the run creates, in the order they are declared.
    pub(crate) created: Vec<Created>,
    /// The area grids hold a dense copy of.
    area: IRect,
}

impl Targets {
    /// No layers yet; grids copy `area` densely.
    pub(crate) fn new(area: IRect) -> Targets {
        Targets {
            area,
            ..Targets::default()
        }
    }

    fn tile_target_of(&mut self, map: &Map, name: &str, origin: TargetOrigin) -> usize {
        let layer = match origin {
            TargetOrigin::Existing(id) => map.layer(id),
            TargetOrigin::New | TargetOrigin::Temporary => None,
        };
        let tiles = layer.and_then(|l| l.as_tile_layer());
        let size =
            (!map.infinite).then(|| tiles.map_or(UVec2::new(map.width, map.height), |t| t.size()));
        let locked = match origin {
            TargetOrigin::Existing(id) => is_locked(map, id),
            TargetOrigin::New | TargetOrigin::Temporary => false,
        };
        self.tiles.push(TileTarget {
            name: name.to_owned(),
            origin,
            locked,
            size,
            grid: Grid::new(tiles.cloned(), self.area),
            properties: None,
        });
        let index = self.tiles.len() - 1;
        self.tile_names.insert(name.to_owned(), index);
        if origin == TargetOrigin::New {
            self.created.push(Created::Tiles(index));
        }
        index
    }

    /// The tile layer rules write for `name`: bound, found by name, or created.
    pub(crate) fn tile_output(&mut self, map: &Map, names: &LayerNames, name: &str) -> usize {
        if let Some(&index) = self.tile_names.get(name) {
            return index;
        }
        let origin = match names.tile_layer(name) {
            Some(id) => TargetOrigin::Existing(id),
            None => match names.unbound {
                Unbound::LookUpByName => find_layer(map, name, is_tiles)
                    .map_or(TargetOrigin::New, |l| TargetOrigin::Existing(l.id)),
                Unbound::Temporary => TargetOrigin::Temporary,
            },
        };
        self.tile_target_of(map, name, origin)
    }

    /// The tile layer rules read for `name`, if there is one: bound, found by name, or one an
    /// earlier output created.
    pub(crate) fn tile_input(
        &mut self,
        map: &Map,
        names: &LayerNames,
        name: &str,
    ) -> Option<usize> {
        if let Some(&index) = self.tile_names.get(name) {
            return Some(index);
        }
        let origin = match names.tile_layer(name) {
            Some(id) => TargetOrigin::Existing(id),
            None => match names.unbound {
                Unbound::LookUpByName => {
                    TargetOrigin::Existing(find_layer(map, name, is_tiles)?.id)
                }
                Unbound::Temporary => return None,
            },
        };
        Some(self.tile_target_of(map, name, origin))
    }

    /// The object layer rules place objects on for `name`: bound, found by name, or created.
    pub(crate) fn object_output(&mut self, map: &Map, names: &LayerNames, name: &str) -> usize {
        if let Some(&index) = self.object_names.get(name) {
            return index;
        }
        let origin = match names.object_layer(name) {
            Some(id) => TargetOrigin::Existing(id),
            None => match names.unbound {
                Unbound::LookUpByName => find_layer(map, name, is_objects)
                    .map_or(TargetOrigin::New, |l| TargetOrigin::Existing(l.id)),
                Unbound::Temporary => TargetOrigin::Temporary,
            },
        };
        let locked = match origin {
            TargetOrigin::Existing(id) => is_locked(map, id),
            TargetOrigin::New | TargetOrigin::Temporary => false,
        };
        self.objects.push(ObjectTarget {
            name: name.to_owned(),
            origin,
            locked,
            properties: None,
            placed: false,
        });
        let index = self.objects.len() - 1;
        self.object_names.insert(name.to_owned(), index);
        if origin == TargetOrigin::New {
            self.created.push(Created::Objects(index));
        }
        index
    }
}

/// Why a run's layer names cannot be used: a bound layer that is gone or of the wrong kind, or a
/// layer bound twice.
pub(crate) fn refused_bindings(map: &Map, names: &LayerNames) -> Option<Problem> {
    let refuse = |text: String| Some(Problem::new(ProblemKind::BindingRefused, text));
    let mut seen: Vec<LayerId> = Vec::new();
    for (binding, objects) in names
        .tiles
        .iter()
        .map(|b| (b, false))
        .chain(names.objects.iter().map(|b| (b, true)))
    {
        let Some(layer) = map.layer(binding.layer) else {
            return refuse(format!(
                "the name “{}” is bound to layer {}, which the map does not have",
                binding.name, binding.layer
            ));
        };
        let right_kind = if objects {
            layer.as_object_layer().is_some()
        } else {
            layer.as_tile_layer().is_some()
        };
        if !right_kind {
            return refuse(format!(
                "the name “{}” is bound to “{}”, which is a {}",
                binding.name,
                layer.name,
                layer.kind_name()
            ));
        }
        if seen.contains(&binding.layer) {
            return refuse(format!(
                "layer “{}” is bound to two names; a layer stands for one name",
                layer.name
            ));
        }
        seen.push(binding.layer);
    }
    None
}

#[cfg(test)]
mod tests {
    use bt_core::{Layer, LayerKind, ObjectLayer, TileLayer};

    use super::*;

    fn map() -> Map {
        let tiles = |id, name: &str| {
            Layer::new(
                LayerId(id),
                name,
                LayerKind::Tile(TileLayer::finite(UVec2::new(4, 4))),
            )
        };
        let mut locked_group = Layer::new(
            LayerId(3),
            "group",
            LayerKind::Group(vec![tiles(4, "walls")]),
        );
        locked_group.locked = true;
        Map {
            width: 4,
            height: 4,
            layers: vec![
                tiles(1, "ground"),
                Layer::new(
                    LayerId(2),
                    "things",
                    LayerKind::Object(ObjectLayer::default()),
                ),
                locked_group,
                tiles(5, "walls"),
            ],
            ..Map::default()
        }
    }

    #[test]
    fn names_find_the_bottom_most_layer_and_outputs_the_map_lacks_are_created() {
        let map = map();
        let names = LayerNames::by_name();
        let mut targets = Targets::new(IRect {
            x: 0,
            y: 0,
            w: 4,
            h: 4,
        });
        let walls = targets.tile_output(&map, &names, "walls");
        assert_eq!(
            targets.tiles[walls].origin,
            TargetOrigin::Existing(LayerId(4))
        );
        assert!(targets.tiles[walls].locked, "its group is locked");
        let deco = targets.tile_output(&map, &names, "deco");
        assert_eq!(targets.tiles[deco].origin, TargetOrigin::New);
        assert_eq!(targets.tile_input(&map, &names, "deco"), Some(deco));
        assert_eq!(
            targets.tile_input(&map, &names, "things"),
            None,
            "not tiles"
        );
        let things = targets.object_output(&map, &names, "things");
        assert_eq!(
            targets.objects[things].origin,
            TargetOrigin::Existing(LayerId(2))
        );
        assert_eq!(targets.created, [Created::Tiles(deco)]);
    }

    #[test]
    fn temporary_outputs_are_not_created() {
        let map = map();
        let names = LayerNames::builder().unbound(Unbound::Temporary).build();
        let mut targets = Targets::new(IRect::default());
        let ground = targets.tile_output(&map, &names, "ground");
        assert_eq!(targets.tiles[ground].origin, TargetOrigin::Temporary);
        assert_eq!(targets.tile_input(&map, &names, "walls"), None);
        assert!(targets.created.is_empty());
    }

    #[test]
    fn bindings_must_name_layers_of_the_right_kind_once() {
        let map = map();
        let ok = LayerNames::builder().tiles("floor", LayerId(1)).build();
        assert_eq!(refused_bindings(&map, &ok), None);
        for names in [
            LayerNames::builder().tiles("floor", LayerId(9)).build(),
            LayerNames::builder().tiles("floor", LayerId(2)).build(),
            LayerNames::builder()
                .tiles("floor", LayerId(1))
                .tiles("ground", LayerId(1))
                .build(),
        ] {
            assert_eq!(
                refused_bindings(&map, &names).map(|p| p.kind),
                Some(ProblemKind::BindingRefused)
            );
        }
        let mut targets = Targets::new(IRect::default());
        let floor = targets.tile_output(&map, &ok, "floor");
        assert_eq!(
            targets.tiles[floor].origin,
            TargetOrigin::Existing(LayerId(1))
        );
    }
}
