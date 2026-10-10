//! What a condition accepts, and how a written tile bears on it. The locality analysis counts a
//! write as something a later condition depends on only when the write can change what the
//! condition says about its cell: writing B over A cannot make "is C" true or false. Conditions
//! are Tiled's compiled "any of" and "none of" lists, compared tile by tile under their flip
//! masks.
//!
//! Tiles are compared by their place in the rule set's tileset table. Two tilesets of the table
//! that a run may unify (similar ones, one of them embedded in a rule map) can be one tileset on
//! some map: their tiles are compared cautiously, every judgment involving them answering what
//! keeps a region run exact.

use bt_core::Flip;

use crate::rulemap::{MatchCell, TableTile, TilesetTable};

/// The cells a condition at one position accepts.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Accepted<'a> {
    /// Every cell: there is no condition there.
    Every,
    /// The cells one of these matches.
    AnyOf(&'a [MatchCell]),
    /// The cells none of these matches.
    NoneOf(&'a [MatchCell]),
}

/// Whether two table tilesets are one tileset on a map.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SameTileset {
    /// The same tileset of the table.
    Yes,
    /// Different tilesets a run may unify.
    Maybe,
    /// Tilesets no run unifies.
    No,
}

/// Compares conditions and written tiles, knowing which tilesets of the table a run may unify.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConditionLogic {
    /// For each table tileset, the first tileset of the group a run may unify it with (itself
    /// when there is none).
    group: Vec<usize>,
    /// For each table tileset, whether its group holds another.
    ambiguous: Vec<bool>,
}

impl ConditionLogic {
    /// The logic for the tilesets of `table`: two of them may be unified when they are similar
    /// (Tiled's criteria) and one is embedded in a rule map, as `TilesetBinding` unifies them.
    pub(crate) fn new(table: &TilesetTable) -> ConditionLogic {
        let entries = table.entries();
        let mut parent: Vec<usize> = (0..entries.len()).collect();
        for (i, a) in entries.iter().enumerate() {
            for (j, b) in entries.iter().enumerate().skip(i + 1) {
                let embedded = a.reference.file.is_none() || b.reference.file.is_none();
                if embedded && a.reference.tileset.is_similar_to(&b.reference.tileset) {
                    let (first, second) = (root(&parent, i), root(&parent, j));
                    if let Some(slot) = parent.get_mut(second.max(first)) {
                        *slot = second.min(first);
                    }
                }
            }
        }
        let group: Vec<usize> = (0..parent.len()).map(|i| root(&parent, i)).collect();
        ConditionLogic::from_groups(group)
    }

    /// The logic for tilesets in `group`s: the first tileset of each one's group.
    fn from_groups(group: Vec<usize>) -> ConditionLogic {
        let ambiguous = group
            .iter()
            .enumerate()
            .map(|(i, g)| group.iter().enumerate().any(|(j, h)| i != j && g == h))
            .collect();
        ConditionLogic { group, ambiguous }
    }

    fn same_tileset(&self, a: usize, b: usize) -> SameTileset {
        if a == b {
            SameTileset::Yes
        } else if self
            .group
            .get(a)
            .is_some_and(|g| self.group.get(b) == Some(g))
        {
            SameTileset::Maybe
        } else {
            SameTileset::No
        }
    }

    /// Whether a run may unify the tileset with another.
    fn is_ambiguous(&self, tileset: usize) -> bool {
        self.ambiguous.get(tileset).copied().unwrap_or(false)
    }

    fn mentions_ambiguous(&self, accepted: Accepted<'_>) -> bool {
        match accepted {
            Accepted::Every => false,
            Accepted::AnyOf(cells) | Accepted::NoneOf(cells) => cells
                .iter()
                .any(|m| m.tile.is_some_and(|t| self.is_ambiguous(t.tileset))),
        }
    }

    /// Whether some cell matches both, `true` when it may (tiles of tilesets a run may unify).
    fn overlap(&self, a: &MatchCell, b: &MatchCell) -> bool {
        match (a.tile, b.tile) {
            (None, None) => true,
            (Some(x), Some(y)) => {
                self.same_tileset(x.tileset, y.tileset) != SameTileset::No
                    && x.tile == y.tile
                    && ((x.flip ^ y.flip) & a.mask & b.mask).is_empty()
            }
            _ => false,
        }
    }

