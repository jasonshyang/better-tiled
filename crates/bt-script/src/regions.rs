//! `bt areas` and `bt reach` (`10-agents.md`, §10.5): how a map's cells connect, as typed
//! outcomes. Cells that share a side are connected ([`crate::grid`]); which cells count is the
//! agent's, written as shapes, so `reach` knows nothing of gravity or doors.

use std::collections::BTreeMap;

use bt_core::{Map, MapGeometry, Orientation};
use bt_utils::IRect;
use bt_utils::hash::CellHashSet;
use bt_utils::text::{and_list, plural};
use glam::IVec2;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::grid::{self, Nearest, sides};
use crate::names::Names;
use crate::report::{Line, Outcome, Report, Section};
use crate::shapes::map_cells;
use crate::words::{RectJson, capped_list, cell_text, cells_text, region_text};

/// The areas the text lists unless asked for all of them.
const AREAS_SHOWN: usize = 25;

/// The most cells an area's line lists them by; a larger area is written by its rectangle.
const AREA_CELLS_LISTED: usize = 6;

/// The most cells of a way listed.
const WAY_CELLS_LISTED: usize = 60;

/// The most cells every way passes through listed.
const NARROWS_LISTED: usize = 20;

/// The most kinds of cells a border is said to be made of.
const BORDER_KINDS_LISTED: usize = 6;

/// A shape as written and the cells it gave on the map.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeCells {
    /// The shape as written.
    pub written: String,
    /// Its cells.
    pub cells: Vec<IVec2>,
}

/// `bt areas MAP SHAPE`: the connected areas of a shape's cells, the largest first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreasOutcome {
    /// The shape as written.
    pub shape: String,
    /// How many cells the shape has.
    pub cells: usize,
    /// Its connected areas, the largest first (then by their first cell, row by row).
    pub areas: Vec<ConnectedArea>,
    /// Whether the text lists every area (`--all`) rather than the first few.
    pub list_all: bool,
}

/// One connected area of a shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectedArea {
    /// How many cells it has.
    pub size: usize,
    /// The rectangle holding it.
    #[serde(with = "RectJson")]
    pub bounds: IRect,
    /// The edges of a finite map it reaches.
    pub edges: Vec<MapEdge>,
    /// Its cell nearest its middle: what `containing X,Y` names it by.
    pub cell: IVec2,
    /// Its cells, when it has at most six; empty for a larger area.
    pub cells: Vec<IVec2>,
}

/// An edge of a finite map: its first or last column or row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapEdge {
    /// Whether it is a column (`x`) or a row (`y`).
    pub axis: GridAxis,
    /// Which column or row.
    pub at: i32,
    /// Where it is on screen: `left`, `top`, `right`, `bottom`; on isometric maps `upper left`,
    /// `upper right`, `lower right`, `lower left`.
    pub screen: String,
}

/// The axis of a column or a row of cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridAxis {
    /// A column: cells of one `x`.
    X,
    /// A row: cells of one `y`.
    Y,
}

impl AreasOutcome {
    /// The areas of `shape`'s cells on `map`; `list_all` lists them all in text.
    pub fn from_cells(map: &Map, shape: &ShapeCells, list_all: bool) -> AreasOutcome {
        let geometry = map.geometry();
        let bounds = (!map.infinite).then(|| map_cells(map));
        let areas = grid::areas(&geometry, &shape.cells)
            .into_iter()
            .map(|cells| connected_area(&geometry, bounds, cells))
            .collect();
        let cells: CellHashSet<IVec2> = shape.cells.iter().copied().collect();
        AreasOutcome {
            shape: shape.written.clone(),
            cells: cells.len(),
            areas,
            list_all,
        }
    }
}

