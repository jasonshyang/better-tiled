//! What is wrong or odd in a rule set against a map (§9 of the design). Before it runs:
//! tilesets the rules use that are copies of the map's, so rules naming their tiles never match
//! its tiles (Tiled says nothing and the rules silently do nothing), and layers the rules read
//! that the map lacks. After a run (`idle`): the rule maps that matched nothing, with what their
//! inputs say about why.

mod idle;

use std::collections::BTreeSet;
use std::path::PathBuf;

use bt_core::{Map, TilesetSlot};

use crate::engine::{TilesetBinding, find_layer, is_tiles};
use crate::problem::{Problem, ProblemKind};
use crate::rules::RuleSet;

pub use idle::IdleRuleMap;

/// A tileset the rules use that copies one of the map's: the same image and grid through
/// another file, or an alike collection. Tiled unifies only tilesets a rule map embeds, so the
/// rules' tiles are other tiles than the map's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilesetCopy {
    /// The rules' tileset, by its index in the rule set ([`RuleSet::tileset`]).
    pub tileset: usize,
    /// Its file; `None` when a rule map embeds it.
    pub rules_file: Option<PathBuf>,
    /// The map's tileset it copies.
    pub map_slot: TilesetSlot,
    /// That tileset's file; `None` when the map embeds it.
    pub map_file: Option<PathBuf>,
    /// The rule maps using the copy, by their index in the rule set.
    pub rule_maps: Vec<usize>,
}

impl TilesetCopy {
    /// The copy as a problem.
    pub fn problem(&self) -> Problem {
        let shown = |file: &Option<PathBuf>| {
            file.as_ref().map_or_else(
                || "a tileset embedded in it".to_owned(),
                |f| f.display().to_string(),
            )
        };
        Problem::new(
            ProblemKind::TilesetCopy,
            format!(
                "the rules use {} and the map {}, copies of one tileset: rules naming the \
                 rules' tiles never match the map's",
                shown(&self.rules_file),
                shown(&self.map_file)
            ),
        )
    }
}

/// A layer the rules read that the map has no tile layer for, and no earlier rule map writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingLayer {
    /// The name the rules use.
    pub name: String,
    /// The rule maps reading it, by their index in the rule set.
    pub rule_maps: Vec<usize>,
}

impl RuleSet {
    /// The tilesets of the rule set that the rules name tiles of.
    fn used_tilesets(&self) -> BTreeSet<usize> {
        let mut used = BTreeSet::new();
        for rule in self.rule_maps().iter().flat_map(|r| &r.rules) {
            for set in &rule.input_sets {
                used.extend(set.cells.iter().filter_map(|c| c.tile).map(|t| t.tileset));
            }
            let outputs = rule
                .fixed
                .iter()
                .chain(rule.alternatives.iter().map(|a| &a.set));
            for output in outputs {
                for tiles in &output.tiles {
                    used.extend(
                        tiles
                            .writes
                            .iter()
                            .filter_map(|w| w.tile)
                            .map(|t| t.tileset),
                    );
                }
                for objects in &output.objects {
                    used.extend(
                        objects
                            .objects
                            .iter()
                            .filter_map(|o| o.tile)
                            .map(|t| t.tileset),
                    );
                }
            }
        }
        used
    }

    /// The tilesets the rules use that copy one of `map`'s.
    pub fn tileset_copies(&self, map: &Map) -> Vec<TilesetCopy> {
        let (binding, _) = TilesetBinding::new(&self.table, map);
        let mut copies = Vec::new();
        for index in self.used_tilesets() {
            let Some(entry) = self.table.get(index) else {
                continue;
            };
            let bound_to_map = binding
                .slot_of(index)
                .is_some_and(|slot| slot.index() < map.tilesets.len());
            if bound_to_map {
                continue;
            }
            let copied = map
                .tilesets
                .iter()
                .position(|r| entry.reference.tileset.is_similar_to(&r.tileset));
            let Some(slot) = copied.and_then(TilesetSlot::new) else {
                continue;
            };
            copies.push(TilesetCopy {
                tileset: index,
                rules_file: entry.reference.file.as_ref().map(|f| f.path.clone()),
                map_slot: slot,
                map_file: map
                    .tilesets
                    .get(slot.index())
                    .and_then(|r| r.file.as_ref())
                    .map(|f| f.path.clone()),
                rule_maps: entry.rule_maps.clone(),
            });
        }
        copies
    }

