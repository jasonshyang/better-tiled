//! How a rule is named: by its rule map's place in the rule set and its own place in the rule map.

use std::fmt;

/// A rule of a rule set: the rule map's index in the set (the order the rule maps run in) and the
/// rule's index in that rule map (rules are ordered by the top-left of their region, as Tiled
/// orders them).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleRef {
    /// The rule map's index in the rule set.
    pub rule_map: usize,
    /// The rule's index in its rule map.
    pub rule: usize,
}

impl fmt::Display for RuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rule {} of rule map {}",
            self.rule + 1,
            self.rule_map + 1
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_named_from_one() {
        let rule = RuleRef {
            rule_map: 0,
            rule: 4,
        };
        assert_eq!(rule.to_string(), "rule 5 of rule map 1");
    }
}
