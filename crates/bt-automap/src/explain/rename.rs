//! Explanations in the map's words: a run with layer names binds the names the rules use to map
//! layers of other names, and an explanation renamed this way says the map layer's name where it
//! said the rules' name.

use bt_core::Map;

use crate::engine::LayerNames;

/// Turns the names the rules use into the names of the map layers bound to them.
pub(crate) struct LayerRenaming<'a> {
    names: &'a LayerNames,
    map: &'a Map,
}

impl<'a> LayerRenaming<'a> {
    /// The renaming of `names`' tile bindings on `map`.
    pub(crate) fn new(names: &'a LayerNames, map: &'a Map) -> LayerRenaming<'a> {
        LayerRenaming { names, map }
    }

    /// Replaces `name` by the name of the map layer it is bound to, if it is bound to one.
    pub(crate) fn rename(&self, name: &mut String) {
        if let Some(layer) = self
            .names
            .tile_layer(name)
            .and_then(|id| self.map.layer(id))
        {
            name.clone_from(&layer.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Layer, LayerId, LayerKind, TileLayer};
    use glam::UVec2;

    use super::*;

    #[test]
    fn bound_names_become_the_layers_names_and_others_stay() {
        let map = Map {
            layers: vec![Layer::new(
                LayerId(3),
                "plat1",
                LayerKind::Tile(TileLayer::finite(UVec2::ONE)),
            )],
            ..Map::default()
        };
        let names = LayerNames::builder().tiles("walls", LayerId(3)).build();
        let renaming = LayerRenaming::new(&names, &map);
        let mut bound = "walls".to_owned();
        renaming.rename(&mut bound);
        assert_eq!(bound, "plat1");
        let mut unbound = "deco".to_owned();
        renaming.rename(&mut unbound);
        assert_eq!(unbound, "deco");
    }
}
