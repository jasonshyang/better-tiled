//! Counting things by name, as reports list them: `grass ×96, water ×12, bridge`, the most
//! first, cut after a few with "and N more".

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A name and how many there are of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameCount {
    /// The name.
    pub name: String,
    /// How many.
    pub count: usize,
}

/// Names counted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    counts: BTreeMap<String, usize>,
}

impl Tally {
    /// Nothing counted yet.
    pub(crate) fn new() -> Tally {
        Tally::default()
    }

    /// Counts one more of `name`.
    pub(crate) fn add_one(&mut self, name: impl Into<String>) {
        self.add(name, 1);
    }

    /// Counts `count` more of `name`.
    pub(crate) fn add(&mut self, name: impl Into<String>, count: usize) {
        *self.counts.entry(name.into()).or_default() += count;
    }

    /// How many different names were counted.
    pub(crate) fn kinds(&self) -> usize {
        self.counts.len()
    }

    /// The counts, the most first, then by name.
    pub(crate) fn sorted(&self) -> Vec<NameCount> {
        let mut counts: Vec<NameCount> = self
            .counts
            .iter()
            .map(|(name, &count)| NameCount {
                name: name.clone(),
                count,
            })
            .collect();
        counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        counts
    }
}

/// `grass ×96, water ×12, bridge and 3 more`: at most `max` names, the most first; a name
/// counted once has no count.
pub(crate) fn counts_text(counts: &[NameCount], max: usize) -> String {
    let parts: Vec<String> = counts.iter().map(count_text).collect();
    crate::words::capped_list(&parts, max)
}

/// `grass ×96`, or `bridge` for one.
pub(crate) fn count_text(count: &NameCount) -> String {
    if count.count == 1 {
        count.name.clone()
    } else {
        format!("{} ×{}", count.name, count.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_counted_the_most_first() {
        let mut tally = Tally::new();
        for name in ["water", "grass", "grass", "bridge", "grass", "water"] {
            tally.add_one(name);
        }
        tally.add("sand", 2);
        assert_eq!(tally.kinds(), 4);
        assert_eq!(
            counts_text(&tally.sorted(), 3),
            "grass ×3, sand ×2, water ×2 and 1 more"
        );
        assert_eq!(
            counts_text(&tally.sorted(), 4),
            "grass ×3, sand ×2, water ×2, bridge"
        );
    }
}
