//! The map's layers as reports list them: top first, as the editor's Layers panel shows them,
//! each group before the layers in it, with how deep in groups it is.

use bt_core::{Layer, Map};

/// A layer and how many groups it is in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerAt<'m> {
    /// The layer.
    pub(crate) layer: &'m Layer,
    /// How many groups it is in: 0 at the map's top level.
    pub(crate) depth: usize,
}

/// Every layer of `map`, the top one first, each group before the layers in it.
pub(crate) fn layers_top_first(map: &Map) -> Vec<LayerAt<'_>> {
    fn walk<'m>(layers: &'m [Layer], depth: usize, out: &mut Vec<LayerAt<'m>>) {
        for layer in layers.iter().rev() {
            out.push(LayerAt { layer, depth });
            walk(layer.children(), depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(&map.layers, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use bt_core::{LayerId, LayerKind};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    #[test]
    fn layers_are_listed_top_first_with_groups_before_their_layers() {
        let child = |name: &str| {
            Layer::new(
                LayerId(0),
                name,
                LayerKind::Tile(bt_core::TileLayer::finite(UVec2::new(2, 2))),
            )
        };
        let group = Layer::new(
            LayerId(10),
            "group",
            LayerKind::Group(vec![child("a"), child("b")]),
        );
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tile_layer("ground", "")
            .layer(group)
            .tile_layer("top", "")
            .build()
            .unwrap();
        let listed: Vec<(String, usize)> = layers_top_first(&map)
            .iter()
            .map(|at| (at.layer.name.clone(), at.depth))
            .collect();
        assert_eq!(
            listed,
            [
                ("top".to_owned(), 0),
                ("group".to_owned(), 0),
                ("b".to_owned(), 1),
                ("a".to_owned(), 1),
                ("ground".to_owned(), 0)
            ]
        );
    }
}
