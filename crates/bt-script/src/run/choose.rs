//! Which layer an operation goes on when it does not name one (`10-agents.md`, §8.2), and the
//! locks that refuse it. A terrain goes where its set's tiles already are, else on the only
//! tile layer; tiles on the layer their name declares, else where most of them are, else on the
//! only tile layer; a sprite where that sprite is, else where its tileset's sprites are, else on
//! any layer with sprites, else on the only object layer, else on a layer named like one for
//! props; a typed object where its type is, else where typed shapes are, else on the only
//! object layer, else on a layer named like one for markers. Otherwise the script must say.

use bt_core::{Cell, Layer, LayerId, LayerKind, Map};
use bt_utils::text::slug;

use super::error::RunFailure;
use super::runner::Runner;
use crate::names::{NamedTerrain, TileGroup};
use crate::script::LayerRef;

/// Words in the names of layers sprites go on when nothing else says, in order.
const SPRITE_LAYER_WORDS: [&str; 7] = [
    "prop", "sprite", "decor", "object", "entit", "thing", "tree",
];

/// Words in the names of layers typed objects go on when nothing else says, in order.
const MARKER_LAYER_WORDS: [&str; 9] = [
    "design", "marker", "logic", "spawn", "trigger", "zone", "meta", "game", "object",
];