    /// Whether every cell `a` matches is matched by one of `by`; `false` when that holds only
    /// if a run unifies two tilesets.
    fn covered(&self, a: &MatchCell, by: &[MatchCell]) -> bool {
        let Some(x) = a.tile else {
            return by.iter().any(|c| c.tile.is_none());
        };
        (0..=Flip::all().bits())
            .map(Flip::from_bits_truncate)
            .filter(|&flip| (flip & a.mask) == (x.flip & a.mask))
            .all(|flip| {
                by.iter().any(|c| {
                    c.tile.is_some_and(|y| {
                        self.same_tileset(x.tileset, y.tileset) == SameTileset::Yes
                            && y.tile == x.tile
                            && (flip & c.mask) == (y.flip & c.mask)
                    })
                })
            })
    }

    /// Whether every cell `a` accepts, `b` accepts; `false` when unsure.
    pub(crate) fn within(&self, a: Accepted<'_>, b: Accepted<'_>) -> bool {
        match (a, b) {
            (_, Accepted::Every) => true,
            (Accepted::Every, _) | (Accepted::NoneOf(_), Accepted::AnyOf(_)) => false,
            (Accepted::AnyOf(x), Accepted::AnyOf(y)) => x.iter().all(|m| self.covered(m, y)),
            (Accepted::AnyOf(x), Accepted::NoneOf(y)) => {
                !x.iter().any(|m| y.iter().any(|n| self.overlap(m, n)))
            }
            // "None of x" within "none of y": every tile y names, x names too.
            (Accepted::NoneOf(x), Accepted::NoneOf(y)) => y.iter().all(|m| self.covered(m, x)),
        }
    }

    /// Whether some cell both accept; `true` when unsure.
    pub(crate) fn meets(&self, a: Accepted<'_>, b: Accepted<'_>) -> bool {
        match (a, b) {
            (Accepted::Every, _)
            | (_, Accepted::Every)
            | (Accepted::NoneOf(_), Accepted::NoneOf(_)) => true,
            (Accepted::AnyOf(x), Accepted::AnyOf(y)) => {
                x.iter().any(|m| y.iter().any(|n| self.overlap(m, n)))
            }
            (Accepted::AnyOf(x), Accepted::NoneOf(y))
            | (Accepted::NoneOf(y), Accepted::AnyOf(x)) => x.iter().any(|m| !self.covered(m, y)),
        }
    }

    /// Whether `accepted` takes the empty cell.
    pub(crate) fn accepts_empty(accepted: Accepted<'_>) -> bool {
        match accepted {
            Accepted::Every => true,
            Accepted::AnyOf(cells) => cells.iter().any(|m| m.tile.is_none()),
            Accepted::NoneOf(cells) => !cells.iter().any(|m| m.tile.is_none()),
        }
    }

    /// Whether `accepted` takes the cell `value` (a tile, or `None` for the empty cell); `None`
    /// when that depends on how a run unifies tilesets.
    fn accepts(&self, accepted: Accepted<'_>, value: Option<TableTile>) -> Option<bool> {
        if value.is_some_and(|v| self.is_ambiguous(v.tileset)) || self.mentions_ambiguous(accepted)
        {
            return None;
        }
        let matches = |m: &MatchCell| match (m.tile, value) {
            (None, None) => true,
            (Some(t), Some(v)) => {
                t.tileset == v.tileset && t.tile == v.tile && (t.flip & m.mask) == (v.flip & m.mask)
            }
            _ => false,
        };
        Some(match accepted {
            Accepted::Every => true,
            Accepted::AnyOf(cells) => cells.iter().any(matches),
            Accepted::NoneOf(cells) => !cells.iter().any(matches),
        })
    }

