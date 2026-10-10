//! The solver against Tiled's sample terrain sets (`reference/tiled/tests/wangtiles`). Tiled's
//! picks cannot be seeded, so what is checked is what holds for any of its picks: every tile
//! chosen is a lowest-penalty, positive-weight choice that passes the lookahead, every cell left
//! unfilled has no such tile, every cell asked for is decided, and the result agrees wherever
//! tiles touch. The checks work from the set's labels with code of their own, not the solver's.

use std::collections::VecDeque;
use std::sync::Arc;

use bt_core::{Cell, Map, Tileset, TilesetSlot, WangSetType};
use bt_test_data::fixtures::MapBuilder;
use glam::{IVec2, UVec2};

use crate::color::TerrainColor;
use crate::filler::{Decision, Filler, Outcome};
use crate::request::BrushTarget;
use crate::terrain::Terrain;
use crate::topology::Topology;

const FREE: u8 = 0xFF;

/// A tile as the checks see it.
#[derive(Clone, Debug)]
struct Choice {
    cell: Cell,
    lanes: [u8; 8],
    weight: f64,
}

/// What the checks know of a set, from its labels alone.
struct Oracle {
    kind_lanes: [bool; 8],
    choices: Vec<Choice>,
    /// `distance[a][b]`, -1 when unreachable.
    distance: Vec<Vec<i32>>,
    max_distance: i32,
    complete: bool,
}

impl Oracle {
    fn of(tileset: &Tileset, set_index: usize) -> Oracle {
        let set = &tileset.wang_sets[set_index];
        assert!(
            tileset.transformations.is_empty(),
            "the checks know plain tiles only"
        );
        let kind_lanes: [bool; 8] = match set.kind {
            WangSetType::Corner => std::array::from_fn(|l| l % 2 == 1),
            WangSetType::Edge => std::array::from_fn(|l| l % 2 == 0),
            WangSetType::Mixed => [true; 8],
        };
        let n = set.colors.len();
        let mut choices = Vec::new();
        for (tile, label) in &set.tiles {
            let raw = label.lanes();
            if !tileset.has_tile(*tile)
                || raw.iter().all(|&c| c == 0)
                || raw.iter().any(|&c| usize::from(c) > n)
            {
                continue;
            }
            let lanes: [u8; 8] = std::array::from_fn(|l| if kind_lanes[l] { raw[l] } else { 0 });
            let mut weight = tileset.tile(*tile).map_or(1.0, |d| d.probability);
            for &c in &lanes {
                if c != 0 {
                    weight *= set.colors[usize::from(c) - 1].probability;
                }
            }
            choices.push(Choice {
                cell: Cell::new(TilesetSlot::new(0).unwrap(), *tile),
                lanes,
                weight,
            });
        }
        // A step between two terrains on the corners, or two on the edges, of one tile.
        let mut step = vec![vec![false; n + 1]; n + 1];
        for choice in &choices {
            for group in [[1, 3, 5, 7], [0, 2, 4, 6]] {
                let colors: Vec<usize> = group
                    .iter()
                    .map(|&l| usize::from(choice.lanes[l]))
                    .collect();
                for &a in &colors {
                    for &b in &colors {
                        if a != b && (a != 0 || b != 0) && colors.iter().any(|&c| c != 0) {
                            step[a][b] = true;
                        }
                    }
                }
            }
        }
        // Shortest paths between terrains, through "no terrain" too; distances to "no terrain"
        // are direct only.
        let mut distance = vec![vec![-1; n + 1]; n + 1];
        let mut max_distance = 1;
        for from in 1..=n {
            let mut seen = vec![-1; n + 1];
            seen[from] = 0;
            let mut queue = VecDeque::from([from]);
            while let Some(c) = queue.pop_front() {
                for next in 0..=n {
                    if step[c][next] && seen[next] == -1 {
                        seen[next] = seen[c] + 1;
                        queue.push_back(next);
                    }
                }
            }
            for to in 1..=n {
                distance[from][to] = seen[to];
                if from != to && seen[to] > 0 {
                    max_distance = max_distance.max(seen[to]);
                }
            }
            distance[from][0] = if step[from][0] { 1 } else { -1 };
        }
        let complete = match set.kind {
            WangSetType::Mixed => {
                let full: std::collections::HashSet<[u8; 8]> = choices
                    .iter()
                    .map(|c| c.lanes)
                    .filter(|l| !l.contains(&0))
                    .collect();
                full.len() as u64 == (n as u64).pow(8)
            }
            _ => false,
        };
        Oracle {
            kind_lanes,
            choices,
            distance,
            max_distance,
            complete,
        }
    }

