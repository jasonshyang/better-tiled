//! A rule map run inside its window, for a region run (§7). A rule is tried only where a full run
//! tries it, where its decision reads only cells of the window (so it decides as the full run
//! does there) and where its output reaches the window (else nothing it writes is read or kept),
//! in the full run's order; `DeleteTiles` erases within the window, and the end of the run keeps
//! only what falls in the area the plan recomputes. And the questions callers ask in advance:
//! whether region runs can be exact, how far an edit reaches, and what a region run may change.

use bt_core::Map;
use bt_utils::IRect;

use crate::bx::Bx;
use crate::region::{RuleReach, plan_region};
use crate::rules::RuleSet;

/// A rule map's window in a region run.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleMapWindow {
    /// The cells, as they are before the rule map runs, that hold what a full run has there.
    pub(crate) cells: Bx,
    /// The region a full run tries the rules around, grown by `AutomappingRadius`; `None` on an
    /// infinite map, where a rule a region run allows is tried wherever it can match.
    pub(crate) full_region: Option<Bx>,
}

impl RuleMapWindow {
    /// The positions to try a rule at: those a full run tries (its `input_box` overlapping the
    /// full region, and within `fit` on a finite map unless it may match outside), whose
    /// decision reads only the window and whose output reaches it. `None` when there are none.
    pub(crate) fn rule_area(
        &self,
        input_box: Bx,
        reach: RuleReach,
        fit: Option<IRect>,
    ) -> Option<Bx> {
        let effect = reach.effect?;
        let mut area = self.cells.sum(effect.negated());
        if let Some(decision) = reach.decision {
            area = area.intersection(self.cells.eroded(decision)?)?;
        }
        if let Some(full) = self.full_region {
            area = area.intersection(full.sum(input_box.negated()))?;
        }
        if let Some(fit) = fit {
            area = area.intersection(Bx::from_rect(fit)?)?;
        }
        Some(area)
    }
}

impl RuleSet {
    /// Whether a region run ([`RuleSet::apply`] with a rectangle, [`Area::Edit`]) can recompute
    /// exactly what a full run gives around an edit: no rule map is non-local (a rule matching
    /// in order that can feed its own later matches, or `NoOverlappingOutput` on a rule whose
    /// matches can overlap) or wraps around the map (`WrapBorder`). Infinite maps ask more of
    /// the rules: [`RuleSet::affected_area`] answers for a given map.
    ///
    /// [`Area::Edit`]: crate::Area::Edit
    pub fn supports_incremental(&self) -> bool {
        self.rule_maps()
            .iter()
            .zip(&self.locality.rule_maps)
            .all(|(rule_map, found)| {
                !rule_map.is_used() || (found.nonlocal.is_none() && !rule_map.options.wrap_border)
            })
    }

    /// How far an edit reaches: the offsets from an edited cell to the cells whose result a
    /// full run can change, as a rectangle around (0, 0). An edit of the rectangle `r` can
    /// change the result only inside `r` grown by these offsets.
    pub fn margin(&self) -> IRect {
        self.locality.influence().rect()
    }

    /// The cells a region run around `region` may change on `map` (the rectangle grown by
    /// [`RuleSet::margin`], within a finite map; empty when that leaves the map), for redrawing
    /// and undo snapshots; `None` when such a run falls back to a full run.
    pub fn affected_area(&self, map: &Map, region: IRect) -> Option<IRect> {
        let plan = plan_region(&self.locality, self.rule_maps(), map, region).ok()?;
        Some(plan.exact.map(Bx::rect).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use glam::IVec2;

    use super::*;

    fn bx(x0: i32, y0: i32, x1: i32, y1: i32) -> Bx {
        Bx {
            min: IVec2::new(x0, y0),
            max: IVec2::new(x1, y1),
        }
    }

    #[test]
    fn rules_are_tried_where_they_read_the_window_and_reach_it() {
        let window = RuleMapWindow {
            cells: bx(10, 10, 19, 19),
            full_region: Some(bx(0, 0, 99, 99)),
        };
        // Reads two cells right, writes the cell between.
        let reach = RuleReach {
            decision: Some(bx(0, 0, 2, 0)),
            effect: Some(bx(1, 0, 1, 0)),
        };
        assert_eq!(
            window.rule_area(bx(0, 0, 2, 0), reach, None),
            Some(bx(10, 10, 17, 19))
        );
        // A rule that changes nothing is never tried; one that reads nothing is tried wherever
        // its output reaches the window.
        let silent = RuleReach {
            decision: Some(bx(0, 0, 0, 0)),
            effect: None,
        };
        assert_eq!(window.rule_area(bx(0, 0, 0, 0), silent, None), None);
        let blind = RuleReach {
            decision: None,
            effect: Some(bx(0, 0, 0, 0)),
        };
        assert_eq!(
            window.rule_area(bx(0, 0, 0, 0), blind, None),
            Some(bx(10, 10, 19, 19))
        );
        // A window at the edge of a finite map tries only positions that fit.
        let edge = RuleMapWindow {
            cells: bx(-3, 0, 4, 4),
            full_region: Some(bx(0, 0, 9, 9)),
        };
        let fit = IRect {
            x: 0,
            y: 0,
            w: 8,
            h: 10,
        };
        assert_eq!(
            edge.rule_area(bx(0, 0, 2, 0), reach, Some(fit)),
            Some(bx(0, 0, 2, 4))
        );
    }
}
