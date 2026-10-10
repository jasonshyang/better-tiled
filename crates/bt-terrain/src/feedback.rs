//! What the status bar and `bt edit` say about a paint or a fill, built from the typed results:
//! the patterns a set lacks for the cells no tile fits, the note for a fill whose terrain has no
//! plain tile, the terrain a stroke pushed back with the tile that would have let it stay, and the
//! cells of another terrain set a stroke painted over. Terrains are named by the caller, so the
//! editor uses its panels' names and `bt` its own.

use std::collections::{BTreeMap, HashMap};

use bt_core::{Map, TileLayer, WangSetType};
use bt_utils::text::{and_list, plural};
use glam::IVec2;

use crate::color::TerrainColor;
use crate::diagnose::{Cause, Needed};
use crate::request::Preview;
use crate::terrain::Terrain;
use crate::wangid::LANE_COUNT;

/// How a sentence names a terrain.
pub type TerrainName<'a> = &'a dyn Fn(TerrainColor) -> String;

/// A pattern of lanes: the terrain each lane needs, `None` where anything would do.
pub type Pattern = [Option<u8>; LANE_COUNT];

const LANE_NAMES: [&str; LANE_COUNT] = [
    "top",
    "top-right",
    "right",
    "bottom-right",
    "bottom",
    "bottom-left",
    "left",
    "top-left",
];

/// Why cells stayed unfilled, in one sentence: "no tile fits 3 cells: “Ground” has none with Sand
/// at the top-left corner and Grass at the other three corners (and 1 other pattern)", or the
/// plain reason when the commonest lack has one. `None` when nothing was left unfilled.
pub fn missing_text(terrain: &Terrain, name: TerrainName<'_>, needed: &[Needed]) -> Option<String> {
    let mut groups: HashMap<(Pattern, Cause), usize> = HashMap::new();
    for need in needed {
        *groups.entry((need.lanes, need.cause)).or_default() += 1;
    }
    let mut groups: Vec<((Pattern, Cause), usize)> = groups.into_iter().collect();
    // The commonest first; ties by pattern, so the sentence does not depend on hashing.
    groups.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.0.cmp(&b.0.0)));
    let ((pattern, cause), _) = groups.first()?;
    let cells = plural(needed.len(), "cell", "cells");
    let set = format!("“{}”", terrain.name());
    let reason = match cause {
        Cause::ZeroProbability => format!("the tile that fits best in {set} has probability 0"),
        _ => plain_reason(terrain, name, pattern)
            .map(|reason| format!("{set} {reason}"))
            .unwrap_or_else(|| {
                format!(
                    "{set} has none with {}",
                    describe(pattern, terrain.kind(), name)
                )
            }),
    };
    let others = groups.len() - 1;
    let more = if others > 0 {
        format!(
            " (and {})",
            plural(others, "other pattern", "other patterns")
        )
    } else {
        String::new()
    };
    Some(format!("no tile fits {cells}: {reason}{more}"))
}

/// The plain reason a pattern is missing, when there is one: no tile all of one terrain, no tile
/// where a terrain borders nothing, no tile joining two terrains.
fn plain_reason(terrain: &Terrain, name: TerrainName<'_>, pattern: &Pattern) -> Option<String> {
    let kind_lanes = kind_lanes(terrain.kind());
    let mut colors: Vec<u8> = pattern.iter().flatten().copied().collect();
    colors.sort_unstable();
    colors.dedup();
    let full = kind_lanes
        .iter()
        .zip(pattern)
        .all(|(&in_kind, lane)| !in_kind || lane.is_some());
    match colors.as_slice() {
        [c] if *c != 0 && full => Some(format!(
            "has no tile that is all {}",
            name(TerrainColor(*c))
        )),
        [0, c]
            if terrain
                .penalty(TerrainColor(*c), TerrainColor::NONE)
                .is_none() =>
        {
            Some(format!(
                "has no tile where {} borders nothing",
                name(TerrainColor(*c))
            ))
        }
        [a, b] if *a != 0 && terrain.penalty(TerrainColor(*a), TerrainColor(*b)) != Some(1) => {
            Some(format!(
                "has no tile joining {} and {}",
                name(TerrainColor(*a)),
                name(TerrainColor(*b))
            ))
        }
        _ => None,
    }
}

