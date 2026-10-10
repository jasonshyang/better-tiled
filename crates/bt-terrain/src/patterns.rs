//! The patterns a terrain set needs (`docs/design/07-terrain.md`, §11): the combinations of its
//! terrains on the lanes of its kind that its tiles should cover, with how many tiles have each,
//! for the tileset editor's Patterns view (Tiled's Patterns tab, `wangtemplatemodel.cpp`).
//!
//! Tiled lists every combination of the set's terrains, leaving out "nothing". That says nothing
//! useful about a terrain drawn over nothing ("1 of 1"), misses the patterns with nothing in
//! them, lists N⁸ patterns for N autotiles that never meet, 256 for a desert whose islands meet
//! only the sand, and counts a complete blob as 47 of 256. So the combinations are taken within
//! the groups of terrains the labels show together:
//!
//! - "nothing" is one of the terrains when a terrain is drawn over nothing;
//! - terrains that never meet are kept apart (each autotile on its own, each island with its
//!   ground);
//! - in a blob of two terrains a corner has the terrain only between two edges of it, so a blob
//!   needs 47 patterns, not 256, even with a stray label;
//! - a group with too many combinations is counted, not listed.

use std::collections::{BTreeSet, HashMap, HashSet};

use bt_core::{WangId, WangSetType};

use crate::lanes::kind_lanes;
use crate::terrain::Terrain;

/// The most patterns listed, as in Tiled.
pub const MAX_LISTED: usize = 0xffff;

/// Groups with more combinations than this are counted, not listed in full.
const ENUMERATED: u64 = 1 << 20;

/// The patterns a set needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patterns {
    /// The patterns, group by group in Tiled's order, each with how many tiles have it; at most
    /// [`MAX_LISTED`].
    pub listed: Vec<NeededPattern>,
    /// How many patterns the set needs in all: more than are listed when a group has too many
    /// combinations to list.
    pub total: u64,
}

/// A pattern a set needs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NeededPattern {
    /// The terrains on the lanes of the set's kind; the other lanes are 0.
    pub pattern: WangId,
    /// How many tiles have it, the flipped and turned variants the tileset allows counting too,
    /// as Tiled counts them.
    pub tiles: usize,
}

impl Patterns {
    /// How many of the listed patterns some tile has.
    pub fn present(&self) -> usize {
        self.listed.iter().filter(|p| p.tiles > 0).count()
    }

    /// The listed patterns no tile has.
    pub fn missing(&self) -> impl Iterator<Item = &NeededPattern> {
        self.listed.iter().filter(|p| p.tiles == 0)
    }
}

/// The patterns `terrain`'s set needs, with how many tiles have each.
pub fn needed(terrain: &Terrain) -> Patterns {
    let colors = u8::try_from(terrain.color_count()).unwrap_or(u8::MAX);
    if colors == 0 {
        return Patterns::default();
    }
    let kind = terrain.kind();
    let lanes = kind_lanes(kind);
    let labels: Vec<[u8; 8]> = terrain
        .labels()
        .values()
        .map(|label| label.masked(terrain.kind_mask()).lanes())
        .collect();
    let mut have: HashMap<[u8; 8], usize> = HashMap::new();
    for candidate in terrain.candidates() {
        *have.entry(candidate.wang.lanes()).or_default() += 1;
    }
    let mut listed = Vec::new();
    let mut seen: HashSet<[u8; 8]> = HashSet::new();
    let mut total = 0_u64;
    for group in groups(&labels, lanes, colors) {
        let foreground = match group.as_slice() {
            [a, b] if kind == WangSetType::Mixed => foreground(&labels, *a, *b),
            _ => None,
        };
        let size = u32::try_from(lanes.len()).unwrap_or(u32::MAX);
        let count = (group.len() as u64).saturating_pow(size);
        let list = |pattern: [u8; 8], listed: &mut Vec<NeededPattern>| {
            if listed.len() < MAX_LISTED {
                listed.push(NeededPattern {
                    pattern: WangId::from_lanes(pattern),
                    tiles: have.get(&pattern).copied().unwrap_or(0),
                });
            }
        };
        if count > ENUMERATED {
            // Every combination: counted, and listed as far as the list goes.
            total = total.saturating_add(count);
            for n in 0..count.min(MAX_LISTED as u64) {
                let pattern = combination(lanes, &group, n);
                if seen.insert(pattern) {
                    list(pattern, &mut listed);
                }
            }
            continue;
        }
        for n in 0..count {
            let pattern = combination(lanes, &group, n);
            let nothing_at_all = pattern.iter().all(|&c| c == 0);
            let breaks_blob = foreground.is_some_and(|fg| !fits_blob(&pattern, fg));
            if nothing_at_all || breaks_blob || !seen.insert(pattern) {
                continue;
            }
            total += 1;
            list(pattern, &mut listed);
        }
    }
    Patterns { listed, total }
}