impl Runner<'_> {
    /// The tile layer `terrain` is painted on: the one named, else where its set's tiles are,
    /// else the only tile layer.
    pub(super) fn terrain_layer(
        &self,
        terrain: &NamedTerrain,
        given: Option<&LayerRef>,
    ) -> Result<LayerId, RunFailure> {
        if let Some(layer) = given {
            return Ok(self.names.tile_layer(&self.map, layer)?);
        }
        let set = self
            .map
            .tileset(terrain.slot)
            .and_then(|tileset| tileset.wang_sets.get(terrain.set));
        let held = |cell: Cell| {
            cell.slot() == Some(terrain.slot)
                && set.is_some_and(|set| set.tiles.contains_key(&cell.tile()))
        };
        self.layer_holding_most(held)
            .or_else(|| self.only_layer(false))
            .ok_or_else(|| self.say_which(&terrain.name, false))
    }

    /// The tile layer tiles go on: the one named, else the one their name declares, else where
    /// most of them are, else the only tile layer.
    pub(super) fn tiles_layer(
        &self,
        tiles: &TileGroup,
        given: Option<&LayerRef>,
    ) -> Result<LayerId, RunFailure> {
        if let Some(layer) = given.or(tiles.layer.as_ref()) {
            return Ok(self.names.tile_layer(&self.map, layer)?);
        }
        let held = |cell: Cell| {
            cell.slot().is_some_and(|slot| {
                tiles
                    .tiles
                    .iter()
                    .any(|t| t.slot() == Some(slot) && t.tile() == cell.tile())
            })
        };
        self.layer_holding_most(held)
            .or_else(|| self.only_layer(false))
            .ok_or_else(|| self.say_which("the tiles", false))
    }

    /// The object layer a sprite goes on: the one named, else where that sprite is, else where
    /// its tileset's sprites are, else any layer with sprites, else the only object layer, else
    /// a layer named like one for props.
    pub(super) fn sprite_layer(
        &self,
        sprite: Cell,
        given: Option<&LayerRef>,
    ) -> Result<LayerId, RunFailure> {
        if let Some(layer) = given {
            return Ok(self.names.object_layer(&self.map, layer)?);
        }
        let same = |object: &bt_core::MapObject| {
            object
                .tile
                .is_some_and(|tile| tile.slot() == sprite.slot() && tile.tile() == sprite.tile())
        };
        let same_tileset = |object: &bt_core::MapObject| {
            object.tile.is_some_and(|tile| tile.slot() == sprite.slot())
        };
        self.object_layer_holding_most(same)
            .or_else(|| self.object_layer_holding_most(same_tileset))
            .or_else(|| self.first_object_layer_with(|o| o.tile.is_some()))
            .or_else(|| self.only_layer(true))
            .or_else(|| self.object_layer_named(&SPRITE_LAYER_WORDS))
            .ok_or_else(|| self.say_which("the sprite", true))
    }

    /// The object layer a typed object goes on: the one named, else where its type is, else
    /// where typed shapes are, else the only object layer, else a layer named like one for
    /// markers.
    pub(super) fn typed_layer(
        &self,
        kind: &str,
        given: Option<&LayerRef>,
    ) -> Result<LayerId, RunFailure> {
        if let Some(layer) = given {
            return Ok(self.names.object_layer(&self.map, layer)?);
        }
        let wanted = slug(kind);
        let of_type = |object: &bt_core::MapObject| {
            !wanted.is_empty() && slug(crate::names::object_type(&self.map, object)) == wanted
        };
        self.object_layer_holding_most(of_type)
            .or_else(|| self.first_object_layer_with(|o| o.tile.is_none() && !o.class.is_empty()))
            .or_else(|| self.only_layer(true))
            .or_else(|| self.object_layer_named(&MARKER_LAYER_WORDS))
            .ok_or_else(|| self.say_which(&format!("the {kind}"), true))
    }

    /// Refuses a change to a locked layer, or to a layer in a locked group, naming the lock to
    /// open.
    pub(super) fn unlocked(&self, layer: LayerId) -> Result<(), RunFailure> {
        unlocked_in(&self.map, layer)
    }

    /// The tile layer where most cells hold what `held` says, the lowest of those that hold as
    /// many; none when no cell does.
    fn layer_holding_most(&self, held: impl Fn(Cell) -> bool) -> Option<LayerId> {
        let mut best: Option<Holding> = None;
        self.map.for_each_layer(|layer, _| {
            let Some(tiles) = layer.as_tile_layer() else {
                return;
            };
            let count = tiles.cells().filter(|placed| held(placed.cell)).count();
            if count > 0 && best.is_none_or(|b| count > b.count) {
                best = Some(Holding {
                    layer: layer.id,
                    count,
                });
            }
        });
        best.map(|b| b.layer)
    }

    /// The object layer where most objects are what `wanted` says, the lowest of those that
    /// hold as many; none when no object is.
    fn object_layer_holding_most(
        &self,
        wanted: impl Fn(&bt_core::MapObject) -> bool,
    ) -> Option<LayerId> {
        let mut best: Option<Holding> = None;
        self.map.for_each_layer(|layer, _| {
            let Some(objects) = layer.as_object_layer() else {
                return;
            };
            let count = objects.objects.iter().filter(|o| wanted(o)).count();
            if count > 0 && best.is_none_or(|b| count > b.count) {
                best = Some(Holding {
                    layer: layer.id,
                    count,
                });
            }
        });
        best.map(|b| b.layer)
    }

    /// The lowest object layer holding an object `wanted` says.
    fn first_object_layer_with(
        &self,
        wanted: impl Fn(&bt_core::MapObject) -> bool,
    ) -> Option<LayerId> {
        let mut found = None;
        self.map.for_each_layer(|layer, _| {
            if found.is_none()
                && layer
                    .as_object_layer()
                    .is_some_and(|objects| objects.objects.iter().any(&wanted))
            {
                found = Some(layer.id);
            }
        });
        found
    }

    /// The map's only layer of the kind, when it has exactly one.
    fn only_layer(&self, objects: bool) -> Option<LayerId> {
        match layers_of_kind(&self.map, objects).as_slice() {
            [only] => Some(only.id),
            _ => None,
        }
    }

    /// The first object layer whose name holds one of `words`, trying the words in order.
    fn object_layer_named(&self, words: &[&str]) -> Option<LayerId> {
        let layers = layers_of_kind(&self.map, true);
        words.iter().find_map(|word| {
            layers
                .iter()
                .find(|layer| slug(&layer.name).contains(word))
                .map(|layer| layer.id)
        })
    }

    /// The error asking which layer, listing the map's layers of the kind.
    fn say_which(&self, what: &str, objects: bool) -> RunFailure {
        RunFailure::SayWhichLayer {
            what: what.to_owned(),
            objects,
            layers: layers_of_kind(&self.map, objects)
                .iter()
                .map(|layer| layer.name.clone())
                .collect(),
        }
    }
}

/// A layer and how many of something it holds.
#[derive(Clone, Copy)]
struct Holding {
    layer: LayerId,
    count: usize,
}

/// The map's tile layers, or object layers, bottom first.
fn layers_of_kind(map: &Map, objects: bool) -> Vec<&Layer> {
    let mut layers = Vec::new();
    map.for_each_layer(|layer, _| {
        let fits = match layer.kind {
            LayerKind::Tile(_) => !objects,
            LayerKind::Object(_) => objects,
            LayerKind::Image(_) | LayerKind::Group(_) => false,
        };
        if fits {
            layers.push(layer);
        }
    });
    layers
}

/// Refuses a change to `layer` of `map` when it, or a group it is in, is locked.
pub(super) fn unlocked_in(map: &Map, layer: LayerId) -> Result<(), RunFailure> {
    let Some(path) = map.layer_path(layer) else {
        return Ok(());
    };
    let name = |id: LayerId| map.layer(id).map_or_else(String::new, |l| l.name.clone());
    if let Some(&locked) = path
        .iter()
        .find(|&&id| map.layer(id).is_some_and(|l| l.locked))
    {
        return Err(RunFailure::Locked {
            layer: name(layer),
            group: (locked != layer).then(|| name(locked)),
        });
    }
    Ok(())
}