/// A fill with `color` shows transitions everywhere when the set has no tile all of it: the note
/// that says so, or `None`.
pub fn fill_note(terrain: &Terrain, name: TerrainName<'_>, color: TerrainColor) -> Option<String> {
    if color.is_none() {
        return None;
    }
    let lanes = kind_lanes(terrain.brush_kind(color));
    let plain: Pattern =
        std::array::from_fn(|l| lanes.get(l).copied().unwrap_or(false).then_some(color.0));
    if terrain.has_tile_with(&plain, true) {
        return None;
    }
    Some(format!(
        "“{}” has no tile that is all {}, so the fill places its transitions",
        terrain.name(),
        name(color)
    ))
}

/// A terrain a stroke pushed back: the cells where it lost lanes, and the tile that would have
/// let it stay at the commonest of them, when the set lacks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushedBack {
    /// The terrain that gave way.
    pub terrain: TerrainColor,
    /// The cells where it did, in row-major order.
    pub cells: Vec<IVec2>,
    /// The commonest tile that would have kept it (the painted terrain where the stroke put it,
    /// the old terrain elsewhere), when the set has no such tile.
    pub missing: Option<Pattern>,
}

/// Which other terrains a stroke of `color` pushed back where it painted: cells of the preview
/// where a terrain other than `color` gave lanes to a third terrain, which a tile joining it to
/// `color` would have spared. Commonest first.
pub fn pushed_back(
    terrain: &Terrain,
    before: &TileLayer,
    preview: &Preview,
    color: TerrainColor,
) -> Vec<PushedBack> {
    let mut by_terrain: BTreeMap<u8, Vec<IVec2>> = BTreeMap::new();
    let mut ideals: BTreeMap<u8, HashMap<Pattern, usize>> = BTreeMap::new();
    let kind_lanes = kind_lanes(terrain.kind());
    for placed in &preview.cells {
        let old = terrain.wang_of_cell(before.cell(placed.at)).lanes();
        let new = terrain.wang_of_cell(placed.cell).lanes();
        let lanes = || kind_lanes.iter().zip(old.iter().zip(&new));
        let mut lost: Vec<u8> = lanes()
            .filter(|&(&in_kind, (&was, &is))| {
                in_kind && was != 0 && was != color.0 && is != was && is != color.0
            })
            .map(|(_, (&was, _))| was)
            .collect();
        lost.sort_unstable();
        lost.dedup();
        for gone in lost {
            by_terrain.entry(gone).or_default().push(placed.at);
            // The painted terrain where the stroke put it, the old one elsewhere.
            let mut ideal: Pattern = [None; LANE_COUNT];
            for (slot, (&in_kind, (&was, &is))) in ideal.iter_mut().zip(lanes()) {
                *slot = in_kind.then_some(if is == color.0 { color.0 } else { was });
            }
            *ideals.entry(gone).or_default().entry(ideal).or_default() += 1;
        }
    }
    let mut pushed: Vec<PushedBack> = by_terrain
        .into_iter()
        .map(|(gone, mut cells)| {
            cells.sort_by_key(|p| (p.y, p.x));
            let missing = ideals.get(&gone).and_then(|ideals| {
                let mut ideals: Vec<(&Pattern, &usize)> = ideals.iter().collect();
                ideals.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                ideals
                    .into_iter()
                    .map(|(pattern, _)| *pattern)
                    .find(|pattern| !terrain.has_tile_with(pattern, false))
            });
            PushedBack {
                terrain: TerrainColor(gone),
                cells,
                missing,
            }
        })
        .collect();
    pushed.sort_by(|a, b| {
        b.cells
            .len()
            .cmp(&a.cells.len())
            .then(a.terrain.cmp(&b.terrain))
    });
    pushed
}

