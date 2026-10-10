//! Options as Tiled reads them (`AutoMapper::setupRuleMapProperties`, `checkRuleOptions`,
//! `setupInputLayerProperties`): names compared without regard to case, values converted as Qt
//! converts them. Where Qt would turn a value it cannot read into a silent 0 or `false`, the
//! option keeps its default and a warning names the property.

use bt_core::{Flip, Properties, Value};

use crate::problem::{Problem, ProblemKind};

/// The options a rule map sets for all its rules and for itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MapOptions {
    /// `DeleteTiles`: erase the input cells of the region from the output layers first.
    pub(crate) delete_tiles: bool,
    /// `MatchOutsideMap`, also implied by the border options.
    pub(crate) match_outside_map: bool,
    /// `OverflowBorder`: cells outside a finite map read as the nearest cell inside.
    pub(crate) overflow_border: bool,
    /// `WrapBorder`: cells outside a finite map read as the cell the map wraps to.
    pub(crate) wrap_border: bool,
    /// `AutomappingRadius`: how far beyond the region rules are tried.
    pub(crate) radius: i32,
    /// `MatchInOrder`: each rule sees the writes of the rules before it.
    pub(crate) match_in_order: bool,
    /// Whether the map sets `MatchInOrder` (legacy regions turn it on unless it does).
    pub(crate) match_in_order_set: bool,
    /// The options every rule starts from.
    pub(crate) rule_defaults: RuleOptions,
}

/// The options of one rule.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct RuleOptions {
    /// `Probability`: the chance the rule is tried at a position at all.
    pub(crate) probability: f64,
    /// `ModX`: positions are tried every this many columns; at least 1.
    pub(crate) mod_x: i32,
    /// `ModY`: positions are tried every this many rows; at least 1.
    pub(crate) mod_y: i32,
    /// `OffsetX`: the column the lattice of `ModX` goes through.
    pub(crate) offset_x: i32,
    /// `OffsetY`: the row the lattice of `ModY` goes through.
    pub(crate) offset_y: i32,
    /// `NoOverlappingOutput`: a match may not overlap the rule's earlier output.
    pub(crate) no_overlapping_output: bool,
    /// `Disabled`.
    pub(crate) disabled: bool,
    /// `IgnoreLock`: write to locked layers too.
    pub(crate) ignore_lock: bool,
}

impl Default for RuleOptions {
    fn default() -> RuleOptions {
        RuleOptions {
            probability: 1.0,
            mod_x: 1,
            mod_y: 1,
            offset_x: 0,
            offset_y: 0,
            no_overlapping_output: false,
            disabled: false,
            ignore_lock: false,
        }
    }
}

impl RuleOptions {
    /// These options with those `values` sets replacing them.
    pub(crate) fn merged(self, values: &RuleOptionValues) -> RuleOptions {
        RuleOptions {
            probability: values.probability.unwrap_or(self.probability),
            mod_x: values.mod_x.unwrap_or(self.mod_x),
            mod_y: values.mod_y.unwrap_or(self.mod_y),
            offset_x: values.offset_x.unwrap_or(self.offset_x),
            offset_y: values.offset_y.unwrap_or(self.offset_y),
            no_overlapping_output: values
                .no_overlapping_output
                .unwrap_or(self.no_overlapping_output),
            disabled: values.disabled.unwrap_or(self.disabled),
            ignore_lock: values.ignore_lock.unwrap_or(self.ignore_lock),
        }
    }
}

/// The rule options a set of properties sets; `None` for those it leaves alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RuleOptionValues {
    pub(crate) probability: Option<f64>,
    pub(crate) mod_x: Option<i32>,
    pub(crate) mod_y: Option<i32>,
    pub(crate) offset_x: Option<i32>,
    pub(crate) offset_y: Option<i32>,
    pub(crate) no_overlapping_output: Option<bool>,
    pub(crate) disabled: Option<bool>,
    pub(crate) ignore_lock: Option<bool>,
}

/// What an input layer's properties set.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct InputLayerOptions {
    /// `AutoEmpty` (alias `StrictEmpty`): a cell left empty is a condition that the target is
    /// empty there.
    pub(crate) auto_empty: bool,
    /// The flips that must agree for a tile to match; `IgnoreHorizontalFlip` and the others
    /// clear theirs.
    pub(crate) mask: Flip,
}

/// What an output layer's properties set.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputLayerOptions {
    /// `Probability`, the weight of the layer's output index, when the layer sets it.
    pub(crate) probability: Option<f64>,
    /// The other properties, merged into the target layer whenever the layer is written.
    pub(crate) properties: Properties,
}

