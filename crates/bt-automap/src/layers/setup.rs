//! Which rules run on which of a map's layers, with which seed: AutoMap layers' setup. A run is
//! the engine's own [`LayerNames`], its unbound names temporary layers.

use std::path::PathBuf;

use bt_core::{LayerId, LayerKind, Map};

use super::rule_layers::RuleLayers;
use crate::engine::{LayerNames, NameBinding, Unbound, find_layer};

/// AutoMap layers' setup: the rules, the runs that bind the names they use to the map's layers,
/// and the seed of their random choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// The rules file or rule map.
    pub rules: PathBuf,
    /// The runs, in order, each over the stage the one before left.
    pub runs: Vec<LayerNames>,
    /// The seed of the first run; the others draw from seeds that follow it.
    pub seed: u64,
}

impl Setup {
    /// The seed run `run` draws from: one after another from the setup's, so two runs over the
    /// same layers do not repeat each other's choices.
    pub fn run_seed(&self, run: usize) -> u64 {
        self.seed
            .wrapping_add(u64::try_from(run).unwrap_or(u64::MAX))
    }
}

impl LayerNames {
    /// A run's names: none bound yet, every name a temporary layer until it is.
    pub fn temporary() -> LayerNames {
        LayerNames::builder().unbound(Unbound::Temporary).build()
    }

    /// Every name the rules use bound to the map's layer of that name, as Tiled finds it (the
    /// bottom-most one, depth first), and the names the map has no such layer for left
    /// temporary: what starting AutoMap layers binds.
    pub fn found_in(map: &Map, rule_layers: &RuleLayers) -> LayerNames {
        let mut names = LayerNames::temporary();
        let tile_names = rule_layers.inputs.iter().chain(&rule_layers.tile_outputs);
        for name in tile_names {
            if let Some(layer) = find_layer(map, name, |k| matches!(k, LayerKind::Tile(_))) {
                names = names.rebound(name, false, Some(layer.id));
            }
        }
        for name in &rule_layers.object_outputs {
            if let Some(layer) = find_layer(map, name, |k| matches!(k, LayerKind::Object(_))) {
                names = names.rebound(name, true, Some(layer.id));
            }
        }
        names
    }

    /// The tile layers bound, in the order they were bound.
    pub fn tile_bindings(&self) -> &[NameBinding] {
        &self.tiles
    }

    /// The object layers bound, in the order they were bound.
    pub fn object_bindings(&self) -> &[NameBinding] {
        &self.objects
    }

    /// These names with `name` (of the rules' tile layers, or with `objects` their object
    /// layers) bound to `layer`, or left unbound with `None`.
    pub fn rebound(&self, name: &str, objects: bool, layer: Option<LayerId>) -> LayerNames {
        let mut names = self.clone();
        let bindings = if objects {
            &mut names.objects
        } else {
            &mut names.tiles
        };
        match (bindings.iter_mut().find(|b| b.name == name), layer) {
            (Some(binding), Some(layer)) => binding.layer = layer,
            (Some(_), None) => bindings.retain(|b| b.name != name),
            (None, Some(layer)) => bindings.push(NameBinding {
                name: name.to_owned(),
                layer,
            }),
            (None, None) => {}
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    #[test]
    fn names_are_found_as_tiled_finds_them_and_the_rest_are_temporary() {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(atlas)
            .tile_layer("set", "1 .\n. .")
            .tile_layer("Ground", ". .\n. .")
            .tile_layer("set", ". .\n. .")
            .build()
            .unwrap();
        let rule_layers = RuleLayers {
            inputs: vec!["set".into(), "Ground".into()],
            tile_outputs: vec!["Ground".into(), "Over".into()],
            object_outputs: vec!["spawns".into()],
            made: vec!["Ground".into()],
        };
        let names = LayerNames::found_in(&map, &rule_layers);
        assert_eq!(names.tile_layer("set"), Some(LayerId(1)), "the bottom-most");
        assert_eq!(names.tile_layer("Ground"), Some(LayerId(2)));
        assert_eq!(names.tile_layer("Over"), None);
        assert_eq!(names.object_layer("spawns"), None);
        assert_eq!(names.unbound(), Unbound::Temporary);
        let moved = names.rebound("set", false, Some(LayerId(3)));
        assert_eq!(moved.tile_layer("set"), Some(LayerId(3)));
        assert_eq!(moved.rebound("set", false, None).tile_layer("set"), None);
        assert_eq!(moved.tile_bindings().len(), 2);
    }
}