/// The sentence for what a stroke of `color` pushed back: "painting Grass pushed back Water at 4
/// cells: “Ground” has no tile with Grass at the top-left corner and Water at the other three
/// corners". `None` when it pushed nothing back.
pub fn pushed_back_text(
    terrain: &Terrain,
    name: TerrainName<'_>,
    color: TerrainColor,
    pushed: &[PushedBack],
) -> Option<String> {
    let first = pushed.first()?;
    let mut text = format!(
        "painting {} pushed back {} at {}",
        name(color),
        name(first.terrain),
        plural(first.cells.len(), "cell", "cells")
    );
    if let Some(pattern) = &first.missing {
        text.push_str(&format!(
            ": “{}” has no tile with {}",
            terrain.name(),
            describe(pattern, terrain.kind(), name)
        ));
    }
    Some(text)
}

/// Cells of another terrain set a stroke painted over (a set takes other sets' tiles for empty
/// cells).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacedSet {
    /// The other set's name.
    pub set_name: String,
    /// The cells, in row-major order.
    pub cells: Vec<IVec2>,
}

/// The cells the preview changes whose tiles belonged to another terrain set of the map's
/// tilesets, by set, most cells first.
pub fn replaced_sets(
    map: &Map,
    terrain: &Terrain,
    before: &TileLayer,
    preview: &Preview,
) -> Vec<ReplacedSet> {
    let mut by_set: BTreeMap<String, Vec<IVec2>> = BTreeMap::new();
    for placed in &preview.cells {
        let old = before.cell(placed.at);
        if old == placed.cell {
            continue;
        }
        let Some(slot) = old.slot() else { continue };
        let Some(tileset) = map.tileset(slot) else {
            continue;
        };
        let other = tileset.wang_sets.iter().enumerate().find(|(index, set)| {
            !(slot == terrain.slot() && *index == terrain.set_index())
                && set.tiles.contains_key(&old.tile())
        });
        if let Some((_, set)) = other {
            by_set.entry(set.name.clone()).or_default().push(placed.at);
        }
    }
    let mut replaced: Vec<ReplacedSet> = by_set
        .into_iter()
        .map(|(set_name, mut cells)| {
            cells.sort_by_key(|p| (p.y, p.x));
            ReplacedSet { set_name, cells }
        })
        .collect();
    replaced.sort_by_key(|r| std::cmp::Reverse(r.cells.len()));
    replaced
}

/// The hint for cells of other sets painted over: "painted over 3 cells of “Water”: paint each
/// terrain set on a layer of its own". `None` when none were.
pub fn replaced_text(replaced: &[ReplacedSet]) -> Option<String> {
    let cells: usize = replaced.iter().map(|r| r.cells.len()).sum();
    if cells == 0 {
        return None;
    }
    let names: Vec<String> = replaced
        .iter()
        .map(|r| format!("“{}”", r.set_name))
        .collect();
    Some(format!(
        "painted over {} of {}: paint each terrain set on a layer of its own",
        plural(cells, "cell", "cells"),
        and_list(&names)
    ))
}

/// The lanes a kind uses.
fn kind_lanes(kind: WangSetType) -> [bool; LANE_COUNT] {
    std::array::from_fn(|l| match kind {
        WangSetType::Corner => l % 2 == 1,
        WangSetType::Edge => l % 2 == 0,
        WangSetType::Mixed => true,
    })
}