/// An area's facts: its size, rectangle, the map edges it reaches and a cell in it.
fn connected_area(geometry: &MapGeometry, map: Option<IRect>, cells: Vec<IVec2>) -> ConnectedArea {
    let bounds = IRect::bounding(cells.iter().copied()).unwrap_or_default();
    let edges = map.map_or_else(Vec::new, |map| edges_reached(geometry, map, bounds));
    let cell = middle_cell(&cells).unwrap_or(bounds.min());
    ConnectedArea {
        size: cells.len(),
        bounds,
        edges,
        cell,
        cells: if cells.len() <= AREA_CELLS_LISTED {
            cells
        } else {
            Vec::new()
        },
    }
}

/// The edges of the map's cells `map` that `bounds` reaches.
fn edges_reached(geometry: &MapGeometry, map: IRect, bounds: IRect) -> Vec<MapEdge> {
    let screen: [&str; 4] = if geometry.orientation == Orientation::Isometric {
        ["upper left", "upper right", "lower right", "lower left"]
    } else {
        ["left", "top", "right", "bottom"]
    };
    let [left, top, right, bottom] = screen;
    let edge = |axis: GridAxis, at: i32, screen: &str| MapEdge {
        axis,
        at,
        screen: screen.to_owned(),
    };
    let mut edges = Vec::new();
    if bounds.x == map.x {
        edges.push(edge(GridAxis::X, map.x, left));
    }
    if bounds.y == map.y {
        edges.push(edge(GridAxis::Y, map.y, top));
    }
    if bounds.max().x == map.max().x {
        edges.push(edge(GridAxis::X, map.max().x - 1, right));
    }
    if bounds.max().y == map.max().y {
        edges.push(edge(GridAxis::Y, map.max().y - 1, bottom));
    }
    edges
}

/// The cell of `cells` nearest their middle, the first of the nearest.
fn middle_cell(cells: &[IVec2]) -> Option<IVec2> {
    let count = cells.len() as f64;
    let middle = cells.iter().map(|c| c.as_dvec2()).sum::<glam::DVec2>() / count.max(1.0);
    cells.iter().copied().min_by(|a, b| {
        let distance = |c: &IVec2| c.as_dvec2().distance_squared(middle);
        distance(a).total_cmp(&distance(b))
    })
}

impl Outcome for AreasOutcome {
    const SCHEMA: &'static str = "bt.areas/1";

    fn report(&self) -> Report {
        let mut report = Report::new();
        if self.areas.is_empty() {
            report.add_line(Line::fact(format!("{}: no cells", self.shape)));
            return report;
        }
        let mut section = Section::headed(format!(
            "{}: {} in {} (cells that share a side are connected), the largest first:",
            self.shape,
            plural(self.cells, "cell", "cells"),
            plural(self.areas.len(), "area", "areas")
        ));
        let shown = if self.list_all {
            self.areas.len()
        } else {
            self.areas.len().min(AREAS_SHOWN)
        };
        for (number, area) in (1..).zip(self.areas.iter().take(shown)) {
            section.add_line(Line::fact(format!("{number}. {}", area_text(area))));
        }
        if let Some(rest) = self.areas.get(shown..).filter(|rest| !rest.is_empty()) {
            let largest = rest.first().map_or(0, |a| a.size);
            let smallest = rest.last().map_or(0, |a| a.size);
            let sizes = if largest == smallest {
                plural(largest, "cell", "cells")
            } else {
                format!("{smallest}-{largest} cells")
            };
            let noun = if rest.len() == 1 { "area" } else { "areas" };
            section.add_line(Line::fact(format!(
                "… and {} more {noun}, {sizes} each (--all lists them)",
                rest.len()
            )));
        }
        report.add_section(section);
        if self.areas.len() > 1
            && let Some(first) = self.areas.first()
        {
            report.add_line(Line::fact(format!(
                "In scripts, `{} containing {}` is area 1 (any of an area's cells will do).",
                self.shape,
                cell_text(first.cell)
            )));
        }
        report
    }
}