/// The `n`-th combination of the terrains of `group` on `lanes`, in Tiled's order: the first lane
/// counts fastest, through the group in its order.
fn combination(lanes: &[usize], group: &[u8], n: u64) -> [u8; 8] {
    let mut pattern = [0_u8; 8];
    let k = group.len().max(1) as u64;
    let mut n = n;
    for &lane in lanes {
        let index = usize::try_from(n % k).unwrap_or(0);
        if let (Some(slot), Some(&color)) = (pattern.get_mut(lane), group.get(index)) {
            *slot = color;
        }
        n /= k;
    }
    pattern
}

/// The terrains (0 for nothing) on a label's lanes, sorted, once each.
fn colors_of(label: &[u8; 8], lanes: &[usize]) -> Vec<u8> {
    let mut colors: Vec<u8> = lanes
        .iter()
        .filter_map(|&lane| label.get(lane).copied())
        .collect();
    colors.sort_unstable();
    colors.dedup();
    colors
}

/// The groups of terrains (0 for nothing) whose combinations a set of `n` terrains needs: each
/// largest group of terrains its labels show together, so that terrains that never meet are not
/// combined.
/// - A set of one terrain is drawn over nothing.
/// - Nothing is one of the terrains when labels show two or more terrains drawn over it; else a
///   label's empty lanes are just not labelled yet.
/// - A terrain no label shows with another goes with the one every group shares, when there are
///   several (the ground the islands are drawn on, or nothing).
/// - Else, and while no label shows two terrains together, every combination of them all, as
///   Tiled lists them.
fn groups(labels: &[[u8; 8]], lanes: &[usize], n: u8) -> Vec<Vec<u8>> {
    if n == 1 {
        return vec![vec![0, 1]];
    }
    let shown: Vec<Vec<u8>> = labels
        .iter()
        .map(|label| colors_of(label, lanes))
        .filter(|colors| colors.iter().all(|&c| c <= n))
        .collect();
    let over_nothing: BTreeSet<u8> = shown
        .iter()
        .filter(|colors| colors.first() == Some(&0))
        .flat_map(|colors| colors.iter().skip(1).copied())
        .collect();
    let nothing = over_nothing.len() >= 2;
    let mut together: Vec<Vec<u8>> = shown
        .into_iter()
        .map(|mut colors| {
            colors.retain(|&c| nothing || c != 0);
            colors
        })
        .filter(|colors| colors.len() > 1)
        .collect();
    together.sort_unstable();
    together.dedup();
    let mut groups: Vec<Vec<u8>> = together
        .iter()
        .filter(|g| {
            !together
                .iter()
                .any(|h| h.len() > g.len() && g.iter().all(|c| h.contains(c)))
        })
        .cloned()
        .collect();
    let every: Vec<u8> = (u8::from(!nothing)..=n).collect();
    if groups.is_empty() {
        return vec![every];
    }
    let lonely: Vec<u8> = (1..=n)
        .filter(|c| !groups.iter().any(|g| g.contains(c)))
        .collect();
    if !lonely.is_empty() {
        let ground = if groups.len() > 1 {
            (0..=n).find(|c| groups.iter().all(|g| g.contains(c)))
        } else {
            None
        };
        let Some(ground) = ground else {
            return vec![every];
        };
        groups.extend(lonely.into_iter().map(|c| {
            if ground < c {
                vec![ground, c]
            } else {
                vec![c, ground]
            }
        }));
        groups.sort_unstable();
    }
    groups
}

