//! The locality analysis of §7: for each rule map, the box of offsets its result at a cell
//! depends on (the cells as they were before it ran), what each rule's matches depend on and
//! change, and whether the rule map is local at all.
//!
//! In the default mode every rule matches against the cells as they were before the rule map, so
//! a written cell depends on what the matches writing it read: the rules' read offsets less their
//! write offsets. With `MatchInOrder` a rule also sees what earlier rules wrote, so the analysis
//! follows read-after-write chains, counting only writes that can change a later condition
//! ([`ConditionLogic::can_change`]). A rule whose matches can change whether it matches at a
//! position it tries later makes the rule map non-local, and so does `NoOverlappingOutput` on a
//! rule two of whose matches can overlap: in both, a chain of matches can run across the map.
//!
//! The analysis also notes what ties a run on an infinite map to the region Tiled grows there: a
//! rule that can match where every cell it reads is empty, and a rule two of whose matches can
//! write different tiles into one cell (then the order its positions are tried in matters).

use std::collections::HashSet;

use bt_core::Map;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::region::condition::{Accepted, ConditionLogic};
use crate::rulemap::{Rule, RuleMap, RuleOptions, TableTile, TilesetTable};

/// What a rule's match at a position depends on and changes, relative to the position.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuleReach {
    /// The cells, as they were before the rule map ran, whose content decides whether the rule
    /// applies at a position; `None` when none does (it reads nothing, or it never matches).
    pub(crate) decision: Option<Bx>,
    /// The cells its matches write or place objects on; `None` when they change nothing.
    pub(crate) effect: Option<Bx>,
}

/// What the analysis found for one rule map.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RuleMapLocality {
    /// The offsets `d` such that the rule map's result at a cell depends only on the cells at
    /// `cell + d` before it ran, on every layer; it holds (0, 0).
    pub(crate) dependency: Bx,
    /// What each rule depends on and changes, by its index in the rule map.
    pub(crate) rules: Vec<RuleReach>,
    /// Why the rule map's result at a cell can depend on cells arbitrarily far away; `None` when
    /// it is local.
    pub(crate) nonlocal: Option<String>,
    /// A rule that can match where every cell it reads is empty, described.
    pub(crate) matches_on_empty: Option<String>,
    /// A rule two of whose matches can write different tiles into one cell, described.
    pub(crate) order_sensitive: Option<String>,
}

impl RuleMapLocality {
    /// A rule map that changes nothing (not used).
    fn idle(rule_count: usize) -> RuleMapLocality {
        RuleMapLocality {
            dependency: Bx::point(IVec2::ZERO),
            rules: vec![RuleReach::default(); rule_count],
            nonlocal: None,
            matches_on_empty: None,
            order_sensitive: None,
        }
    }
}

/// The locality of every rule map of a rule set.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Locality {
    /// Each rule map's, in run order.
    pub(crate) rule_maps: Vec<RuleMapLocality>,
}

impl Locality {
    /// Analyses `rule_maps`, whose tiles are numbered in `table`.
    pub(crate) fn of(rule_maps: &[RuleMap], table: &TilesetTable) -> Locality {
        let logic = ConditionLogic::new(table);
        Locality {
            rule_maps: rule_maps
                .iter()
                .map(|rule_map| analyze_rule_map(rule_map, &logic))
                .collect(),
        }
    }

    /// The offsets from an edited cell to the cells whose result a full run can change: every
    /// rule map's dependency box mirrored, summed.
    pub(crate) fn influence(&self) -> Bx {
        self.rule_maps
            .iter()
            .fold(Bx::point(IVec2::ZERO), |influence, rule_map| {
                influence.sum(rule_map.dependency.negated())
            })
    }

    /// What a rule's matches depend on and change.
    pub(crate) fn reach(&self, rule_map: usize, rule: usize) -> RuleReach {
        self.rule_maps
            .get(rule_map)
            .and_then(|r| r.rules.get(rule))
            .copied()
            .unwrap_or_default()
    }
}

/// A cell of a layer, relative to a match's position.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct CellKey {
    /// The layer, by the rule map's numbering of the names it reads and writes.
    layer: usize,
    /// The offset.
    offset: IVec2,
}

/// A condition of an input set.
#[derive(Copy, Clone, Debug)]
struct Condition<'r> {
    at: CellKey,
    accepted: Accepted<'r>,
}