/// `9 cells in 7,1 3x3, around 8,2; on the map's edge at x=0 (left)`, or a small area's cells.
fn area_text(area: &ConnectedArea) -> String {
    let mut text = if area.cells.is_empty() {
        format!(
            "{} in {}, around {}",
            plural(area.size, "cell", "cells"),
            region_text(area.bounds),
            cell_text(area.cell)
        )
    } else {
        format!(
            "{}: {}",
            plural(area.size, "cell", "cells"),
            cells_text(&area.cells, AREA_CELLS_LISTED)
        )
    };
    if !area.edges.is_empty() {
        let edges: Vec<String> = area
            .edges
            .iter()
            .map(|edge| {
                let axis = match edge.axis {
                    GridAxis::X => "x",
                    GridAxis::Y => "y",
                };
                format!("{axis}={} ({})", edge.at, edge.screen)
            })
            .collect();
        text.push_str(&format!("; on the map's edge at {}", and_list(&edges)));
    }
    text
}

/// What `bt reach MAP FROM TO [--through SHAPE]` asks.
#[derive(Clone, Debug, PartialEq)]
pub struct ReachRequest {
    from: ShapeCells,
    to: ShapeCells,
    through: Option<ShapeCells>,
}

impl ReachRequest {
    /// A way from the cells of `from` to the cells of `to`, through any cell of the map.
    pub fn new(from: ShapeCells, to: ShapeCells) -> ReachRequest {
        ReachRequest {
            from,
            to,
            through: None,
        }
    }

    /// The same, through the cells of `through` only (the ends may lie outside it).
    pub fn through(mut self, through: ShapeCells) -> ReachRequest {
        self.through = Some(through);
        self
    }
}

/// `bt reach MAP FROM TO [--through SHAPE]`: the shortest way through the allowed cells, or how
/// far one gets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReachOutcome {
    /// Where the way starts, as written.
    pub from: String,
    /// Where it ends, as written.
    pub to: String,
    /// The cells it may go through, as written; the whole map when there is none.
    pub through: Option<String>,
    /// The ends (as written) with no cell in `through`: a way starts and ends anywhere.
    pub ends_outside: Vec<String>,
    /// The way, or how far one gets.
    pub reach: Reach,
}

/// What `reach` found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "found", rename_all = "snake_case")]
pub enum Reach {
    /// A way.
    Way(Way),
    /// No way.
    NoWay(NoWay),
}

/// The shortest way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Way {
    /// How many steps between cells that share a side it takes.
    pub steps: usize,
    /// Its ends and the cells where it turns.
    pub turns: Vec<IVec2>,
    /// Its cells, ends included, when there are at most sixty; else empty.
    pub cells: Vec<IVec2>,
    /// The cells every way passes through, ends left out: closing one closes them all.
    pub on_every_way: Vec<IVec2>,
}

/// How far one gets when there is no way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoWay {
    /// How many cells can be reached from `from`, its own included.
    pub reached: usize,
    /// The rectangle holding them.
    #[serde(with = "RectJson")]
    pub bounds: IRect,
    /// Their cell nearest their middle.
    pub around: IVec2,
    /// The reached cell nearest to `to`, with its steps to it counting every cell between,
    /// whatever is on it.
    pub nearest: Option<Nearest>,
    /// What the cells next to the reached ones that a way may not use show, the most first.
    pub border: Vec<CellsShowing>,
}

/// How many cells show one thing from above.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellsShowing {
    /// What they show: a terrain, `grass/water edges`, a tile, or `nothing`.
    pub what: String,
    /// How many cells.
    pub cells: usize,
}