/// Reads options, recording what it cannot read.
pub(crate) struct OptionReader<'p> {
    /// What the properties belong to, for messages: "the rule map", "layer “input_walls”".
    owner: String,
    problems: &'p mut Vec<Problem>,
}

impl<'p> OptionReader<'p> {
    /// A reader of the properties of `owner`.
    pub(crate) fn new(
        owner: impl Into<String>,
        problems: &'p mut Vec<Problem>,
    ) -> OptionReader<'p> {
        OptionReader {
            owner: owner.into(),
            problems,
        }
    }

    /// Records that `name` means nothing here, as Tiled warns.
    fn record_unknown(&mut self, name: &str, value: &Value) {
        let text = format!(
            "{} has the property “{name}” = {}, which AutoMap ignores",
            self.owner,
            shown(value)
        );
        self.problems
            .push(Problem::new(ProblemKind::UnknownProperty, text));
    }

    fn record_unreadable(&mut self, name: &str, value: &Value, needed: &str) {
        let text = format!(
            "{}: the option “{name}” = {} is not {needed}; it keeps its default",
            self.owner,
            shown(value)
        );
        self.problems
            .push(Problem::new(ProblemKind::UnreadableOption, text));
    }

    fn record_odd(&mut self, name: &str, value: &Value, read_as: &str) {
        let text = format!(
            "{}: the option “{name}” = {} is read as {read_as}, as Tiled reads it",
            self.owner,
            shown(value)
        );
        self.problems
            .push(Problem::new(ProblemKind::OddOptionValue, text));
    }

    /// A yes or no, as Qt converts values to `bool`.
    pub(crate) fn read_bool(&mut self, name: &str, value: &Value) -> Option<bool> {
        match value {
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Float(f) => Some(*f != 0.0),
            Value::String(text) => {
                let lower = text.to_lowercase();
                match lower.as_str() {
                    "" | "0" | "false" => Some(false),
                    "1" | "true" => Some(true),
                    _ => {
                        // Qt reads any other text as true.
                        self.record_odd(name, value, "true");
                        Some(true)
                    }
                }
            }
            _ => {
                self.record_unreadable(name, value, "a yes or no");
                None
            }
        }
    }

    /// A whole number, as Qt converts values to `int`: reals rounded, text holding a whole
    /// number.
    pub(crate) fn read_int(&mut self, name: &str, value: &Value) -> Option<i32> {
        let read = match value {
            Value::Int(i) => i32::try_from(*i).ok(),
            Value::Bool(b) => Some(i32::from(*b)),
            Value::Float(f) => {
                let rounded = f.round();
                (rounded.is_finite()
                    && rounded >= f64::from(i32::MIN)
                    && rounded <= f64::from(i32::MAX))
                // The value was checked to fit.
                .then_some(rounded as i32)
            }
            Value::String(text) => text.trim().parse::<i32>().ok(),
            _ => None,
        };
        if read.is_none() {
            self.record_unreadable(name, value, "a whole number");
        }
        read
    }

    /// A real number, as Qt converts values to `double`.
    pub(crate) fn read_real(&mut self, name: &str, value: &Value) -> Option<f64> {
        let read = match value {
            // Integers in properties are far below 2^53.
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::String(text) => text.trim().parse::<f64>().ok(),
            _ => None,
        };
        let read = read.filter(|f| f.is_finite());
        if read.is_none() {
            self.record_unreadable(name, value, "a number");
        }
        read
    }

    /// A modulus (`ModX`, `ModY`): 0 is read as 1, as Tiled does to avoid dividing by zero; a
    /// negative one cannot be read.
    fn read_modulus(&mut self, name: &str, value: &Value) -> Option<i32> {
        let modulus = self.read_int(name, value)?;
        match modulus {
            0 => {
                self.record_odd(name, value, "1");
                Some(1)
            }
            m if m < 0 => {
                self.record_unreadable(name, value, "a positive whole number");
                None
            }
            m => Some(m),
        }
    }

    /// Reads one rule option into `values`; `false` when `name` is not a rule option.
    pub(crate) fn read_rule_option(
        &mut self,
        name: &str,
        value: &Value,
        values: &mut RuleOptionValues,
    ) -> bool {
        let is = |option: &str| name.eq_ignore_ascii_case(option);
        if is("Probability") {
            if let Some(p) = self.read_real(name, value) {
                values.probability = Some(p);
            }
        } else if is("ModX") {
            if let Some(m) = self.read_modulus(name, value) {
                values.mod_x = Some(m);
            }
        } else if is("ModY") {
            if let Some(m) = self.read_modulus(name, value) {
                values.mod_y = Some(m);
            }
        } else if is("OffsetX") {
            if let Some(o) = self.read_int(name, value) {
                values.offset_x = Some(o);
            }
        } else if is("OffsetY") {
            if let Some(o) = self.read_int(name, value) {
                values.offset_y = Some(o);
            }
        } else if is("NoOverlappingOutput") {
            if let Some(b) = self.read_bool(name, value) {
                values.no_overlapping_output = Some(b);
            }
        } else if is("Disabled") {
            if let Some(b) = self.read_bool(name, value) {
                values.disabled = Some(b);
            }
        } else if is("IgnoreLock") {
            if let Some(b) = self.read_bool(name, value) {
                values.ignore_lock = Some(b);
            }
        } else {
            return false;
        }
        true
    }

    /// The options of a rule map's own properties.
    pub(crate) fn read_map_options(&mut self, properties: &Properties) -> MapOptions {
        let mut options = MapOptions::default();
        let mut rule_values = RuleOptionValues::default();
        let mut no_overlapping_rules = None;
        for (name, property) in properties {
            let value = &property.value;
            let is = |option: &str| name.eq_ignore_ascii_case(option);
            if is("DeleteTiles") {
                if let Some(b) = self.read_bool(name, value) {
                    options.delete_tiles = b;
                }
            } else if is("MatchOutsideMap") {
                if let Some(b) = self.read_bool(name, value) {
                    options.match_outside_map = b;
                }
            } else if is("OverflowBorder") {
                if let Some(b) = self.read_bool(name, value) {
                    options.overflow_border = b;
                }
            } else if is("WrapBorder") {
                if let Some(b) = self.read_bool(name, value) {
                    options.wrap_border = b;
                }
            } else if is("AutomappingRadius") {
                if let Some(r) = self.read_int(name, value) {
                    options.radius = r;
                }
            } else if is("NoOverlappingRules") {
                if let Some(b) = self.read_bool(name, value) {
                    no_overlapping_rules = Some(b);
                }
            } else if is("MatchInOrder") {
                if let Some(b) = self.read_bool(name, value) {
                    options.match_in_order = b;
                    options.match_in_order_set = true;
                }
            } else if !self.read_rule_option(name, value, &mut rule_values) {
                self.record_unknown(name, value);
            }
        }
        // Each border option implies MatchOutsideMap.
        if options.overflow_border || options.wrap_border {
            options.match_outside_map = true;
        }
        // NoOverlappingRules is the old name of NoOverlappingOutput, used when the new one is
        // not set.
        if rule_values.no_overlapping_output.is_none() {
            rule_values.no_overlapping_output = Some(no_overlapping_rules.unwrap_or(false));
        }
        options.rule_defaults = RuleOptions::default().merged(&rule_values);
        options
    }

    /// The rule options a `rule_options` rectangle's properties set.
    pub(crate) fn read_area_options(&mut self, properties: &Properties) -> RuleOptionValues {
        let mut values = RuleOptionValues::default();
        for (name, property) in properties {
            if !self.read_rule_option(name, &property.value, &mut values) {
                self.record_unknown(name, &property.value);
            }
        }
        values
    }

    /// The options of an `input` or `inputnot` layer.
    pub(crate) fn read_input_layer_options(
        &mut self,
        properties: &Properties,
    ) -> InputLayerOptions {
        let mut options = InputLayerOptions {
            auto_empty: false,
            mask: Flip::all(),
        };
        for (name, property) in properties {
            let value = &property.value;
            let is = |option: &str| name.eq_ignore_ascii_case(option);
            let flip = if is("StrictEmpty") || is("AutoEmpty") {
                if let Some(b) = self.read_bool(name, value) {
                    options.auto_empty = b;
                }
                continue;
            } else if is("IgnoreHorizontalFlip") {
                Flip::H
            } else if is("IgnoreVerticalFlip") {
                Flip::V
            } else if is("IgnoreDiagonalFlip") {
                Flip::D
            } else if is("IgnoreHexRotate120") {
                Flip::HEX120
            } else {
                self.record_unknown(name, value);
                continue;
            };
            if self.read_bool(name, value) == Some(true) {
                options.mask.remove(flip);
            }
        }
        options
    }

    /// The options of an `output` layer: its index's `Probability` and the properties merged
    /// into the target layer.
    pub(crate) fn read_output_layer_options(
        &mut self,
        properties: &Properties,
    ) -> OutputLayerOptions {
        let mut options = OutputLayerOptions {
            probability: None,
            properties: Properties::new(),
        };
        for (name, property) in properties {
            if name.eq_ignore_ascii_case("Probability") {
                if let Some(p) = self.read_real(name, &property.value) {
                    options.probability = Some(p);
                }
                continue;
            }
            options.properties.insert(name.clone(), property.clone());
        }
        options
    }
}

