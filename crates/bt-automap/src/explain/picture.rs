//! A rule as it is drawn in its rule map (§8, `rule_picture`), for the AutoMap panel's
//! pictures: its input, inputnot and output tile layers, each with the rule's own cells (its
//! 8-connected component, not everything inside its box), special tiles included.

use bt_core::{Map, PlacedCell};
use bt_utils::IRect;

use crate::rule_ref::RuleRef;
use crate::rules::RuleSet;

/// What a rule map layer is for.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PictureRole {
    /// `input…_…`: tiles to find.
    Input,
    /// `inputnot…_…`: tiles not to find.
    InputNot,
    /// `output…_…`: tiles to write.
    Output,
}

/// A rule map layer within a rule.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureLayer {
    /// The layer's name in the rule map (`input_walls`, `outputA_walls`…).
    pub name: String,
    /// What it is for.
    pub role: PictureRole,
    /// The rule's cells on it that hold a tile, in the rule map's coordinates and tileset slots
    /// ([`RuleMap::map`](crate::RuleMap::map) draws them).
    pub cells: Vec<PlacedCell>,
}

/// A rule as drawn in its rule map ([`RuleSet::rule_picture`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RulePicture {
    /// The rule.
    pub rule: RuleRef,
    /// The box of the rule's cells in the rule map.
    pub area: IRect,
    /// Its input, inputnot and output tile layers with a tile in the rule, in the rule map's
    /// order.
    pub layers: Vec<PictureLayer>,
}

/// What a layer named `name` is for, as the rule map reader reads names.
fn role_of(name: &str) -> Option<PictureRole> {
    if name.starts_with("//") || !name.contains('_') {
        return None;
    }
    let starts = |prefix: &str| {
        name.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    if starts("regions") || name.eq_ignore_ascii_case("rule_options") {
        None
    } else if starts("output") {
        Some(PictureRole::Output)
    } else if starts("inputnot") {
        Some(PictureRole::InputNot)
    } else if starts("input") {
        Some(PictureRole::Input)
    } else {
        None
    }
}

impl RuleSet {
    /// `rule` as it is drawn in its rule map; `None` for a rule the set does not have.
    pub fn rule_picture(&self, rule: RuleRef) -> Option<RulePicture> {
        let rule_map = self.rule_maps().get(rule.rule_map)?;
        let compiled = rule_map.rules.get(rule.rule)?;
        let source: &Map = rule_map.map();
        let mut layers = Vec::new();
        source.for_each_layer(|layer, _| {
            let (Some(role), Some(tiles)) = (role_of(&layer.name), layer.as_tile_layer()) else {
                return;
            };
            let cells: Vec<PlacedCell> = compiled
                .cells
                .cells()
                .map(|at| PlacedCell {
                    at,
                    cell: tiles.cell(at),
                })
                .filter(|placed| !placed.cell.is_empty())
                .collect();
            if !cells.is_empty() {
                layers.push(PictureLayer {
                    name: layer.name.clone(),
                    role,
                    cells,
                });
            }
        });
        Some(RulePicture {
            rule,
            area: compiled.bounds.rect(),
            layers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_names_give_picture_roles_as_the_reader_reads_them() {
        assert_eq!(role_of("Input_walls"), Some(PictureRole::Input));
        assert_eq!(role_of("inputnotA_walls"), Some(PictureRole::InputNot));
        assert_eq!(role_of("output2_walls"), Some(PictureRole::Output));
        assert_eq!(role_of("regions_input"), None);
        assert_eq!(role_of("rule_options"), None);
        assert_eq!(role_of("// input_notes"), None);
        assert_eq!(role_of("walls"), None);
    }
}