/// A pattern in words: "Sand at the top-left corner and Grass at the other three corners".
fn describe(pattern: &Pattern, kind: WangSetType, name: TerrainName<'_>) -> String {
    let mut groups: Vec<(u8, Vec<usize>)> = Vec::new();
    for (lane, color) in pattern.iter().enumerate() {
        if let Some(c) = color {
            match groups.iter_mut().find(|(g, _)| g == c) {
                Some((_, lanes)) => lanes.push(lane),
                None => groups.push((*c, vec![lane])),
            }
        }
    }
    groups.sort_by_key(|(_, lanes)| (lanes.len(), lanes.first().copied()));
    let kind_lanes = kind_lanes(kind);
    let mut covered = [false; LANE_COUNT];
    let mut parts = Vec::new();
    let last = groups.len().saturating_sub(1);
    for (i, (color, lanes)) in groups.iter().enumerate() {
        let terrain = if *color == 0 {
            "nothing".to_owned()
        } else {
            name(TerrainColor(*color))
        };
        let corners: Vec<usize> = lanes.iter().copied().filter(|l| l % 2 == 1).collect();
        let edges: Vec<usize> = lanes.iter().copied().filter(|l| l % 2 == 0).collect();
        let mut places = Vec::new();
        for (subset, noun, nouns, every, at) in [
            (&corners, "corner", "corners", "at every corner", "at"),
            (&edges, "edge", "edges", "on every edge", "on"),
        ] {
            if subset.is_empty() {
                continue;
            }
            let of_kind = kind_lanes
                .iter()
                .enumerate()
                .filter(|&(l, &k)| k && (l % 2 == 1) == (noun == "corner"))
                .count();
            let before = covered
                .iter()
                .enumerate()
                .filter(|&(l, &c)| c && (l % 2 == 1) == (noun == "corner"))
                .count();
            if subset.len() == of_kind && before == 0 {
                places.push(every.to_owned());
            } else if i == last && before > 0 && before + subset.len() == of_kind {
                let count = ["no", "one", "two", "three"]
                    .get(subset.len())
                    .copied()
                    .unwrap_or("all");
                let noun = if subset.len() == 1 { noun } else { nouns };
                places.push(format!("{at} the other {count} {noun}"));
            } else {
                let names: Vec<&str> = subset
                    .iter()
                    .filter_map(|&l| LANE_NAMES.get(l).copied())
                    .collect();
                let noun = if subset.len() == 1 { noun } else { nouns };
                places.push(format!("{at} the {} {noun}", and_list(&names)));
            }
        }
        for &l in lanes {
            if let Some(c) = covered.get_mut(l) {
                *c = true;
            }
        }
        parts.push(format!("{terrain} {}", places.join(" and ")));
    }
    and_list(&parts)
}

#[cfg(test)]
mod tests {
    use bt_core::{TileId, Tileset, TilesetSlot, WangColor, WangId, WangSet};
    use bt_test_data::fixtures::Atlas;

    use super::*;

    fn name(color: TerrainColor) -> String {
        ["nothing", "Sand", "Grass", "Water"][usize::from(color.0)].to_owned()
    }

    fn corner_set(labels: &[[u8; 4]]) -> Terrain {
        let mut tileset: Tileset = (*Atlas::builder()
            .grid(8, 8)
            .build_embedded()
            .unwrap()
            .tileset)
            .clone();
        let color = |n: &str| WangColor {
            name: n.to_owned(),
            class: String::new(),
            color: bt_core::Color::rgb(0, 0, 0),
            tile: None,
            probability: 1.0,
            properties: Default::default(),
        };
        let mut set = WangSet {
            name: "Ground".to_owned(),
            class: String::new(),
            kind: WangSetType::Corner,
            tile: None,
            colors: vec![color("Sand"), color("Grass"), color("Water")],
            tiles: Default::default(),
            properties: Default::default(),
        };
        for (i, c) in labels.iter().enumerate() {
            set.tiles.insert(
                TileId(i as u32),
                WangId::from_lanes([0, c[0], 0, c[1], 0, c[2], 0, c[3]]),
            );
        }
        tileset.wang_sets.push(set);
        Terrain::from_tileset(&tileset, TilesetSlot::new(0).unwrap(), 0).unwrap()
    }

