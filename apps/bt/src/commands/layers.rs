//! Layers named on the command line (`--layers` of `bt view` and `bt render`, the map's side of
//! `bt explain`'s bindings): by name or `#ID`, as scripts name them (`10-agents.md`, §5).

use bt_core::{LayerId, Map};
use bt_script::{LayerRef, Names};

/// A layer as written: `#ID`, else a name.
pub(crate) fn layer_ref(written: &str) -> LayerRef {
    match written.strip_prefix('#').and_then(|id| id.parse().ok()) {
        Some(id) => LayerRef::Id(id),
        None => LayerRef::Name(written.to_owned()),
    }
}

/// The layers `written` names, in order; an unknown name is an error that lists the map's
/// layers, and a name several layers have asks for one by id.
pub(crate) fn named_layers(
    map: &Map,
    names: &Names,
    written: &[String],
) -> anyhow::Result<Vec<LayerId>> {
    written
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| Ok(names.layer(map, &layer_ref(name))?))
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    #[test]
    fn layers_go_by_name_or_id_and_an_unknown_name_lists_them() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tile_layer("ground", "")
            .tile_layer("Deco Tiles", "")
            .object_layer("props", Vec::new())
            .build()
            .unwrap();
        let names = Names::of(&map);
        let ids = |written: &[&str]| {
            let written: Vec<String> = written.iter().map(|w| (*w).to_owned()).collect();
            named_layers(&map, &names, &written)
        };
        let props = map.layers[2].id;
        let deco = map.layers[1].id;
        assert_eq!(
            ids(&["deco-tiles", " props"]).unwrap(),
            [deco, props],
            "case and separators don't matter"
        );
        assert_eq!(ids(&[&format!("#{}", props.0)]).unwrap(), [props]);
        let error = ids(&["prop"]).unwrap_err().to_string();
        assert_eq!(
            error,
            "unknown layer “prop”: did you mean props? (layers: deco-tiles, ground, props)"
        );
    }
}
