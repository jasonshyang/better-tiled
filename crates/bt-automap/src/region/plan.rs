//! The plan of a region run (§7): the cells it recomputes and writes back, the edit grown by the
//! rule set's influence (within a finite map), and for each rule map the window in which the
//! cells as they were before it must be what a full run has there. Windows are computed backward
//! through the chain: the last rule map's window is the area written back grown by its
//! dependency box, each earlier one the next window grown by its own, so every cell a later rule
//! map reads is one the earlier ones recomputed. Windows are not clipped to a finite map: cells
//! outside it read as empty, or as the nearest cell inside, which a window reaching past the
//! edge holds.
//!
//! When the rules cannot be run exactly around an edit, the plan is the reason, and the run is a
//! full run that says so.

use bt_core::Map;
use bt_utils::IRect;

use crate::bx::Bx;
use crate::problem::{Problem, ProblemKind};
use crate::region::locality::{Locality, layers_have_map_size};
use crate::rulemap::RuleMap;

/// What a region run recomputes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegionPlan {
    /// The cells the run recomputes and writes back; `None` when the edit misses a finite map.
    pub(crate) exact: Option<Bx>,
    /// For each rule map, in run order, the cells as they were before it that must be what a
    /// full run has there; `None` with `exact`.
    pub(crate) windows: Vec<Option<Bx>>,
}

impl RegionPlan {
    /// The window of rule map `index`.
    pub(crate) fn window(&self, index: usize) -> Option<Bx> {
        self.windows.get(index).copied().flatten()
    }

    /// The cells the run reads densely: the first rule map's window, within a finite map.
    pub(crate) fn interest(&self, map_rect: Option<IRect>) -> IRect {
        let Some(window) = self.windows.first().copied().flatten() else {
            return IRect::default();
        };
        match map_rect {
            Some(map_rect) => window.rect().intersection(map_rect),
            None => window.rect(),
        }
    }
}