    /// Whether writing `value` over a cell that held one of the cells `before` accepts (the
    /// writer's own conditions there: it matched) can change whether `condition` holds there;
    /// `true` when unsure.
    pub(crate) fn can_change(
        &self,
        value: Option<TableTile>,
        before: &[Accepted<'_>],
        condition: Accepted<'_>,
    ) -> bool {
        if matches!(condition, Accepted::Every) {
            return false;
        }
        if before.is_empty() || before.iter().any(|b| matches!(b, Accepted::Every)) {
            return true;
        }
        if before.iter().any(|&b| self.mentions_ambiguous(b)) {
            return true;
        }
        match self.accepts(condition, value) {
            None => true,
            // The condition holds after the write: it changed unless it held before already.
            Some(true) => !before.iter().all(|&b| self.within(b, condition)),
            // It fails after: it changed if it could have held before.
            Some(false) => before.iter().any(|&b| self.meets(b, condition)),
        }
    }
}

/// The first tileset of the group `i` is in.
fn root(group: &[usize], mut i: usize) -> usize {
    while let Some(&parent) = group.get(i) {
        if parent == i {
            break;
        }
        i = parent;
    }
    i
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{TileId, TilesetRef};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn tile(tileset: usize, id: u32, flip: Flip, mask: Flip) -> MatchCell {
        MatchCell {
            tile: Some(TableTile {
                tileset,
                tile: TileId(id),
                flip,
            }),
            mask,
        }
    }

    fn plain(id: u32) -> MatchCell {
        tile(0, id, Flip::empty(), Flip::all())
    }

    fn written(id: u32) -> Option<TableTile> {
        Some(TableTile {
            tileset: 0,
            tile: TileId(id),
            flip: Flip::empty(),
        })
    }

    fn logic() -> ConditionLogic {
        ConditionLogic::from_groups(vec![0, 1])
    }

    #[test]
    fn subsets_and_meetings_follow_the_lists_and_their_flip_masks() {
        let logic = logic();
        let a = [plain(1)];
        let ab = [plain(1), plain(2)];
        let empty = [MatchCell::EMPTY];
        assert!(logic.within(Accepted::AnyOf(&a), Accepted::AnyOf(&ab)));
        assert!(!logic.within(Accepted::AnyOf(&ab), Accepted::AnyOf(&a)));
        assert!(logic.within(Accepted::AnyOf(&a), Accepted::NoneOf(&empty)));
        assert!(!logic.within(Accepted::NoneOf(&empty), Accepted::AnyOf(&a)));
        assert!(logic.within(Accepted::NoneOf(&ab), Accepted::NoneOf(&a)));
        assert!(logic.meets(Accepted::AnyOf(&ab), Accepted::NoneOf(&a)));
        assert!(!logic.meets(Accepted::AnyOf(&a), Accepted::NoneOf(&ab)));
        assert!(!logic.meets(Accepted::AnyOf(&a), Accepted::AnyOf(&empty)));
        // "Tile 1 in any flip" covers "tile 1 mirrored", not the other way round.
        let any_flip = [tile(0, 1, Flip::empty(), Flip::empty())];
        let mirrored = [tile(0, 1, Flip::H, Flip::all())];
        assert!(logic.within(Accepted::AnyOf(&mirrored), Accepted::AnyOf(&any_flip)));
        assert!(!logic.within(Accepted::AnyOf(&any_flip), Accepted::AnyOf(&mirrored)));
        assert!(logic.meets(Accepted::AnyOf(&any_flip), Accepted::AnyOf(&mirrored)));
        assert!(!logic.meets(Accepted::AnyOf(&a), Accepted::AnyOf(&mirrored)));
        assert!(ConditionLogic::accepts_empty(Accepted::NoneOf(&a)));
        assert!(!ConditionLogic::accepts_empty(Accepted::AnyOf(&a)));
    }

    #[test]
    fn a_write_changes_a_condition_only_when_its_answer_can_differ() {
        let logic = logic();
        let a = [plain(1)];
        let b = [plain(2)];
        let c = [plain(3)];
        // B written where A was: "is C" stays false, "is A" turns false, "is B" turns true.
        let before = [Accepted::AnyOf(&a)];
        assert!(!logic.can_change(written(2), &before, Accepted::AnyOf(&c)));
        assert!(logic.can_change(written(2), &before, Accepted::AnyOf(&a)));
        assert!(logic.can_change(written(2), &before, Accepted::AnyOf(&b)));
        // "Not empty" holds before and after a tile replaces a tile, not after an erase.
        let empty = [MatchCell::EMPTY];
        assert!(!logic.can_change(written(2), &before, Accepted::NoneOf(&empty)));
        assert!(logic.can_change(None, &before, Accepted::NoneOf(&empty)));
        // A writer without a condition there could have replaced anything.
        assert!(logic.can_change(written(2), &[Accepted::Every], Accepted::AnyOf(&c)));
    }

    #[test]
    fn similar_tilesets_one_of_them_embedded_are_compared_cautiously() {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let copy = Arc::new(bt_core::Tileset::clone(&atlas));
        let mut table = TilesetTable::new();
        table.insert_tileset(&TilesetRef::embedded(Arc::clone(&atlas)), 0);
        table.insert_tileset(&TilesetRef::embedded(copy), 1);
        let logic = ConditionLogic::new(&table);
        assert_eq!(logic.group, [0, 0]);
        assert_eq!(logic.ambiguous, [true, true]);
        let first = [plain(1)];
        let second = [tile(1, 1, Flip::empty(), Flip::all())];
        // The same tile on some maps: they may meet, neither is surely within the other.
        assert!(logic.meets(Accepted::AnyOf(&first), Accepted::AnyOf(&second)));
        assert!(!logic.within(Accepted::AnyOf(&first), Accepted::AnyOf(&second)));
        assert!(logic.can_change(
            written(1),
            &[Accepted::AnyOf(&first)],
            Accepted::AnyOf(&second)
        ));
    }
}