    fn penalty(&self, wanted: u8, has: u8) -> i32 {
        match (usize::from(wanted), usize::from(has)) {
            (0, 0) => 0,
            (0, b) => self.distance[b][0],
            (a, b) => self.distance[a][b],
        }
    }

    /// Whether some tile has `wanted` where `mask` is set, within the set's lanes.
    fn has(&self, wanted: [u8; 8], mask: [u8; 8]) -> bool {
        self.choices.iter().any(|c| {
            (0..8).all(|l| !self.kind_lanes[l] || mask[l] != FREE || c.lanes[l] == wanted[l])
        })
    }

    /// Checks one decision; returns what was wrong.
    fn check(&self, decision: &Decision, corrections: bool, erasing: bool) -> Result<(), String> {
        let wanted = decision.info.wanted.lanes();
        let mask = decision.info.mask.lanes();
        let erase = Choice {
            cell: Cell::EMPTY,
            lanes: [0; 8],
            weight: 1.0,
        };
        let mut tiers: Vec<(i32, &Choice)> = Vec::new();
        for choice in self.choices.iter().chain(erasing.then_some(&erase)) {
            if (0..8).any(|l| mask[l] == FREE && choice.lanes[l] != wanted[l]) {
                continue;
            }
            let mut total = 0;
            let mut rejected = false;
            for (&want, &has) in wanted.iter().zip(&choice.lanes) {
                if want == FREE || want == has {
                    continue;
                }
                let mut p = self.penalty(want, has);
                if p < 0 {
                    if corrections {
                        rejected = true;
                        break;
                    }
                    p = self.max_distance + 1;
                }
                total += p;
            }
            if !rejected {
                tiers.push((total, choice));
            }
        }
        let lowest = tiers.iter().map(|(t, _)| *t).min();
        let best: Vec<&Choice> = tiers
            .iter()
            .filter(|(t, c)| Some(*t) == lowest && c.weight > 0.0)
            .map(|(_, c)| *c)
            .collect();
        let look = !corrections && !self.complete;
        let passes =
            |choice: &Choice| !look || self.leaves_neighbours_a_tile(decision, choice.lanes);
        match decision.chosen {
            Some(cell) => {
                let ok = best.iter().any(|c| c.cell == cell && passes(c));
                if ok {
                    Ok(())
                } else {
                    Err(format!(
                        "{}: chose {cell:?}, not a best tile passing the lookahead ({} best)",
                        decision.cell,
                        best.len()
                    ))
                }
            }
            None => match best.iter().find(|c| passes(c)) {
                Some(c) => Err(format!(
                    "{}: left unfilled, but {:?} fits",
                    decision.cell, c.cell
                )),
                None => Ok(()),
            },
        }
    }