/// Which of two terrains of a mixed set (or nothing) is drawn over the other, as the labels with
/// just those two show: a corner has it only where both edges next to the corner have it too (a
/// blob's rule). A stray label or two does not hide it; a terrain over nothing is the one drawn
/// when the labels fit either way. `None` when the labels do not tell.
fn foreground(labels: &[[u8; 8]], a: u8, b: u8) -> Option<u8> {
    let pair = [a.min(b), a.max(b)];
    let both: Vec<&[u8; 8]> = labels
        .iter()
        .filter(|label| colors_of(label, &[0, 1, 2, 3, 4, 5, 6, 7]) == pair)
        .collect();
    let misfits = |fg: u8| both.iter().filter(|label| !fits_blob(label, fg)).count();
    let few = both.len() / 10;
    match (misfits(a), misfits(b)) {
        (0, 0) if a == 0 => Some(b),
        (0, 0) if b == 0 => Some(a),
        (0, 0) => None,
        (ma, mb) if ma <= few && ma < mb => Some(a),
        (ma, mb) if mb <= few && mb < ma => Some(b),
        _ => None,
    }
}

/// A blob's rule: a corner has `fg` only where both edges next to it have it.
fn fits_blob(label: &[u8; 8], fg: u8) -> bool {
    [1_usize, 3, 5, 7].iter().all(|&corner| {
        let at = |lane: usize| label.get(lane % 8).copied();
        at(corner) != Some(fg) || (at(corner - 1) == Some(fg) && at(corner + 1) == Some(fg))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_count_like_tileds_templates() {
        let corners = kind_lanes(WangSetType::Corner);
        // Corner sets: the top-right corner counts fastest, then bottom-right, bottom-left,
        // top-left.
        assert_eq!(combination(corners, &[1, 2], 0), [0, 1, 0, 1, 0, 1, 0, 1]);
        assert_eq!(combination(corners, &[1, 2], 1), [0, 2, 0, 1, 0, 1, 0, 1]);
        assert_eq!(combination(corners, &[1, 2], 2), [0, 1, 0, 2, 0, 1, 0, 1]);
        assert_eq!(combination(corners, &[1, 2], 15), [0, 2, 0, 2, 0, 2, 0, 2]);
        // Edge sets: top, right, bottom, left.
        let edges = kind_lanes(WangSetType::Edge);
        assert_eq!(combination(edges, &[1, 2, 3], 3), [1, 0, 2, 0, 1, 0, 1, 0]);
        let all = kind_lanes(WangSetType::Mixed);
        assert_eq!(combination(all, &[1, 2], 128), [1, 1, 1, 1, 1, 1, 1, 2]);
    }

    #[test]
    fn groups_follow_what_the_labels_show_together() {
        let corners = kind_lanes(WangSetType::Corner);
        let c = |tr, br, bl, tl| [0, tr, 0, br, 0, bl, 0, tl];
        assert_eq!(
            groups(&[], corners, 1),
            [vec![0, 1]],
            "one terrain over nothing"
        );
        assert_eq!(
            groups(&[], corners, 3),
            [vec![1, 2, 3]],
            "nothing shown yet"
        );
        // Brick and cobblestone each on sand, water not drawn yet: water goes with the sand.
        let islands = [c(1, 1, 1, 2), c(1, 3, 3, 1)];
        assert_eq!(
            groups(&islands, corners, 4),
            [vec![1, 2], vec![1, 3], vec![1, 4]]
        );
        // Two terrains over nothing, never together: nothing is a terrain of each group.
        let apart = [c(1, 0, 0, 0), c(0, 2, 2, 0), c(3, 3, 3, 3)];
        assert_eq!(
            groups(&apart, corners, 3),
            [vec![0, 1], vec![0, 2], vec![0, 3]]
        );
    }

    #[test]
    fn a_blob_s_foreground_shows_in_its_corners() {
        // Grass (1) over sand (2): an inner corner and an edge.
        let inner = [1, 2, 1, 1, 1, 1, 1, 1];
        let edge = [1, 1, 1, 2, 2, 2, 1, 1];
        assert_eq!(foreground(&[inner, edge], 1, 2), Some(1));
        assert_eq!(foreground(&[], 0, 1), Some(1), "over nothing, the terrain");
        assert!(fits_blob(&edge, 1) && !fits_blob(&[2, 1, 2, 2, 2, 2, 2, 2], 1));
    }
}