/// A value as it reads in a message.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) | Value::File(text) => format!("“{text}”"),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Color(Some(color)) => format!("{color:?}"),
        Value::Color(None) => "no colour".to_owned(),
        Value::Object(id) => format!("object {id}"),
        Value::Class(_) => "a class value".to_owned(),
        Value::List(_) => "a list".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Property;

    use super::*;

    fn properties(entries: &[(&str, Value)]) -> Properties {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), Property::new(value.clone())))
            .collect()
    }

    #[test]
    fn map_options_are_read_without_regard_to_case() {
        let mut problems = Vec::new();
        let options =
            OptionReader::new("the rule map", &mut problems).read_map_options(&properties(&[
                ("deletetiles", Value::Bool(true)),
                ("WrapBorder", Value::String("true".into())),
                ("modX", Value::Int(2)),
                ("Probability", Value::Float(0.25)),
                ("AutomappingRadius", Value::Float(1.6)),
            ]));
        assert!(problems.is_empty(), "{problems:?}");
        assert!(options.delete_tiles && options.wrap_border && options.match_outside_map);
        assert_eq!(options.radius, 2, "reals are rounded, as Qt converts them");
        assert_eq!(options.rule_defaults.mod_x, 2);
        assert_eq!(options.rule_defaults.probability, 0.25);
        assert!(!options.match_in_order_set);
    }

    #[test]
    fn values_that_cannot_be_read_are_warnings_not_zeros() {
        let mut problems = Vec::new();
        let options =
            OptionReader::new("the rule map", &mut problems).read_map_options(&properties(&[
                ("ModX", Value::String("two".into())),
                ("OffsetY", Value::Color(None)),
                ("Probability", Value::String("often".into())),
                ("MatchInOrder", Value::String("yes".into())),
                ("ModY", Value::Int(0)),
                ("Colour", Value::Int(3)),
            ]));
        let kinds: Vec<ProblemKind> = problems.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [
                ProblemKind::UnknownProperty,
                ProblemKind::OddOptionValue,
                ProblemKind::UnreadableOption,
                ProblemKind::OddOptionValue,
                ProblemKind::UnreadableOption,
                ProblemKind::UnreadableOption,
            ],
            "{problems:#?}"
        );
        assert!(problems[2].text.contains("“ModX” = “two”"));
        assert_eq!(
            options.rule_defaults,
            RuleOptions {
                mod_y: 1,
                ..RuleOptions::default()
            }
        );
        assert!(options.match_in_order && options.match_in_order_set);
    }

    #[test]
    fn no_overlapping_rules_is_the_old_name_of_no_overlapping_output() {
        let mut problems = Vec::new();
        let mut reader = OptionReader::new("the rule map", &mut problems);
        let old =
            reader.read_map_options(&properties(&[("NoOverlappingRules", Value::Bool(true))]));
        assert!(old.rule_defaults.no_overlapping_output);
        let both = reader.read_map_options(&properties(&[
            ("NoOverlappingOutput", Value::Bool(false)),
            ("NoOverlappingRules", Value::Bool(true)),
        ]));
        assert!(!both.rule_defaults.no_overlapping_output);
    }

    #[test]
    fn rule_options_merge_over_the_defaults() {
        let mut problems = Vec::new();
        let values =
            OptionReader::new("a rectangle", &mut problems).read_area_options(&properties(&[
                ("OffsetX", Value::Int(1)),
                ("Disabled", Value::Bool(true)),
            ]));
        let defaults = RuleOptions {
            mod_x: 2,
            ..RuleOptions::default()
        };
        let merged = defaults.merged(&values);
        assert_eq!(
            (merged.mod_x, merged.offset_x, merged.disabled),
            (2, 1, true)
        );
    }

    #[test]
    fn input_layers_clear_the_flips_they_ignore() {
        let mut problems = Vec::new();
        let options = OptionReader::new("layer “input_set”", &mut problems)
            .read_input_layer_options(&properties(&[
                ("IgnoreHorizontalFlip", Value::Bool(true)),
                ("IgnoreVerticalFlip", Value::Bool(false)),
                ("AutoEmpty", Value::Bool(true)),
            ]));
        assert!(problems.is_empty());
        assert!(options.auto_empty);
        assert_eq!(options.mask, Flip::V | Flip::D | Flip::HEX120);
    }

    #[test]
    fn output_layers_keep_their_other_properties_for_the_target() {
        let mut problems = Vec::new();
        let options = OptionReader::new("layer “output_walls”", &mut problems)
            .read_output_layer_options(&properties(&[
                ("probability", Value::Float(3.0)),
                ("collides", Value::Bool(true)),
            ]));
        assert_eq!(options.probability, Some(3.0));
        assert_eq!(options.properties.len(), 1);
        assert!(options.properties.contains_key("collides"));
    }
}