impl ReachOutcome {
    /// The shortest way `request` asks for on `map`; refused when an end has no cells.
    pub fn from_request(
        map: &Map,
        names: &Names,
        request: &ReachRequest,
    ) -> Result<ReachOutcome, Error> {
        for end in [&request.from, &request.to] {
            if end.cells.is_empty() {
                return Err(Error::NoCells {
                    written: end.written.clone(),
                });
            }
        }
        let geometry = map.geometry();
        let bounds = map_cells(map);
        let allowed: CellHashSet<IVec2> = match &request.through {
            Some(through) => through.cells.iter().copied().collect(),
            None => bounds.points().collect(),
        };
        let ends_outside = match &request.through {
            Some(_) => [&request.from, &request.to]
                .into_iter()
                .filter(|end| !end.cells.iter().any(|cell| allowed.contains(cell)))
                .map(|end| end.written.clone())
                .collect(),
            None => Vec::new(),
        };
        let from = &request.from.cells;
        let to = &request.to.cells;
        let open = |cell: IVec2| allowed.contains(&cell);
        let reach = match grid::way(&geometry, from, to, open) {
            Some(cells) => {
                let on_every_way = if cells.len() > 2 {
                    grid::on_every_way(&geometry, from, to, open, &cells)
                } else {
                    Vec::new()
                };
                Reach::Way(Way {
                    steps: cells.len().saturating_sub(1),
                    turns: grid::turns(&geometry, &cells),
                    cells: if cells.len() <= WAY_CELLS_LISTED {
                        cells
                    } else {
                        Vec::new()
                    },
                    on_every_way,
                })
            }
            None => Reach::NoWay(no_way(map, names, &geometry, request, &allowed)),
        };
        Ok(ReachOutcome {
            from: request.from.written.clone(),
            to: request.to.written.clone(),
            through: request.through.as_ref().map(|t| t.written.clone()),
            ends_outside,
            reach,
        })
    }
}

/// How far one gets from `from` through the allowed cells, the reached cell nearest to `to`,
/// and what closes the reached cells in.
fn no_way(
    map: &Map,
    names: &Names,
    geometry: &MapGeometry,
    request: &ReachRequest,
    allowed: &CellHashSet<IVec2>,
) -> NoWay {
    let reached = grid::flood(geometry, &request.from.cells, |cell| {
        allowed.contains(&cell)
    });
    let sorted = grid::sorted_cells(reached.iter().copied());
    let bounds = IRect::bounding(sorted.iter().copied()).unwrap_or_default();
    let map_bounds = map_cells(map);
    // Counting every cell between: on the map, or around everything there is on an infinite
    // map.
    let room = if map.infinite {
        let around = IRect::bounding(
            sorted
                .iter()
                .chain(&request.to.cells)
                .copied()
                .chain([map_bounds.min(), map_bounds.max() - IVec2::ONE]),
        )
        .unwrap_or(map_bounds);
        IRect::from_min_size(around.min() - IVec2::ONE, around.size() + IVec2::splat(2))
    } else {
        map_bounds
    };
    let nearest = grid::nearest(geometry, &request.to.cells, &reached, |cell| {
        room.contains(cell)
    });
    let on_map = |cell: IVec2| map.infinite || map_bounds.contains(cell);
    let border: Vec<IVec2> = grid::sorted_cells(
        sorted
            .iter()
            .flat_map(|&cell| sides(geometry, cell))
            .filter(|&side| on_map(side) && !reached.contains(&side)),
    );
    NoWay {
        reached: sorted.len(),
        bounds,
        around: middle_cell(&sorted).unwrap_or(bounds.min()),
        nearest,
        border: made_of(map, names, &border),
    }
}

/// What the cells show from above, on the top shown tile layer that has a tile there, the most
/// first.
fn made_of(map: &Map, names: &Names, cells: &[IVec2]) -> Vec<CellsShowing> {
    let mut layers = Vec::new();
    map.for_each_layer(|layer, _| {
        let shown = map.effective_layer(layer.id).is_some_and(|l| l.visible);
        if let Some(tiles) = layer.as_tile_layer().filter(|_| shown) {
            layers.push(tiles);
        }
    });
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for &cell in cells {
        let top = layers
            .iter()
            .rev()
            .map(|layer| layer.cell(cell))
            .find(|tile| !tile.is_empty());
        let what = top.map_or_else(|| "nothing".to_owned(), |tile| names.tile_label(map, tile));
        *counts.entry(what).or_default() += 1;
    }
    let mut showing: Vec<CellsShowing> = counts
        .into_iter()
        .map(|(what, cells)| CellsShowing { what, cells })
        .collect();
    showing.sort_by(|a, b| b.cells.cmp(&a.cells).then_with(|| a.what.cmp(&b.what)));
    showing
}

