//! What a rule set reads and writes, by the names its rule maps use: the names a run of AutoMap
//! layers binds to the map's layers.

use crate::rules::RuleSet;

/// The names a rule set's rule maps read and write, each once, in the order the rule maps first
/// use them. Rule maps that are not used (they have errors) are left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleLayers {
    /// The names read.
    pub inputs: Vec<String>,
    /// The names of the tile layers written.
    pub tile_outputs: Vec<String>,
    /// The names of the object layers written.
    pub object_outputs: Vec<String>,
    /// The inputs an earlier rule map writes as tiles: they need no map layer, since a run
    /// reads what that rule map wrote (Tiled's sewers example reads what its first rule map made).
    pub made: Vec<String>,
}

impl RuleSet {
    /// The names the rules read and write.
    pub fn rule_layers(&self) -> RuleLayers {
        let mut layers = RuleLayers::default();
        for rule_map in self.rule_maps().iter().filter(|r| r.is_used()) {
            for input in rule_map.inputs() {
                if layers.tile_outputs.contains(input) && !layers.made.contains(input) {
                    layers.made.push(input.clone());
                }
                push_once(&mut layers.inputs, input);
            }
            for output in rule_map.tile_outputs() {
                push_once(&mut layers.tile_outputs, output);
            }
            for output in rule_map.object_outputs() {
                push_once(&mut layers.object_outputs, output);
            }
        }
        layers
    }
}

fn push_once(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|n| n == name) {
        names.push(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::Map;
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    fn rule_map(layers: &[(&str, &str)]) -> Arc<Map> {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let mut builder = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1)).tileset(atlas);
        for (name, cells) in layers {
            builder = builder.tile_layer(name, cells);
        }
        Arc::new(builder.build().unwrap())
    }

    #[test]
    fn inputs_an_earlier_rule_map_writes_need_no_layer() {
        let rules = RuleSet::builder()
            .rule_map(
                "/r/one.tmx",
                rule_map(&[("input_set", "1"), ("output_Ground", "2")]),
            )
            .rule_map(
                "/r/two.tmx",
                rule_map(&[("input_Ground", "2"), ("output_Over", "3")]),
            )
            .build();
        let layers = rules.rule_layers();
        assert_eq!(layers.inputs, ["set", "Ground"]);
        assert_eq!(layers.tile_outputs, ["Ground", "Over"]);
        assert!(layers.object_outputs.is_empty());
        assert_eq!(layers.made, ["Ground"]);
    }
}