/// A cell a rule writes, with what the cell can hold just before a match writes it.
#[derive(Clone, Debug)]
struct Write<'r> {
    at: CellKey,
    /// The tile written; `None` erases.
    value: Option<TableTile>,
    /// What the cell holds when the rule matches: its input sets' conditions there.
    before: Vec<Accepted<'r>>,
}

/// A layer a footprint is on, for `NoOverlappingOutput`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum FootprintLayer {
    Tiles(usize),
    Objects(usize),
}

/// A rule's footprint on one layer.
#[derive(Clone, Debug)]
struct Footprint {
    layer: FootprintLayer,
    cells: CellSet,
}

/// What the analysis needs of a rule that can match.
struct RuleShape<'r> {
    index: usize,
    rule: &'r Rule,
    /// The conditions of each input set that can hold.
    sets: Vec<Vec<Condition<'r>>>,
    /// Whether an input set holds everywhere (it has only `Ignore` tiles).
    unconditional: bool,
    /// The box of the cells its conditions read.
    reads: Option<Bx>,
    writes: Vec<Write<'r>>,
    /// The box of the cells it writes or places objects on.
    effect: Option<Bx>,
    footprints: Vec<Footprint>,
}

/// The rule map's numbering of the layer names it reads and writes: a name read and written is
/// one layer.
#[derive(Default)]
struct LayerIds {
    names: Vec<String>,
}

impl LayerIds {
    fn id(&mut self, name: &str) -> usize {
        match self.names.iter().position(|n| n == name) {
            Some(index) => index,
            None => {
                self.names.push(name.to_owned());
                self.names.len() - 1
            }
        }
    }
}