impl Outcome for ReachOutcome {
    const SCHEMA: &'static str = "bt.reach/1";

    fn report(&self) -> Report {
        let through = self
            .through
            .as_ref()
            .map_or_else(String::new, |t| format!(" through {t}"));
        let mut section = Section::headed(format!("from {} to {}{through}:", self.from, self.to));
        for end in &self.ends_outside {
            section.add_line(Line::note(format!(
                "{end} isn't in --through: a way may start and end outside it, every cell between is in it"
            )));
        }
        match &self.reach {
            Reach::Way(way) => {
                let turns: Vec<String> = way.turns.iter().map(|&c| cell_text(c)).collect();
                section.add_line(Line::fact(format!(
                    "yes: {} at the shortest, {}",
                    plural(way.steps, "step", "steps"),
                    turns.join(" → ")
                )));
                if !way.cells.is_empty() {
                    section.add_line(Line::fact(format!(
                        "its cells: {}",
                        cells_text(&way.cells, WAY_CELLS_LISTED)
                    )));
                }
                if way.steps > 1 {
                    section.add_line(Line::fact(if way.on_every_way.is_empty() {
                        "no cell is on every way: each has a way around it".to_owned()
                    } else {
                        format!(
                            "every way passes through {}",
                            cells_text(&way.on_every_way, NARROWS_LISTED)
                        )
                    }));
                }
            }
            Reach::NoWay(stuck) => {
                let mut line = format!(
                    "no way: from {}, {} can be reached in {}, around {}",
                    self.from,
                    plural(stuck.reached, "cell", "cells"),
                    region_text(stuck.bounds),
                    cell_text(stuck.around)
                );
                if let Some(nearest) = stuck.nearest {
                    line.push_str(&format!(
                        "; the nearest of them to {} is {}, {} from it counting every cell, whatever is on it",
                        self.to,
                        cell_text(nearest.cell),
                        plural(nearest.steps, "step", "steps")
                    ));
                }
                section.add_line(Line::fact(line));
                if !stuck.border.is_empty() {
                    let kinds: Vec<String> = stuck
                        .border
                        .iter()
                        .map(|part| format!("{} ×{}", part.what, part.cells))
                        .collect();
                    let outside = if self.through.is_some() {
                        "in --through"
                    } else {
                        "on the map"
                    };
                    section.add_line(Line::fact(format!(
                        "next to them, not {outside}: {}",
                        capped_list(&kinds, BORDER_KINDS_LISTED)
                    )));
                }
            }
        }
        let mut report = Report::new();
        report.add_section(section);
        report
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bt_core::{Tileset, WangSetType};
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
    use glam::UVec2;

    use super::*;
    use crate::context::Context;
    use crate::report::{from_json, to_json};
    use crate::script::{Area, Shape};
    use crate::shapes::area_cells;

    fn v(x: i32, y: i32) -> IVec2 {
        IVec2::new(x, y)
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect::from_min_size(v(x, y), v(w, h))
    }

    /// A 12×8 map of grass tiles with water tiles in the cells `water` says.
    fn water_map(water: impl Fn(i32, i32) -> bool) -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        let grass = combination_tile(2, [1, 1, 1, 1]).unwrap().0;
        let pool = combination_tile(2, [2, 2, 2, 2]).unwrap().0;
        let rows: Vec<String> = (0..8)
            .map(|y| {
                let cells: Vec<String> = (0..12)
                    .map(|x| if water(x, y) { pool } else { grass }.to_string())
                    .collect();
                cells.join(" ")
            })
            .collect();
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", &rows.join("\n"))
            .build()
            .unwrap()
    }

    fn shape(map: &Map, written: &str, area: Area) -> ShapeCells {
        let cells = area_cells(map, &Names::of(map), &Context::new(1), &area).unwrap();
        ShapeCells {
            written: written.to_owned(),
            cells,
        }
    }

