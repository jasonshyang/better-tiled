//! A compiled rule: where it is, what it asks of the map and what it writes there.

use bt_utils::rng::RandomPicker;
use glam::IVec2;

use crate::bx::Bx;
use crate::cell_set::CellSet;
use crate::rulemap::inputs::{InputSet, Never};
use crate::rulemap::options::RuleOptions;
use crate::rulemap::outputs::OutputSet;

/// One of a rule's random outputs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Alternative {
    /// What it writes and places.
    pub(crate) set: OutputSet,
    /// Its weight, the `Probability` of its layers.
    pub(crate) weight: f64,
}

/// A rule. Every offset in it is relative to `origin`, the top-left of its input box in the rule
/// map; a match at a position puts that corner there.
#[derive(Clone, Debug)]
pub(crate) struct Rule {
    /// The top-left of the input box in the rule map.
    pub(crate) origin: IVec2,
    /// The input box, relative to `origin`: from (0, 0) to its size less one.
    pub(crate) input_box: Bx,
    /// The box of all its cells, input and output, in the rule map.
    pub(crate) bounds: Bx,
    /// Its output cells, relative to `origin`: what a match adds to the region later rule maps
    /// run on, where the region grows.
    pub(crate) output_cells: CellSet,
    /// The top-left of the output box in the rule map, from which objects are placed.
    pub(crate) output_origin: IVec2,
    /// Its options.
    pub(crate) options: RuleOptions,
    /// Its input sets; it matches where any of them holds.
    pub(crate) input_sets: Vec<InputSet>,
    /// The output always applied: the unnamed index's (Tiled 1.11).
    pub(crate) fixed: Option<OutputSet>,
    /// The random outputs, one of which is applied per match.
    pub(crate) alternatives: Vec<Alternative>,
    /// The weighted choice among `alternatives`, by index; those of weight 0 are left out.
    pub(crate) picker: RandomPicker<usize>,
}

impl Rule {
    /// Whether the rule has anything to apply.
    pub(crate) fn has_output(&self) -> bool {
        self.fixed.is_some() || !self.picker.is_empty()
    }

    /// Why the rule can never match, if it cannot: no output, or every input set impossible.
    pub(crate) fn never_matches(&self) -> Option<String> {
        if !self.has_output() {
            return Some("it has no output that can be chosen".to_owned());
        }
        if self.input_sets.is_empty() {
            return Some("it has no input set".to_owned());
        }
        let reasons: Vec<String> = self
            .input_sets
            .iter()
            .map(|set| match &set.never {
                Some(Never::Contradiction { layer, at }) => Some(format!(
                    "input set “{}” asks for tiles it also forbids on layer “{layer}” at \
                     ({}, {}) of the rule",
                    set.index, at.x, at.y
                )),
                Some(Never::NoConditions) => Some(format!(
                    "input set “{}” has no condition and no Ignore tile",
                    set.index
                )),
                None => None,
            })
            .collect::<Option<Vec<String>>>()?;
        Some(reasons.join("; "))
    }
}
