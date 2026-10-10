//! After a run (§9): the rule maps that matched nothing, with the reasons their inputs give,
//! which the editor turns into "why the rules did less than they could". A layer a rule map
//! reads is missing when the map has no layer for it (as the run's layer names find layers) and
//! no earlier rule map writes it, so it read as empty; it is empty when it held no tile when the
//! rule map ran: empty on the map and written by no earlier rule map that applied anything. When
//! neither holds, the rule map's inputs held tiles and simply no rule matched them.

use bt_core::{Layer, Map};

use crate::engine::{RunRequest, Unbound, find_layer, is_tiles};
use crate::problem::{Problem, ProblemKind};
use crate::rules::{Report, RuleSet};

/// A rule map that matched nothing in a run, with what its inputs say about why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleRuleMap {
    /// The rule map, by its index in the rule set.
    pub rule_map: usize,
    /// The layers it reads that the map lacks and no earlier rule map writes: they read as
    /// empty, so no rule needing a tile there can match.
    pub missing_layers: Vec<String>,
    /// The layers it reads that held no tile when it ran.
    pub empty_layers: Vec<String>,
}

impl IdleRuleMap {
    /// The idle rule map as problems: one for each missing and each empty layer, or, when its
    /// inputs held tiles, one saying that no rule matched them.
    pub fn problems(&self, set: &RuleSet) -> Vec<Problem> {
        let name = set
            .rule_maps()
            .get(self.rule_map)
            .map_or_else(String::new, |r| {
                let file = r.path().file_name().map_or_else(
                    || r.path().display().to_string(),
                    |file| file.to_string_lossy().into_owned(),
                );
                format!(" ({file})")
            });
        let rule_map = format!("rule map {}{name}", self.rule_map + 1);
        let mut problems: Vec<Problem> = self
            .missing_layers
            .iter()
            .map(|layer| {
                Problem::new(
                    ProblemKind::IdleMissingLayer,
                    format!(
                        "{rule_map} matched nothing: it reads layer “{layer}”, which the map does \
                         not have, so it reads as empty"
                    ),
                )
            })
            .collect();
        problems.extend(self.empty_layers.iter().map(|layer| {
            Problem::new(
                ProblemKind::IdleEmptyLayer,
                format!(
                    "{rule_map} matched nothing: layer “{layer}”, which it reads, held no tile \
                     when it ran"
                ),
            )
        }));
        if problems.is_empty() {
            problems.push(Problem::new(
                ProblemKind::IdleNoMatch,
                format!(
                    "{rule_map} matched nothing: its input layers held tiles, but no rule \
                     matched them (Explain shows why at a cell)"
                ),
            ));
        }
        problems
    }
}