/// The rule map as a fallback's reason names it: its number and file name.
fn named(index: usize, rule_map: &RuleMap) -> String {
    let file = rule_map.path().file_name().map_or_else(
        || rule_map.path().display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("rule map {} ({file})", index + 1)
}

fn fallback(reason: String) -> Problem {
    Problem::new(
        ProblemKind::FullRunFallback,
        format!("the rules ran on the whole map: {reason}"),
    )
}

/// Why a region run on `map` cannot be exact, if it cannot.
fn fallback_reason(locality: &Locality, rule_maps: &[RuleMap], map: &Map) -> Option<Problem> {
    let mut active_before = false;
    for (index, (rule_map, found)) in rule_maps.iter().zip(&locality.rule_maps).enumerate() {
        if !rule_map.is_used() {
            continue;
        }
        let options = &rule_map.options;
        let name = || named(index, rule_map);
        if let Some(reason) = &found.nonlocal {
            return Some(fallback(format!("{}: {reason}", name())));
        }
        if !map.infinite {
            if options.wrap_border {
                return Some(fallback(format!(
                    "{} has WrapBorder, so what it reads and writes wraps around the map",
                    name()
                )));
            }
            if options.overflow_border && !layers_have_map_size(map) {
                return Some(fallback(format!(
                    "{} has OverflowBorder and a tile layer of the map is not the map's size, so \
                     cells outside it read as cells far from the edit",
                    name()
                )));
            }
            continue;
        }
        if let Some(rule) = &found.matches_on_empty {
            return Some(fallback(format!(
                "on an infinite map, {rule} of {} can match where every cell it reads is \
                 empty, so where it is tried depends on the region the run grows",
                name()
            )));
        }
        if options.match_in_order {
            return Some(fallback(format!(
                "on an infinite map, {} matches in order (MatchInOrder), so where and in which \
                 order its rules are tried depends on the region the run grows",
                name()
            )));
        }
        if options.radius < 0 {
            return Some(fallback(format!(
                "on an infinite map, {} has a negative AutomappingRadius, so where its rules are \
                 tried depends on the region the run grows",
                name()
            )));
        }
        if active_before && let Some(rule) = &found.order_sensitive {
            return Some(fallback(format!(
                "on an infinite map, two matches of {rule} of {} can write different tiles into \
                 one cell, and the order they are tried in depends on the region earlier rule \
                 maps grew",
                name()
            )));
        }
        if rule_map.rule_count() > 0 {
            active_before = true;
        }
    }
    None
}

/// The plan of a region run around `edit` on `map`, or why the run must be a full run.
pub(crate) fn plan_region(
    locality: &Locality,
    rule_maps: &[RuleMap],
    map: &Map,
    edit: IRect,
) -> Result<RegionPlan, Problem> {
    if let Some(problem) = fallback_reason(locality, rule_maps, map) {
        return Err(problem);
    }
    let exact = Bx::from_rect(edit).and_then(|edit| {
        let grown = edit.sum(locality.influence());
        if map.infinite {
            return Some(grown);
        }
        let map_rect = IRect {
            x: 0,
            y: 0,
            w: i32::try_from(map.width).unwrap_or(i32::MAX),
            h: i32::try_from(map.height).unwrap_or(i32::MAX),
        };
        Bx::from_rect(map_rect).and_then(|map_box| grown.intersection(map_box))
    });
    let mut windows = vec![None; locality.rule_maps.len()];
    let mut next = exact;
    for (window, found) in windows.iter_mut().zip(&locality.rule_maps).rev() {
        next = next.map(|w| w.sum(found.dependency));
        *window = next;
    }
    Ok(RegionPlan { exact, windows })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Property, Value};
    use bt_test_data::fixtures::{Atlas, MapBuilder, rule_tiles};
    use glam::{IVec2, UVec2};

    use super::*;
    use crate::rules::RuleSet;

    fn rule_map(input: &str, output: &str, options: &[(&str, Value)]) -> Arc<Map> {
        let atlas = Atlas::builder().build_embedded().unwrap().tileset;
        let width = input.split_whitespace().count() as u32;
        let mut map = MapBuilder::new(UVec2::splat(16), UVec2::new(width, 1))
            .tileset(atlas)
            .tileset(rule_tiles().unwrap())
            .tile_layer("input_set", input)
            .tile_layer("output_set", output)
            .build()
            .unwrap();
        for (name, value) in options {
            map.properties
                .insert((*name).to_owned(), Property::new(value.clone()));
        }
        Arc::new(map)
    }

    fn target(size: u32, infinite: bool) -> Map {
        let mut builder = MapBuilder::new(UVec2::splat(16), UVec2::new(size, size));
        if infinite {
            builder = builder.infinite();
        }
        builder.build().unwrap()
    }

    fn bx(x0: i32, y0: i32, x1: i32, y1: i32) -> Bx {
        Bx {
            min: IVec2::new(x0, y0),
            max: IVec2::new(x1, y1),
        }
    }

    #[test]
    fn windows_grow_backward_through_the_chain() {
        // Each rule map reads one cell either side of the cell it writes.
        let set = RuleSet::builder()
            .rule_map("/r/a.tmx", rule_map("1 . 1", ". 2 .", &[]))
            .rule_map("/r/b.tmx", rule_map("2 . 2", ". 3 .", &[]))
            .build();
        let map = target(20, false);
        let edit = IRect {
            x: 10,
            y: 10,
            w: 1,
            h: 1,
        };
        let plan = plan_region(&set.locality, set.rule_maps(), &map, edit).unwrap();
        // An edit reaches two cells either way through two rule maps.
        assert_eq!(plan.exact, Some(bx(8, 10, 12, 10)));
        assert_eq!(plan.window(1), Some(bx(7, 10, 13, 10)));
        assert_eq!(plan.window(0), Some(bx(6, 10, 14, 10)));
        // At the map's edge the area written back is clipped, the windows are not.
        let corner = plan_region(
            &set.locality,
            set.rule_maps(),
            &map,
            IRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
        )
        .unwrap();
        assert_eq!(corner.exact, Some(bx(0, 0, 2, 0)));
        assert_eq!(corner.window(0), Some(bx(-2, 0, 4, 0)));
        // An edit beside a finite map changes nothing.
        let outside = plan_region(
            &set.locality,
            set.rule_maps(),
            &map,
            IRect {
                x: 40,
                y: 0,
                w: 1,
                h: 1,
            },
        )
        .unwrap();
        assert_eq!(outside.exact, None);
    }

    #[test]
    fn region_runs_fall_back_with_their_reason() {
        let wrapping = RuleSet::builder()
            .rule_map(
                "/r/wrap.tmx",
                rule_map("1", "2", &[("WrapBorder", Value::Bool(true))]),
            )
            .build();
        let edit = IRect {
            x: 1,
            y: 1,
            w: 1,
            h: 1,
        };
        let problem = plan_region(
            &wrapping.locality,
            wrapping.rule_maps(),
            &target(4, false),
            edit,
        )
        .unwrap_err();
        assert_eq!(problem.kind, ProblemKind::FullRunFallback);
        assert!(problem.text.contains("rule map 1 (wrap.tmx)"), "{problem}");
        assert!(problem.text.contains("WrapBorder"));
        // WrapBorder means nothing on an infinite map.
        assert!(
            plan_region(
                &wrapping.locality,
                wrapping.rule_maps(),
                &target(4, true),
                edit
            )
            .is_ok()
        );
        // A rule matching empty cells is fine on a finite map, not on an infinite one.
        let empty = RuleSet::builder()
            .rule_map("/r/empty.tmx", rule_map("E", "2", &[]))
            .build();
        assert!(plan_region(&empty.locality, empty.rule_maps(), &target(4, false), edit).is_ok());
        let problem =
            plan_region(&empty.locality, empty.rule_maps(), &target(4, true), edit).unwrap_err();
        assert!(
            problem.text.contains("every cell it reads is empty"),
            "{problem}"
        );
    }
}
