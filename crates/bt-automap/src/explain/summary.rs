//! The rules as written, for the AutoMap panel's lists (§8, `rule_summaries`): where each rule
//! is in its rule map, what it reads and writes, how many input sets and random outputs it has,
//! its options, and why it never matches when it does not.

use bt_utils::IRect;
use glam::IVec2;

use crate::rule_ref::RuleRef;
use crate::rules::RuleSet;

/// One rule, as written in its rule map ([`RuleSet::rule_summaries`]).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleSummary {
    /// The rule.
    pub rule: RuleRef,
    /// The box of its cells in the rule map, input and output.
    pub area: IRect,
    /// Its input box in the rule map: a match at a position puts this box's top-left there.
    pub input: IRect,
    /// The box of its output cells in the rule map.
    pub output: IRect,
    /// The layers its conditions read, by the names the rules use, in order.
    pub reads: Vec<String>,
    /// The layers it writes tiles to, then those it places objects on, in order.
    pub writes: Vec<String>,
    /// How many of its input sets can hold.
    pub input_sets: usize,
    /// How many random outputs it chooses from (those of weight above 0).
    pub random_outputs: usize,
    /// The chance it is tried at a position (`Probability`).
    pub probability: f64,
    /// Positions are tried every this many columns and rows (`ModX`, `ModY`).
    pub modulo: IVec2,
    /// The column and row the lattice goes through (`OffsetX`, `OffsetY`).
    pub offset: IVec2,
    /// Whether a match may not overlap the rule's earlier output (`NoOverlappingOutput`).
    pub no_overlapping_output: bool,
    /// Whether it writes to locked layers (`IgnoreLock`).
    pub ignore_lock: bool,
    /// Why it never matches: it is disabled, or the reason it can never match; `None` when it
    /// can.
    pub inactive: Option<String>,
}

impl RuleSet {
    /// Every rule of every rule map, in the order they run.
    pub fn rule_summaries(&self) -> Vec<RuleSummary> {
        let mut summaries = Vec::new();
        for (rule_map_index, rule_map) in self.rule_maps().iter().enumerate() {
            for (rule_index, rule) in rule_map.rules.iter().enumerate() {
                let mut reads: Vec<String> = Vec::new();
                for set in rule.input_sets.iter().filter(|s| s.never.is_none()) {
                    for layer in &set.layers {
                        if let Some(name) = rule_map.input_names.get(layer.target)
                            && !reads.contains(name)
                        {
                            reads.push(name.clone());
                        }
                    }
                }
                let outputs: Vec<_> = rule
                    .fixed
                    .iter()
                    .chain(rule.alternatives.iter().map(|a| &a.set))
                    .collect();
                let mut writes: Vec<String> = Vec::new();
                let tile_names = outputs
                    .iter()
                    .flat_map(|o| &o.tiles)
                    .filter_map(|t| rule_map.tile_outputs.get(t.target));
                let object_names = outputs
                    .iter()
                    .flat_map(|o| &o.objects)
                    .filter_map(|o| rule_map.object_outputs.get(o.target));
                for name in tile_names.chain(object_names) {
                    if !writes.contains(name) {
                        writes.push(name.clone());
                    }
                }
                let inactive = if rule.options.disabled {
                    Some("it is disabled (the Disabled option)".to_owned())
                } else {
                    rule.never_matches()
                };
                summaries.push(RuleSummary {
                    rule: RuleRef {
                        rule_map: rule_map_index,
                        rule: rule_index,
                    },
                    area: rule.bounds.rect(),
                    input: rule.input_box.translated(rule.origin).rect(),
                    output: rule
                        .output_cells
                        .bounding()
                        .map(|b| b.translated(rule.origin).rect())
                        .unwrap_or_default(),
                    reads,
                    writes,
                    input_sets: rule.input_sets.iter().filter(|s| s.never.is_none()).count(),
                    random_outputs: rule.picker.len(),
                    probability: rule.options.probability,
                    modulo: IVec2::new(rule.options.mod_x, rule.options.mod_y),
                    offset: IVec2::new(rule.options.offset_x, rule.options.offset_y),
                    no_overlapping_output: rule.options.no_overlapping_output,
                    ignore_lock: rule.options.ignore_lock,
                    inactive,
                });
            }
        }
        summaries
    }
}