    /// Tiled's lookahead: every unsettled neighbour still has some tile once `placed` is there.
    fn leaves_neighbours_a_tile(&self, decision: &Decision, placed: [u8; 8]) -> bool {
        decision
            .neighbours
            .iter()
            .enumerate()
            .all(|(adjacent_lane, n)| {
                if n.settled {
                    return true;
                }
                let mut wanted = n.info.wanted.lanes();
                let mut mask = n.info.mask.lanes();
                let lane = (adjacent_lane + 4) % 8;
                wanted[lane] = placed[adjacent_lane];
                mask[lane] = FREE;
                if lane % 2 == 0 {
                    for (corner, from) in [
                        ((lane + 1) % 8, (adjacent_lane + 7) % 8),
                        ((lane + 7) % 8, (adjacent_lane + 1) % 8),
                    ] {
                        wanted[corner] = placed[from];
                        mask[corner] = FREE;
                    }
                }
                wanted.iter().all(|&c| c == 0) || self.has(wanted, mask)
            })
    }

    /// Every two neighbouring tiles placed by the run agree where they touch: a placed tile
    /// passes its lanes to its unsettled neighbours as rules. (Against the tiles around a fill,
    /// or beyond what the brush re-solves, the wishes are soft, so a seam there is Tiled's.)
    fn check_agreement(&self, outcome: &Outcome) -> Result<(), String> {
        let lanes_at = |p: IVec2| -> Option<[u8; 8]> {
            let cell = outcome.placed.iter().find(|c| c.at == p)?.cell;
            if cell.is_empty() {
                return Some([0; 8]);
            }
            self.choices
                .iter()
                .find(|c| c.cell.same_tile(cell))
                .map(|c| c.lanes)
        };
        for placed in &outcome.placed {
            let p = placed.at;
            let Some(a) = lanes_at(p) else { continue };
            for (offset, touching) in [
                (IVec2::X, [(2, 6), (1, 7), (3, 5)]),
                (IVec2::Y, [(4, 0), (5, 7), (3, 1)]),
            ] {
                let Some(b) = lanes_at(p + offset) else {
                    continue;
                };
                for (mine, theirs) in touching {
                    if self.kind_lanes[mine] && a[mine] != b[theirs] {
                        return Err(format!("{p} and {} disagree: {a:?} {b:?}", p + offset));
                    }
                }
            }
        }
        Ok(())
    }
}

fn sample_sets() -> Vec<Arc<Tileset>> {
    let Some(folder) = bt_test_data::test_data("reference/tiled/tests/wangtiles") else {
        return Vec::new();
    };
    [
        "PathAndObjects.tsx",
        "grassAndWater.tsx",
        "walkways.tsx",
        "wangblob.tsx",
    ]
    .iter()
    .map(|name| {
        bt_tmx::Loader::default()
            .load_tileset(&folder.join(name))
            .unwrap()
            .value
    })
    .collect()
}

fn map_of(tileset: &Arc<Tileset>, base: Option<&Outcome>) -> Map {
    let mut map = MapBuilder::new(UVec2::splat(32), UVec2::splat(10))
        .tileset(Arc::clone(tileset))
        .tile_layer("ground", "")
        .build()
        .unwrap();
    if let Some(base) = base {
        let layer = map.layers[0].as_tile_layer_mut().unwrap();
        for placed in &base.placed {
            layer.set_cell(placed.at, placed.cell).unwrap();
        }
    }
    map
}

/// Runs the checks on one outcome of a run.
fn check_run(
    name: &str,
    terrain: &Terrain,
    oracle: &Oracle,
    outcome: &Outcome,
    corrections: bool,
    erasing: bool,
) {
    for decision in &outcome.decisions {
        if let Err(problem) = oracle.check(decision, corrections, erasing) {
            panic!("{name}: {problem}");
        }
    }
    if let Err(problem) = oracle.check_agreement(outcome) {
        panic!("{name}: {problem}");
    }
    for need in &outcome.needed {
        let placeable = need.cause == crate::diagnose::Cause::ZeroProbability;
        assert!(
            !terrain.has_tile_with(&need.lanes, placeable),
            "{name}: the set has a tile for {need:?}"
        );
    }
}