    fn need(lanes: Pattern, cause: Cause) -> Needed {
        Needed {
            cell: IVec2::ZERO,
            lanes,
            cause,
        }
    }

    #[test]
    fn patterns_read_as_where_each_terrain_goes() {
        let one_corner = [None, Some(1), None, Some(2), None, Some(2), None, Some(2)];
        assert_eq!(
            describe(&one_corner, WangSetType::Corner, &name),
            "Sand at the top-right corner and Grass at the other three corners"
        );
        let two = [None, Some(1), None, Some(1), None, Some(2), None, Some(2)];
        assert_eq!(
            describe(&two, WangSetType::Corner, &name),
            "Sand at the top-right and bottom-right corners and Grass at the other two corners"
        );
        let edges = [Some(3), None, None, None, None, None, None, None];
        assert_eq!(
            describe(&edges, WangSetType::Edge, &name),
            "Water on the top edge"
        );
        let all = [None, Some(2), None, Some(2), None, Some(2), None, Some(2)];
        assert_eq!(
            describe(&all, WangSetType::Corner, &name),
            "Grass at every corner"
        );
    }

    #[test]
    fn the_commonest_lack_is_said_with_its_plain_reason() {
        // Sand, Grass and one transition between them; Water alone.
        let terrain = corner_set(&[[1, 1, 1, 1], [2, 2, 2, 2], [1, 1, 1, 2], [3, 3, 3, 3]]);
        let joining = [None, Some(1), None, Some(3), None, None, None, None];
        let twice = [need(joining, Cause::NoTile), need(joining, Cause::NoTile)];
        assert_eq!(
            missing_text(&terrain, &name, &twice).unwrap(),
            "no tile fits 2 cells: “Ground” has no tile joining Sand and Water"
        );
        let general = [None, Some(1), None, Some(1), None, Some(2), None, Some(2)];
        let mixed = [
            need(general, Cause::NoTile),
            need(general, Cause::NoTile),
            need(joining, Cause::NoTile),
        ];
        assert_eq!(
            missing_text(&terrain, &name, &mixed).unwrap(),
            "no tile fits 3 cells: “Ground” has none with Sand at the top-right and \
             bottom-right corners and Grass at the other two corners (and 1 other pattern)"
        );
        let nothing = [None, Some(0), None, Some(2), None, None, None, None];
        assert_eq!(
            missing_text(&terrain, &name, &[need(nothing, Cause::NoTile)]).unwrap(),
            "no tile fits 1 cell: “Ground” has no tile where Grass borders nothing"
        );
        let plain = [None, Some(2), None, Some(2), None, Some(2), None, Some(2)];
        assert_eq!(
            missing_text(&terrain, &name, &[need(plain, Cause::ZeroProbability)]).unwrap(),
            "no tile fits 1 cell: the tile that fits best in “Ground” has probability 0"
        );
        assert_eq!(missing_text(&terrain, &name, &[]), None);
    }

    #[test]
    fn a_fill_of_a_terrain_with_no_plain_tile_says_so() {
        let terrain = corner_set(&[[1, 1, 1, 1], [1, 1, 1, 2]]);
        assert_eq!(fill_note(&terrain, &name, TerrainColor(1)), None);
        assert_eq!(
            fill_note(&terrain, &name, TerrainColor(2)).unwrap(),
            "“Ground” has no tile that is all Grass, so the fill places its transitions"
        );
        assert_eq!(fill_note(&terrain, &name, TerrainColor::NONE), None);
    }

    #[test]
    fn other_sets_painted_over_are_named() {
        let replaced = [ReplacedSet {
            set_name: "Water".to_owned(),
            cells: vec![IVec2::ZERO, IVec2::ONE],
        }];
        assert_eq!(
            replaced_text(&replaced).unwrap(),
            "painted over 2 cells of “Water”: paint each terrain set on a layer of its own"
        );
        assert_eq!(replaced_text(&[]), None);
    }
}