impl RuleSet {
    /// The rule maps that matched nothing in the run `report` describes, which ran on `map`
    /// with `request`'s layer names, with the reasons their inputs give. Rule maps that are not
    /// used or have no rules are left out: their own problems say why. The counts are the
    /// run's: after a region or selection run, what it tried.
    pub fn idle_rule_maps(
        &self,
        map: &Map,
        request: &RunRequest,
        report: &Report,
    ) -> Vec<IdleRuleMap> {
        let names = request.layer_names();
        let mut written_before: Vec<&str> = Vec::new();
        let mut applied_before: Vec<&str> = Vec::new();
        let mut idle = Vec::new();
        for (index, rule_map) in self.rule_maps().iter().enumerate() {
            if !rule_map.is_used() {
                continue;
            }
            let matches = report.rule_map_matches.get(index).copied();
            if matches == Some(0) && rule_map.rule_count() > 0 {
                let mut found = IdleRuleMap {
                    rule_map: index,
                    missing_layers: Vec::new(),
                    empty_layers: Vec::new(),
                };
                for name in rule_map.inputs() {
                    let layer = match names.tile_layer(name) {
                        Some(id) => map.layer(id),
                        None => match names.unbound() {
                            Unbound::LookUpByName => find_layer(map, name, is_tiles),
                            Unbound::Temporary => None,
                        },
                    };
                    let has_tiles = layer
                        .and_then(Layer::as_tile_layer)
                        .is_some_and(|tiles| tiles.cells().next().is_some());
                    if layer.is_none() && !written_before.contains(&name.as_str()) {
                        found.missing_layers.push(name.clone());
                    } else if !has_tiles && !applied_before.contains(&name.as_str()) {
                        found.empty_layers.push(name.clone());
                    }
                }
                idle.push(found);
            }
            let outputs = rule_map.tile_outputs().iter().map(String::as_str);
            written_before.extend(outputs.clone());
            if matches.is_some_and(|m| m > 0) {
                applied_before.extend(outputs);
            }
        }
        idle
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{LayerId, Map};
    use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
    use glam::UVec2;

    use super::*;
    use crate::engine::LayerNames;

    fn rule_map(input: &str, output: &str) -> Arc<Map> {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        Arc::new(
            MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
                .tileset(atlas)
                .tileset(rule_tiles().unwrap())
                .tile_layer(&format!("input_{input}"), "1")
                .tile_layer(&format!("output_{output}"), "2")
                .build()
                .unwrap(),
        )
    }

    fn kinds(set: &RuleSet, idle: &[IdleRuleMap]) -> Vec<ProblemKind> {
        idle.iter()
            .flat_map(|i| i.problems(set))
            .map(|p| p.kind)
            .collect()
    }

    #[test]
    fn rule_maps_that_matched_nothing_say_what_their_inputs_held() {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        // Reads "ground" (holds a 3), "water" (missing), "sky" (empty), and "made", which the
        // first rule map writes.
        let set = RuleSet::builder()
            .rule_map("/r/ground.tmx", rule_map("ground", "made"))
            .rule_map("/r/water.tmx", rule_map("water", "deco"))
            .rule_map("/r/sky.tmx", rule_map("sky", "deco"))
            .rule_map("/r/made.tmx", rule_map("made", "deco"))
            .build();
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 1))
            .tileset(atlas)
            .tile_layer("ground", "3 .")
            .tile_layer("sky", ". .")
            .build()
            .unwrap();
        let request = RunRequest::builder(1).build();
        let before = map.clone();
        let report = set.run(&mut map, &request);
        assert_eq!(report.rule_map_matches, [0, 0, 0, 0]);
        let idle = set.idle_rule_maps(&before, &request, &report);
        let found: Vec<(usize, Vec<String>, Vec<String>)> = idle
            .iter()
            .map(|i| (i.rule_map, i.missing_layers.clone(), i.empty_layers.clone()))
            .collect();
        assert_eq!(
            found,
            [
                (0, vec![], vec![]),
                (1, vec!["water".to_owned()], vec![]),
                (2, vec![], vec!["sky".to_owned()]),
                (3, vec![], vec!["made".to_owned()]),
            ]
        );
        assert_eq!(
            kinds(&set, &idle),
            [
                ProblemKind::IdleNoMatch,
                ProblemKind::IdleMissingLayer,
                ProblemKind::IdleEmptyLayer,
                ProblemKind::IdleEmptyLayer
            ]
        );
        assert!(
            idle[1].problems(&set)[0]
                .text
                .contains("rule map 2 (water.tmx)")
        );
    }

    #[test]
    fn layers_an_earlier_rule_map_filled_or_bindings_find_are_not_reasons() {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let set = RuleSet::builder()
            .rule_map("/r/ground.tmx", rule_map("ground", "made"))
            .rule_map("/r/made.tmx", rule_map("made", "deco"))
            .build();
        // Ground holds a 1: the first rule map writes "made", which the second reads (and
        // finds a 2, which it does not want).
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
            .tileset(Arc::clone(&atlas))
            .tile_layer("ground", "1")
            .build()
            .unwrap();
        let request = RunRequest::builder(1).build();
        let before = map.clone();
        let report = set.run(&mut map, &request);
        assert_eq!(report.rule_map_matches, [1, 0]);
        let idle = set.idle_rule_maps(&before, &request, &report);
        assert_eq!(idle.len(), 1);
        assert!(idle[0].missing_layers.is_empty() && idle[0].empty_layers.is_empty());
        // Bound to a layer of another name, the input is not missing.
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
            .tileset(atlas)
            .tile_layer("plat1", "5")
            .build()
            .unwrap();
        let request = RunRequest::builder(1)
            .layer_names(LayerNames::builder().tiles("ground", LayerId(1)).build())
            .build();
        let mut ran = map.clone();
        let report = set.run(&mut ran, &request);
        let idle = set.idle_rule_maps(&map, &request, &report);
        assert_eq!(idle[0].rule_map, 0);
        assert!(idle[0].missing_layers.is_empty());
    }
}