#[test]
fn tileds_sample_sets_choose_as_tiled_may() {
    let mut runs = 0;
    for tileset in sample_sets() {
        for set_index in 0..tileset.wang_sets.len() {
            let set_name = format!("{} / {}", tileset.name, tileset.wang_sets[set_index].name);
            let oracle = Oracle::of(&tileset, set_index);
            let empty = map_of(&tileset, None);
            let terrain = Terrain::new(&empty, TilesetSlot::new(0).unwrap(), set_index).unwrap();
            let colors = terrain.color_count() as u8;
            let topology = Topology::of(&empty);
            let layer = empty.layers[0].as_tile_layer().unwrap();
            let all: Vec<IVec2> = (0..10)
                .flat_map(|y| (0..10).map(move |x| IVec2::new(x, y)))
                .collect();
            for seed in 0..4 {
                // Tiled's fill over an empty layer.
                let mut filler = Filler::new(&terrain, layer, &topology, false, true, seed);
                filler.set_region(&all);
                let base = filler.run();
                let name = format!("{set_name}, fill, seed {seed}");
                check_run(&name, &terrain, &oracle, &base, false, true);
                let decided = base.placed.len() + base.invalid.len();
                assert_eq!(decided, all.len(), "{name}: every cell is decided");
                runs += 1;

                let filled = map_of(&tileset, Some(&base));
                let filled_layer = filled.layers[0].as_tile_layer().unwrap();
                let inner: Vec<IVec2> = all
                    .iter()
                    .copied()
                    .filter(|p| (3..7).contains(&p.x) && (3..7).contains(&p.y))
                    .collect();
                for color in 0..=colors {
                    // A fill of each terrain over the filled layer, without empty cells.
                    let mut filler =
                        Filler::new(&terrain, filled_layer, &topology, false, false, seed);
                    filler.set_region(&inner);
                    if color != 0 {
                        filler.prefer_color(color, terrain.brush_kind(TerrainColor(color)));
                    }
                    let outcome = filler.run();
                    let name = format!("{set_name}, fill with {color}, seed {seed}");
                    check_run(&name, &terrain, &oracle, &outcome, false, false);
                    runs += 1;

                    // The brush, with each kind of target its terrain paints.
                    let kind = terrain.brush_kind(TerrainColor(color));
                    let mut targets = vec![BrushTarget::Tile(IVec2::new(5, 5))];
                    if kind != WangSetType::Edge {
                        targets.push(BrushTarget::Corner(IVec2::new(4, 6)));
                    }
                    if kind != WangSetType::Corner {
                        targets.push(BrushTarget::Edge {
                            cell: IVec2::new(6, 4),
                            side: bt_core::Lane::Right,
                        });
                    }
                    for target in targets {
                        let mut filler =
                            Filler::new(&terrain, filled_layer, &topology, true, true, seed);
                        filler.set_target(target, color, kind);
                        let outcome = filler.run();
                        let name = format!("{set_name}, {target:?} with {color}, seed {seed}");
                        check_run(&name, &terrain, &oracle, &outcome, true, true);
                        runs += 1;
                    }
                }
            }
        }
    }
    if bt_test_data::test_data("reference/tiled/tests/wangtiles").is_some() {
        assert!(runs > 100, "{runs} runs");
    }
}

#[test]
fn the_checks_agree_with_the_prepared_set_on_distances() {
    for tileset in sample_sets() {
        for set_index in 0..tileset.wang_sets.len() {
            let oracle = Oracle::of(&tileset, set_index);
            let terrain =
                Terrain::from_tileset(&tileset, TilesetSlot::new(0).unwrap(), set_index).unwrap();
            let n = terrain.color_count() as u8;
            for a in 0..=n {
                for b in 0..=n {
                    assert_eq!(
                        terrain.raw_penalty(a, b),
                        oracle.penalty(a, b),
                        "{} set {set_index}: {a} to {b}",
                        tileset.name
                    );
                }
            }
            assert_eq!(terrain.raw_max_distance(), oracle.max_distance);
            assert_eq!(terrain.is_complete(), oracle.complete);
            assert_eq!(terrain.candidate_count(), oracle.choices.len());
        }
    }
}
