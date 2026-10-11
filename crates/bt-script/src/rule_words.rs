//! How reports speak of AutoMap rules (`10-agents.md`, §10.1, §10.6): a rule by its rule map
//! and number, the layers rules read by their exact names with the near miss a map has, and a
//! rule set's problems as warning and note lines.

use std::path::Path;

use bt_automap::{Problem, RuleSet, Severity};
use bt_core::{Layer, Map};
use bt_utils::paths::display_name;
use bt_utils::text::slug;

use crate::report::Line;

/// A rule map by its file name, or by its number when it has none.
pub(crate) fn rule_map_name(rules: &RuleSet, index: usize) -> String {
    rules.rule_maps().get(index).map_or_else(
        || format!("rule map {}", index + 1),
        |rule_map| display_name(rule_map.path()),
    )
}

/// A problem of a rule set as a line: errors and warnings as warnings, notes as notes.
pub(crate) fn problem_line(problem: &Problem) -> Line {
    match problem.severity() {
        Severity::Error | Severity::Warning => Line::warning(problem.text.clone()),
        Severity::Note => Line::note(problem.text.clone()),
    }
}

/// The tile layer of `map` the rules find by `name`: the bottom-most of that exact name, as
/// Tiled looks layers up.
pub(crate) fn tile_layer_named<'m>(map: &'m Map, name: &str) -> Option<&'m Layer> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if found.is_none() && layer.name == name && layer.as_tile_layer().is_some() {
            found = Some(layer);
        }
    });
    found
}

/// A tile layer of `map` named almost `name`: the same but for case and separators, else one
/// of the closest names. `None` when the map has `name` itself or nothing like it.
pub(crate) fn near_miss_layer(map: &Map, name: &str) -> Option<String> {
    if tile_layer_named(map, name).is_some() {
        return None;
    }
    let mut names: Vec<&str> = Vec::new();
    map.for_each_layer(|layer, _| {
        if layer.as_tile_layer().is_some() {
            names.push(layer.name.as_str());
        }
    });
    let wanted = slug(name);
    if let Some(same) = names.iter().find(|other| slug(other) == wanted) {
        return Some((*same).to_owned());
    }
    bt_utils::text::fuzzy_match(name, &names)
        .first()
        .map(|close| (*close).to_owned())
}

/// The rules file as a report names it.
pub(crate) fn rules_file_name(path: &Path) -> String {
    display_name(path)
}

#[cfg(test)]
mod tests {
    use bt_automap::ProblemKind;
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    #[test]
    fn a_layer_named_almost_like_the_rules_say_is_a_near_miss() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tile_layer("Set", "")
            .tile_layer("ground", "")
            .build()
            .unwrap();
        assert_eq!(near_miss_layer(&map, "set").as_deref(), Some("Set"));
        assert_eq!(near_miss_layer(&map, "Set"), None, "the map has it");
        assert_eq!(near_miss_layer(&map, "grund").as_deref(), Some("ground"));
        assert_eq!(near_miss_layer(&map, "walls"), None);
        assert_eq!(
            problem_line(&Problem::new(ProblemKind::OrphanInput, "tiles")).kind,
            crate::report::LineKind::Note
        );
    }
}