impl<'r> RuleShape<'r> {
    /// The shape of rule `index` of `rule_map`; `None` when it never matches.
    fn of(
        index: usize,
        rule: &'r Rule,
        rule_map: &RuleMap,
        ids: &mut LayerIds,
    ) -> Option<RuleShape<'r>> {
        if rule.options.disabled || !rule.has_output() {
            return None;
        }
        let mut sets = Vec::new();
        for set in rule.input_sets.iter().filter(|s| s.never.is_none()) {
            let mut conditions = Vec::new();
            for layer in &set.layers {
                let name = rule_map
                    .input_names
                    .get(layer.target)
                    .map(String::as_str)
                    .unwrap_or_default();
                let id = ids.id(name);
                for position in &layer.positions {
                    let accepted = if position.any > 0 {
                        Accepted::AnyOf(set.any_of(position))
                    } else {
                        Accepted::NoneOf(set.none_of(position))
                    };
                    conditions.push(Condition {
                        at: CellKey {
                            layer: id,
                            offset: position.offset,
                        },
                        accepted,
                    });
                }
            }
            sets.push(conditions);
        }
        if sets.is_empty() {
            return None;
        }
        let unconditional = sets.iter().any(Vec::is_empty);
        let reads = Bx::bounding(sets.iter().flatten().map(|c| c.at.offset));
        let mut writes: Vec<Write<'r>> = Vec::new();
        let mut footprints: Vec<Footprint> = Vec::new();
        let mut effect: Option<Bx> = None;
        let grow = |effect: &mut Option<Bx>, b: Bx| {
            *effect = Some(effect.map_or(b, |e| e.union(b)));
        };
        let outputs = rule
            .fixed
            .iter()
            .chain(rule.alternatives.iter().map(|a| &a.set));
        for output in outputs {
            for tiles in &output.tiles {
                let name = rule_map
                    .tile_outputs
                    .get(tiles.target)
                    .map(String::as_str)
                    .unwrap_or_default();
                let id = ids.id(name);
                for write in &tiles.writes {
                    let at = CellKey {
                        layer: id,
                        offset: write.offset,
                    };
                    grow(&mut effect, Bx::point(write.offset));
                    if !writes.iter().any(|w| w.at == at && w.value == write.tile) {
                        writes.push(Write {
                            at,
                            value: write.tile,
                            before: Vec::new(),
                        });
                    }
                }
                add_footprint(&mut footprints, FootprintLayer::Tiles(id), &tiles.footprint);
            }
            for objects in &output.objects {
                if let Some(cells) = objects.footprint.bounding() {
                    grow(&mut effect, cells);
                }
                add_footprint(
                    &mut footprints,
                    FootprintLayer::Objects(objects.target),
                    &objects.footprint,
                );
            }
        }
        let mut shape = RuleShape {
            index,
            rule,
            sets,
            unconditional,
            reads,
            writes,
            effect,
            footprints,
        };
        let befores: Vec<Vec<Accepted<'r>>> =
            shape.writes.iter().map(|w| shape.before(w.at)).collect();
        for (write, before) in shape.writes.iter_mut().zip(befores) {
            write.before = before;
        }
        Some(shape)
    }

    /// What the cell at `at` holds when the rule matches: the conditions there of its input
    /// sets (one of which holds), or every cell when a set has none there.
    fn before(&self, at: CellKey) -> Vec<Accepted<'r>> {
        if self.unconditional {
            return vec![Accepted::Every];
        }
        let mut before: Vec<Accepted<'r>> = Vec::new();
        for set in &self.sets {
            match set.iter().find(|c| c.at == at) {
                Some(condition) => before.push(condition.accepted),
                None => return vec![Accepted::Every],
            }
        }
        before
    }

    /// The rule, as problems name it.
    fn describe(&self) -> String {
        format!(
            "rule {} at ({}, {})",
            self.index + 1,
            self.rule.bounds.min.x,
            self.rule.bounds.min.y
        )
    }

    /// Whether two matches, at `p` and `p + d`, can both match the same cells.
    fn can_both_match(&self, d: IVec2, logic: &ConditionLogic) -> bool {
        if self.unconditional {
            return true;
        }
        self.sets.iter().any(|first| {
            self.sets.iter().any(|second| {
                second.iter().all(|c| {
                    let at = CellKey {
                        layer: c.at.layer,
                        offset: c.at.offset + d,
                    };
                    first
                        .iter()
                        .find(|f| f.at == at)
                        .is_none_or(|f| logic.meets(f.accepted, c.accepted))
                })
            })
        })
    }

    /// Whether two of its matches can overlap (`NoOverlappingOutput`); with `assume_matches`
    /// any two can match, else they must match the same cells.
    fn can_overlap(&self, assume_matches: bool, logic: &ConditionLogic) -> bool {
        let mut tried: HashSet<IVec2> = HashSet::new();
        for footprint in &self.footprints {
            let cells: Vec<IVec2> = footprint.cells.cells().collect();
            for &a in &cells {
                for &b in &cells {
                    let d = a - b;
                    if d == IVec2::ZERO
                        || !is_lattice_step(d, &self.rule.options)
                        || !tried.insert(d)
                    {
                        continue;
                    }
                    if assume_matches || self.can_both_match(d, logic) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether it can match where every cell it reads is empty.
    fn matches_on_empty(&self) -> bool {
        self.unconditional
            || self.sets.iter().any(|set| {
                set.iter()
                    .all(|c| ConditionLogic::accepts_empty(c.accepted))
            })
    }

    /// Whether two of its matches can write different tiles into one cell.
    fn writes_conflict(&self, logic: &ConditionLogic) -> bool {
        self.writes.iter().any(|first| {
            self.writes.iter().any(|second| {
                let d = first.at.offset - second.at.offset;
                first.at.layer == second.at.layer
                    && first.value != second.value
                    && d != IVec2::ZERO
                    && is_lattice_step(d, &self.rule.options)
                    && self.can_both_match(d, logic)
            })
        })
    }
}

fn add_footprint(footprints: &mut Vec<Footprint>, layer: FootprintLayer, cells: &CellSet) {
    match footprints.iter_mut().find(|f| f.layer == layer) {
        Some(footprint) => footprint.cells.insert_set(cells),
        None => footprints.push(Footprint {
            layer,
            cells: cells.clone(),
        }),
    }
}

/// Whether `d` leads from a position on the rule's `ModX`/`ModY` lattice to another.
fn is_lattice_step(d: IVec2, options: &RuleOptions) -> bool {
    d.x.rem_euclid(options.mod_x.max(1)) == 0 && d.y.rem_euclid(options.mod_y.max(1)) == 0
}

/// Whether a position `d` away is tried before, row by row.
fn is_scanned_before(d: IVec2) -> bool {
    d.y < 0 || (d.y == 0 && d.x < 0)
}

/// Analyses one rule map.
pub(crate) fn analyze_rule_map(rule_map: &RuleMap, logic: &ConditionLogic) -> RuleMapLocality {
    let mut locality = RuleMapLocality::idle(rule_map.rules.len());
    if !rule_map.is_used() {
        return locality;
    }
    let mut ids = LayerIds::default();
    let shapes: Vec<RuleShape<'_>> = rule_map
        .rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| RuleShape::of(index, rule, rule_map, &mut ids))
        .collect();
    let in_order = rule_map.options.match_in_order;
    for (position, shape) in shapes.iter().enumerate() {
        let mut decision = shape.reads;
        let mut self_affecting = false;
        if in_order {
            for writer in shapes.iter().take(position + 1) {
                for write in &writer.writes {
                    for condition in shape
                        .sets
                        .iter()
                        .flatten()
                        .filter(|c| c.at.layer == write.at.layer)
                    {
                        if !logic.can_change(write.value, &write.before, condition.accepted) {
                            continue;
                        }
                        let d = condition.at.offset - write.at.offset;
                        if writer.index == shape.index {
                            if d == IVec2::ZERO || !is_lattice_step(d, &shape.rule.options) {
                                continue;
                            }
                            if is_scanned_before(d) {
                                locality.nonlocal = Some(format!(
                                    "with MatchInOrder, {} can change whether it matches at a \
                                     position it tries later, so its matches can chain across \
                                     the map",
                                    shape.describe()
                                ));
                                return locality;
                            }
                            self_affecting = true;
                        } else if let Some(earlier) =
                            locality.rules.get(writer.index).and_then(|r| r.decision)
                        {
                            let moved = earlier.translated(d);
                            decision = Some(decision.map_or(moved, |b| b.union(moved)));
                        }
                    }
                }
            }
        }
        if shape.rule.options.no_overlapping_output
            && shape.can_overlap(in_order && self_affecting, logic)
        {
            locality.nonlocal = Some(format!(
                "with NoOverlappingOutput, two matches of {} can overlap, so whether one applies \
                 depends on a chain of matches across the map",
                shape.describe()
            ));
            return locality;
        }
        if locality.matches_on_empty.is_none() && shape.matches_on_empty() {
            locality.matches_on_empty = Some(shape.describe());
        }
        if locality.order_sensitive.is_none() && shape.writes_conflict(logic) {
            locality.order_sensitive = Some(shape.describe());
        }
        if let Some(reach) = locality.rules.get_mut(shape.index) {
            *reach = RuleReach {
                decision,
                effect: shape.effect,
            };
        }
        if let (Some(decision), Some(effect)) = (decision, shape.effect) {
            locality.dependency = locality.dependency.union(decision.sum(effect.negated()));
        }
    }
    locality
}

/// Whether a finite map's tile layers all have the map's size (clamped reads land on the map's
/// edge only then).
pub(crate) fn layers_have_map_size(map: &Map) -> bool {
    let mut same = true;
    map.for_each_layer(|layer, _| {
        if let Some(tiles) = layer.as_tile_layer()
            && (tiles.size().x != map.width || tiles.size().y != map.height)
        {
            same = false;
        }
    });
    same
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Property, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
    use glam::UVec2;

    use super::*;
    use crate::rules::RuleSet;

    fn bx(x0: i32, y0: i32, x1: i32, y1: i32) -> Bx {
        Bx {
            min: IVec2::new(x0, y0),
            max: IVec2::new(x1, y1),
        }
    }

    /// The locality of a rule set of one rule map with the given layers and options.
    fn locality_of(layers: &[(&str, &str)], options: &[(&str, Value)]) -> RuleMapLocality {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let width = layers[0]
            .1
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .count() as u32;
        let height = layers[0].1.lines().count() as u32;
        let mut builder = MapBuilder::new(UVec2::splat(16), UVec2::new(width, height))
            .tileset(atlas)
            .tileset(rule_tiles().unwrap());
        for (name, grid) in layers {
            builder = builder.tile_layer(name, grid);
        }
        let mut map = builder.build().unwrap();
        for (name, value) in options {
            map.properties
                .insert((*name).to_owned(), Property::new(value.clone()));
        }
        let set = RuleSet::builder()
            .rule_map("/rules/rules.tmx", Arc::new(map))
            .build();
        set.locality.rule_maps[0].clone()
    }

    #[test]
    fn a_written_cell_depends_on_what_the_rules_writing_it_read() {
        // Reads (0, 0) and (2, 0), writes (1, 0): the cell written depends on one cell either
        // side.
        let locality = locality_of(&[("input_set", "1 . 2"), ("output_set", ". 3 .")], &[]);
        assert_eq!(locality.nonlocal, None);
        assert_eq!(locality.dependency, bx(-1, 0, 1, 0));
        assert_eq!(
            locality.rules[0],
            RuleReach {
                decision: Some(bx(0, 0, 2, 0)),
                effect: Some(bx(1, 0, 1, 0)),
            }
        );
        assert_eq!(locality.matches_on_empty, None);
    }

    #[test]
    fn in_order_chains_count_only_writes_that_can_change_a_condition() {
        // Rule 1 turns a 1 followed by a 7 into a 2; rule 2 reads a 2 two cells right of a 4
        // (an Ignore tile joins them) and writes 5. Rule 2's decision depends on what rule 1
        // decided where it reads the 2, which depends on the cell right of that.
        let chain = locality_of(
            &[
                ("input_set", "1 7 . . 4 I 2"),
                ("output_set", "2 . . . 5 . ."),
            ],
            &[("MatchInOrder", Value::Bool(true))],
        );
        assert_eq!(chain.nonlocal, None);
        assert_eq!(chain.rules[0].decision, Some(bx(0, 0, 1, 0)));
        assert_eq!(chain.rules[1].decision, Some(bx(0, 0, 3, 0)));
        assert_eq!(chain.dependency, bx(0, 0, 3, 0));
        // Rule 1 now writes 3: "is 2" stays false where it writes, so nothing chains.
        let unrelated = locality_of(
            &[
                ("input_set", "1 7 . . 4 I 2"),
                ("output_set", "3 . . . 5 . ."),
            ],
            &[("MatchInOrder", Value::Bool(true))],
        );
        assert_eq!(unrelated.rules[1].decision, Some(bx(0, 0, 2, 0)));
        assert_eq!(unrelated.dependency, bx(0, 0, 2, 0));
        // Matched together, rule 2 never sees what rule 1 wrote.
        let together = locality_of(
            &[
                ("input_set", "1 7 . . 4 I 2"),
                ("output_set", "2 . . . 5 . ."),
            ],
            &[],
        );
        assert_eq!(together.rules[1].decision, Some(bx(0, 0, 2, 0)));
    }

    #[test]
    fn a_rule_feeding_its_own_later_matches_is_not_local() {
        // Extends a row of 1s to the right one cell per match: the match at x feeds x + 1.
        let spread = locality_of(
            &[("input_set", "1 E"), ("output_set", ". 1")],
            &[("MatchInOrder", Value::Bool(true))],
        );
        assert!(spread.nonlocal.is_some());
        // The same rule matching simultaneously is local.
        let together = locality_of(&[("input_set", "1 E"), ("output_set", ". 1")], &[]);
        assert_eq!(together.nonlocal, None);
        assert_eq!(together.dependency, bx(-1, 0, 0, 0));
        // Spreading to the left feeds only positions already tried: local.
        let left = locality_of(
            &[("input_set", "E 1"), ("output_set", "1 .")],
            &[("MatchInOrder", Value::Bool(true))],
        );
        assert_eq!(left.nonlocal, None);
    }

    #[test]
    fn overlapping_outputs_that_both_can_match_are_not_local() {
        let overlapping = locality_of(
            &[("input_set", "1 ."), ("output_set", "2 2")],
            &[("NoOverlappingOutput", Value::Bool(true))],
        );
        assert!(overlapping.nonlocal.is_some());
        // Two matches a cell apart would need a 1 and a 3 in one cell: they never both match.
        let exclusive = locality_of(
            &[("input_set", "1 3"), ("output_set", "2 2")],
            &[("NoOverlappingOutput", Value::Bool(true))],
        );
        assert_eq!(exclusive.nonlocal, None);
        // On a lattice two columns apart, matches never overlap.
        let spaced = locality_of(
            &[("input_set", "1 ."), ("output_set", "2 2")],
            &[
                ("NoOverlappingOutput", Value::Bool(true)),
                ("ModX", Value::Int(2)),
            ],
        );
        assert_eq!(spaced.nonlocal, None);
    }

    #[test]
    fn rules_matching_empty_cells_and_conflicting_writes_are_noted() {
        let empty = locality_of(&[("input_set", "E"), ("output_set", "2")], &[]);
        assert!(empty.matches_on_empty.is_some());
        let conflict = locality_of(&[("input_set", "1 ."), ("output_set", "2 3")], &[]);
        assert!(conflict.order_sensitive.is_some());
        let single = locality_of(&[("input_set", "1"), ("output_set", "2")], &[]);
        assert_eq!(single.order_sensitive, None);
        assert_eq!(single.matches_on_empty, None);
    }
}