    /// The layers the rules read that `map` has no tile layer for (by exact name), leaving out
    /// those an earlier rule map writes.
    pub fn missing_layers(&self, map: &Map) -> Vec<MissingLayer> {
        let mut written: BTreeSet<&str> = BTreeSet::new();
        let mut missing: Vec<MissingLayer> = Vec::new();
        for (index, rule_map) in self.rule_maps().iter().enumerate() {
            if !rule_map.is_used() {
                continue;
            }
            for name in rule_map.inputs() {
                if written.contains(name.as_str()) || find_layer(map, name, is_tiles).is_some() {
                    continue;
                }
                match missing.iter_mut().find(|m| m.name == *name) {
                    Some(entry) => {
                        if !entry.rule_maps.contains(&index) {
                            entry.rule_maps.push(index);
                        }
                    }
                    None => missing.push(MissingLayer {
                        name: name.clone(),
                        rule_maps: vec![index],
                    }),
                }
            }
            written.extend(rule_map.tile_outputs().iter().map(String::as_str));
        }
        missing
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{FileRef, Tileset, TilesetRef};
    use bt_test_data::fixtures::{Atlas, MapBuilder};
    use glam::UVec2;

    use super::*;

    fn atlas() -> Arc<Tileset> {
        Atlas::builder().build_embedded().unwrap().tileset
    }

    fn rule_map(tileset: TilesetRef, input: &str, output: &str) -> Arc<Map> {
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(1, 1))
            .tile_layer(&format!("input_{input}"), "2")
            .tile_layer(&format!("output_{output}"), "3")
            .build()
            .unwrap();
        map.tilesets.push(tileset);
        Arc::new(map)
    }

    #[test]
    fn layers_read_that_the_map_lacks_and_no_earlier_rule_map_writes_are_missing() {
        let tiles = atlas();
        let rules = RuleSet::builder()
            .rule_map(
                "/r/a.tmx",
                rule_map(TilesetRef::embedded(Arc::clone(&tiles)), "ground", "deco"),
            )
            .rule_map(
                "/r/b.tmx",
                rule_map(TilesetRef::embedded(Arc::clone(&tiles)), "deco", "walls"),
            )
            .rule_map(
                "/r/c.tmx",
                rule_map(TilesetRef::embedded(Arc::clone(&tiles)), "water", "walls"),
            )
            .build();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tileset(tiles)
            .tile_layer("ground", ". .\n. .")
            .build()
            .unwrap();
        assert_eq!(
            rules.missing_layers(&map),
            [MissingLayer {
                name: "water".into(),
                rule_maps: vec![2]
            }]
        );
    }

    #[test]
    fn a_rule_tileset_copying_the_maps_through_another_file_never_matches_and_is_reported() {
        let tiles = atlas();
        let file = |path: &str| TilesetRef {
            file: Some(FileRef::new(path, path)),
            first_gid: Some(1),
            tileset: Arc::new(Tileset::clone(&tiles)),
        };
        let rules = RuleSet::builder()
            .rule_map("/r/a.tmx", rule_map(file("/r/tiles.tsx"), "ground", "deco"))
            .rule_map(
                "/r/b.tmx",
                rule_map(
                    TilesetRef::embedded(Arc::new(Tileset::clone(&tiles))),
                    "ground",
                    "deco",
                ),
            )
            .build();
        let map = MapBuilder::new(UVec2::splat(16), UVec2::new(2, 2))
            .tile_layer("ground", ". .\n. .")
            .build()
            .unwrap();
        let mut map = map;
        map.tilesets.push(file("/m/tiles.tsx"));
        let copies = rules.tileset_copies(&map);
        // The embedded copy is unified with the map's, as Tiled does; the file is not.
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].rules_file, Some(PathBuf::from("/r/tiles.tsx")));
        assert_eq!(copies[0].map_file, Some(PathBuf::from("/m/tiles.tsx")));
        assert_eq!(copies[0].rule_maps, [0]);
        let problem = copies[0].problem();
        assert_eq!(problem.kind, ProblemKind::TilesetCopy);
        assert!(problem.text.contains("/r/tiles.tsx") && problem.text.contains("/m/tiles.tsx"));
    }
}