    fn where_water() -> Area {
        Area::of(Shape::Where(vec![crate::script::Named::Name(
            "water".into(),
        )]))
    }

    fn all_but_water() -> Area {
        Area {
            minus: vec![Shape::Where(vec![crate::script::Named::Name(
                "water".into(),
            )])],
            ..Area::of(Shape::All)
        }
    }

    fn cells(list: &[[i32; 2]]) -> ShapeCells {
        let written: Vec<String> = list.iter().map(|c| format!("{},{}", c[0], c[1])).collect();
        ShapeCells {
            written: written.join(" "),
            cells: list.iter().map(|c| v(c[0], c[1])).collect(),
        }
    }

    #[test]
    fn areas_are_the_connected_parts_largest_first() {
        // A pond of 2×2 cells and a lake of 3×3.
        let map = water_map(|x, y| {
            ((1..=2).contains(&x) && (1..=2).contains(&y))
                || ((7..=9).contains(&x) && (1..=3).contains(&y))
        });
        let water = shape(&map, "where water", where_water());
        let outcome = AreasOutcome::from_cells(&map, &water, false);
        assert_eq!(outcome.cells, 13);
        assert_eq!(outcome.areas.len(), 2);
        let lake = &outcome.areas[0];
        assert_eq!(
            (lake.size, lake.bounds, lake.cell),
            (9, rect(7, 1, 3, 3), v(8, 2))
        );
        assert!(lake.cells.is_empty() && lake.edges.is_empty());
        assert_eq!(outcome.areas[1].cells, [v(1, 1), v(2, 1), v(1, 2), v(2, 2)]);
        assert_eq!(
            outcome.report().text(),
            "where water: 13 cells in 2 areas (cells that share a side are connected), the largest first:\n  \
             1. 9 cells in 7,1 3x3, around 8,2\n  \
             2. 4 cells: 1,1 2,1 1,2 2,2\n\
             In scripts, `where water containing 8,2` is area 1 (any of an area's cells will do).\n"
        );
        assert_eq!(
            from_json::<AreasOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
        // The edges of the map an area reaches.
        let corner = cells(&[[0, 0], [1, 0], [0, 1], [1, 1]]);
        let edge = AreasOutcome::from_cells(&map, &corner, false);
        assert_eq!(
            area_text(&edge.areas[0]),
            "4 cells: 0,0 1,0 0,1 1,1; on the map's edge at x=0 (left) and y=0 (top)"
        );
        let none = AreasOutcome::from_cells(&map, &cells(&[]), false);
        assert_eq!(none.report().text(), ": no cells\n");
    }

    #[test]
    fn many_areas_are_cut_in_text_and_kept_in_the_outcome() {
        let map = water_map(|_, _| false);
        let scattered: Vec<[i32; 2]> = (0..6)
            .flat_map(|x| (0..4).map(move |y| [x * 2, y * 2]))
            .collect();
        let outcome = AreasOutcome::from_cells(&map, &cells(&scattered), false);
        assert_eq!(
            outcome.areas.len(),
            24,
            "every other cell of every other row"
        );
        let text = outcome.report().text();
        assert!(!text.contains("… and"), "{text}");
        let more: Vec<[i32; 2]> = (0..12)
            .flat_map(|x| (0..8).map(move |y| [x, y]))
            .filter(|c| (c[0] + c[1]) % 2 == 0)
            .collect();
        let outcome = AreasOutcome::from_cells(&map, &cells(&more), false);
        assert_eq!(outcome.areas.len(), 48);
        assert!(
            outcome
                .report()
                .text()
                .contains("  … and 23 more areas, 1 cell each (--all lists them)\n")
        );
        let all = AreasOutcome::from_cells(&map, &cells(&more), true);
        assert!(all.report().text().contains("  48. 1 cell: "));
    }

    #[test]
    fn a_way_through_the_allowed_cells_with_the_cells_every_way_passes() {
        // A wall of water down column 5 but for a gap at row 6.
        let map = water_map(|x, y| x == 5 && y != 6);
        let names = Names::of(&map);
        let dry = shape(&map, "all minus where water", all_but_water());
        let request = ReachRequest::new(cells(&[[0, 0]]), cells(&[[11, 0]])).through(dry);
        let outcome = ReachOutcome::from_request(&map, &names, &request).unwrap();
        let Reach::Way(way) = &outcome.reach else {
            panic!("a way through the gap: {outcome:?}");
        };
        assert_eq!(way.steps, 11 + 2 * 6);
        assert_eq!(way.turns.first(), Some(&v(0, 0)));
        assert!(way.cells.contains(&v(5, 6)));
        assert_eq!(way.on_every_way, [v(4, 6), v(5, 6), v(6, 6)]);
        let text = outcome.report().text();
        assert!(
            text.starts_with("from 0,0 to 11,0 through all minus where water:\n  yes: 23 steps at the shortest, 0,0 → "),
            "{text}"
        );
        assert!(
            text.ends_with("  every way passes through 4,6 5,6 6,6\n"),
            "{text}"
        );
        assert_eq!(
            from_json::<ReachOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
    }

    #[test]
    fn no_way_says_how_far_one_gets_and_what_is_in_the_way() {
        // A wall of water down columns 5 and 6, top to bottom.
        let map = water_map(|x, _| (5..=6).contains(&x));
        let names = Names::of(&map);
        let dry = shape(&map, "all minus where water", all_but_water());
        let request = ReachRequest::new(cells(&[[0, 7]]), cells(&[[11, 7]])).through(dry);
        let outcome = ReachOutcome::from_request(&map, &names, &request).unwrap();
        let Reach::NoWay(stuck) = &outcome.reach else {
            panic!("no way across: {outcome:?}");
        };
        assert_eq!((stuck.reached, stuck.bounds), (5 * 8, rect(0, 0, 5, 8)));
        assert_eq!(
            stuck.nearest,
            Some(Nearest {
                cell: v(4, 7),
                steps: 7
            })
        );
        assert_eq!(
            stuck.border,
            [CellsShowing {
                what: "water".into(),
                cells: 8
            }]
        );
        assert_eq!(
            outcome.report().text(),
            "from 0,7 to 11,7 through all minus where water:\n  \
             no way: from 0,7, 40 cells can be reached in 0,0 5x8, around 2,3; the nearest of them to 11,7 is 4,7, 7 steps from it counting every cell, whatever is on it\n  \
             next to them, not in --through: water ×8\n"
        );
        assert_eq!(
            from_json::<ReachOutcome>(&to_json(&outcome).unwrap()).unwrap(),
            outcome
        );
    }

    #[test]
    fn ends_outside_what_a_way_goes_through_are_noted_and_empty_ends_refused() {
        let map = water_map(|_, _| false);
        let names = Names::of(&map);
        let road = cells(&[[1, 0], [2, 0], [3, 0]]);
        let request = ReachRequest::new(cells(&[[0, 0]]), cells(&[[4, 0]])).through(road);
        let outcome = ReachOutcome::from_request(&map, &names, &request).unwrap();
        assert_eq!(outcome.ends_outside, ["0,0", "4,0"]);
        assert!(
            matches!(&outcome.reach, Reach::Way(way) if way.steps == 4 && way.on_every_way.len() == 3)
        );
        assert!(
            outcome
                .report()
                .text()
                .contains("  note: 0,0 isn't in --through: a way may start and end outside it, every cell between is in it\n")
        );
        let nowhere = ReachRequest::new(cells(&[]), cells(&[[4, 0]]));
        assert_eq!(
            ReachOutcome::from_request(&map, &names, &nowhere)
                .unwrap_err()
                .to_string(),
            ": no cells on the map"
        );
        // Without --through every cell of the map is open: no cell is on every way.
        let open = ReachRequest::new(cells(&[[0, 0]]), cells(&[[4, 4]]));
        let outcome = ReachOutcome::from_request(&map, &names, &open).unwrap();
        assert!(outcome.report().text().contains("no cell is on every way"));
    }
}
