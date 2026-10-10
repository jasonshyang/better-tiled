//! A rule set: the rule maps that apply to a map, compiled, in the order they run, with the
//! tileset table their rules are numbered in. `files::load` builds one from a `rules.txt`; tests
//! and the editor's in-memory rule maps build one with [`RuleSet::builder`].

mod report;

use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{CustomTypes, Map};

use crate::problem::Problem;
use crate::rulemap::{CompileContext, MatchTypeCache, RuleMap, TilesetTable, compile_rule_map};

pub use report::{LockedOutput, Report};

/// The compiled rule maps of a rule set and what is derived from them.
#[derive(Clone, Debug)]
pub struct RuleSet {
    rule_maps: Vec<RuleMap>,
    pub(crate) table: TilesetTable,
    pub(crate) match_types: MatchTypeCache,
    pub(crate) types: Arc<CustomTypes>,
    problems: Vec<Problem>,
}

impl RuleSet {
    /// Sets up a rule set from rule maps in memory.
    pub fn builder() -> RuleSetBuilder {
        RuleSetBuilder::default()
    }

    /// The rule maps, in the order they run (a rule map listed twice is here twice).
    pub fn rule_maps(&self) -> &[RuleMap] {
        &self.rule_maps
    }

    /// The problems of the rule set as a whole: rule maps that could not be read, with what
    /// was kept of them. Each rule map has its own ([`RuleMap::problems`]).
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// How many rules there are in all.
    pub fn rule_count(&self) -> usize {
        self.rule_maps.iter().map(RuleMap::rule_count).sum()
    }

    /// The custom types the rules' tiles took their classes' `MatchType` from.
    pub fn custom_types(&self) -> &Arc<CustomTypes> {
        &self.types
    }
}

/// A rule map to compile, with the problems found reading it.
#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    map: Arc<Map>,
    problems: Vec<Problem>,
}

/// Sets up a [`RuleSet`]: its rule maps in the order they run, and the custom types that give
/// tiles their classes' `MatchType`.
#[derive(Clone, Debug, Default)]
pub struct RuleSetBuilder {
    types: Arc<CustomTypes>,
    entries: Vec<Entry>,
    problems: Vec<Problem>,
}

impl RuleSetBuilder {
    /// The project's custom types (none by default).
    pub fn custom_types(mut self, types: Arc<CustomTypes>) -> RuleSetBuilder {
        self.types = types;
        self
    }

    /// Adds a rule map, read from `path`, to run after those added before.
    pub fn rule_map(self, path: impl Into<PathBuf>, map: Arc<Map>) -> RuleSetBuilder {
        self.rule_map_with_problems(path, map, Vec::new())
    }

    /// Adds a rule map with the problems found reading it.
    pub(crate) fn rule_map_with_problems(
        mut self,
        path: impl Into<PathBuf>,
        map: Arc<Map>,
        problems: Vec<Problem>,
    ) -> RuleSetBuilder {
        self.entries.push(Entry {
            path: path.into(),
            map,
            problems,
        });
        self
    }

    /// Adds a problem of the rule set as a whole.
    pub(crate) fn problem(mut self, problem: Problem) -> RuleSetBuilder {
        self.problems.push(problem);
        self
    }

    /// Compiles the rule maps.
    pub fn build(self) -> RuleSet {
        let mut table = TilesetTable::new();
        let mut match_types = MatchTypeCache::default();
        let mut rule_maps = Vec::with_capacity(self.entries.len());
        for (index, entry) in self.entries.into_iter().enumerate() {
            let mut context = CompileContext {
                table: &mut table,
                types: &self.types,
                cache: &mut match_types,
            };
            let mut rule_map = compile_rule_map(entry.map, entry.path, index, &mut context);
            for problem in entry.problems {
                rule_map.record_problem(problem);
            }
            rule_maps.push(rule_map);
        }
        RuleSet {
            rule_maps,
            table,
            match_types,
            types: self.types,
            problems: self.problems,
        }
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{Property, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
    use glam::UVec2;

    use super::*;
    use crate::problem::ProblemKind;

    fn rule_map(output: &str) -> Arc<Map> {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        Arc::new(
            MapBuilder::new(UVec2::splat(16), UVec2::new(3, 1))
                .tileset(atlas)
                .tileset(rule_tiles().unwrap())
                .tile_layer("input_set", "2 . .")
                .tile_layer("output_set", output)
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn rule_maps_are_compiled_in_order_with_their_problems() {
        let mut odd = Map::clone(&rule_map("3 . ."));
        odd.properties
            .insert("Colour".into(), Property::new(Value::Int(3)));
        let set = RuleSet::builder()
            .rule_map("/rules/a.tmx", rule_map("3 . ."))
            .rule_map_with_problems(
                "/rules/b.tmx",
                Arc::new(odd),
                vec![Problem::new(ProblemKind::FileWarning, "a warning")],
            )
            .problem(Problem::new(ProblemKind::UnreadableRuleMap, "c.tmx"))
            .build();
        let paths: Vec<&str> = set
            .rule_maps()
            .iter()
            .map(|r| r.path().to_str().unwrap())
            .collect();
        assert_eq!(paths, ["/rules/a.tmx", "/rules/b.tmx"]);
        assert_eq!(set.rule_count(), 2);
        let kinds: Vec<ProblemKind> = set.rule_maps()[1]
            .problems()
            .iter()
            .map(|p| p.kind)
            .collect();
        assert_eq!(
            kinds,
            [ProblemKind::UnknownProperty, ProblemKind::FileWarning]
        );
        assert_eq!(set.problems().len(), 1);
        // Both rule maps' tilesets are different values here, so the table holds four.
        assert_eq!(set.table.entries().len(), 4);
    }
}
