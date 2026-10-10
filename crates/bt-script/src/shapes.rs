//! The cells of the shape language (`10-agents.md`, §6.1). A shape is rasterised on the map's
//! grid, a cell belonging to it when the cell's centre does, inside the map on a finite map;
//! then narrowed in the order `minus`, `on`, `inside`, `avoid`, `near`, `containing`. Shapes
//! are in tile units ([`crate::point_to_pixel`]); objects' shapes are tested on screen, where
//! they are drawn.
//!
//! A ground covers a cell when its terrain is on every lane of its set's kind on some tile
//! layer, or the cell holds one of its tiles on the layer it names (all tile layers by default),
//! or the cell is in its defined area; a terrain touches a cell when it is on any lane.

use std::sync::Arc;

use bt_core::{
    Cell, Map, MapGeometry, MapObject, Outline, Segment, TileLayer, object_outline, polygon_holds,
};
use bt_utils::hash::CellHashSet;
use bt_utils::raster::{Connectivity, points_on_line};
use bt_utils::rng::Rng;
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2};

use crate::context::Context;
use crate::error::Error;
use crate::feet::{Feet, pixel_to_point, point_to_pixel, standing_cell, to_i32};
use crate::grid::{self, sorted_cells, touching};
use crate::names::{Ground, Names};
use crate::script::{Area, Named, Shape};
use crate::words::number_text;

/// The most cells a shape may reach over: what it is rasterised in.
const MOST_CELLS: u64 = 1 << 24;

/// How many smooth waves move a rough outline.
const WAVES: usize = 5;

/// The cells of `area` on `map`, row by row and each once: its shape, narrowed. `context`
/// gives what the map does not hold: the editor's selection, `last` and the seed `rough` draws
/// from.
pub fn area_cells(
    map: &Map,
    names: &Names,
    context: &Context,
    area: &Area,
) -> Result<Vec<IVec2>, Error> {
    let drawn = Drawing::new(map, names, context);
    let mut cells = drawn.shape_cells(&area.shape)?;
    for minus in &area.minus {
        let gone: CellHashSet<IVec2> = drawn.shape_cells(minus)?.into_iter().collect();
        cells.retain(|cell| !gone.contains(cell));
    }
    let geometry = map.geometry();
    if !area.on.is_empty() {
        let grounds = drawn.probes(&area.on)?;
        cells.retain(|&cell| grounds.iter().any(|g| g.covers(cell)));
    }
    if !area.inside.is_empty() {
        let grounds = drawn.probes(&area.inside)?;
        let bounds = map_cells(map);
        let covered = |cell: IVec2| grounds.iter().any(|g| g.covers(cell));
        cells.retain(|&cell| {
            covered(cell)
                && touching(&geometry, cell)
                    .into_iter()
                    .filter(|&near| map.infinite || bounds.contains(near))
                    .all(covered)
        });
    }
    for avoid in &area.avoid {
        let grounds = drawn.probes(&avoid.grounds)?;
        let close = grid::within(&geometry, &cells, avoid.margin, |cell| {
            grounds.iter().any(|g| g.touches(cell))
        });
        cells.retain(|cell| !close.contains(cell));
    }
    for near in &area.near {
        let grounds = drawn.probes(&near.grounds)?;
        let close = grid::within(&geometry, &cells, near.margin, |cell| {
            grounds.iter().any(|g| g.touches(cell))
        });
        cells.retain(|cell| close.contains(cell));
    }
    if !area.containing.is_empty() {
        let all: CellHashSet<IVec2> = cells.iter().copied().collect();
        if let Some(&missing) = area.containing.iter().find(|cell| !all.contains(cell)) {
            return Err(Error::NotInShape {
                cell: missing,
                count: all.len(),
            });
        }
        let kept = grid::flood(&geometry, &area.containing, |cell| all.contains(&cell));
        cells.retain(|cell| kept.contains(cell));
    }
    Ok(cells)
}

/// The cells of one shape on `map`, not narrowed, row by row and each once.
pub fn shape_cells(
    map: &Map,
    names: &Names,
    context: &Context,
    shape: &Shape,
) -> Result<Vec<IVec2>, Error> {
    Drawing::new(map, names, context).shape_cells(shape)
}

/// The cells `all` stands for: a finite map's cells; on an infinite map the area painted so far,
/// or the size it was saved with while nothing is painted.
pub fn map_cells(map: &Map) -> IRect {
    if !map.infinite {
        return map.tile_bounding_rect();
    }
    let mut painted: Option<IRect> = None;
    map.for_each_layer(|layer, _| {
        if let Some(used) = layer.as_tile_layer().and_then(TileLayer::used_bounds) {
            painted = Some(painted.map_or(used, |p| p.union(used)));
        }
    });
    painted.unwrap_or_else(|| {
        IRect::from_min_size(
            IVec2::ZERO,
            IVec2::new(
                i32::try_from(map.width).unwrap_or(i32::MAX),
                i32::try_from(map.height).unwrap_or(i32::MAX),
            ),
        )
    })
}

/// The cells an object covers, as `@NAME` and `under LAYER` take them: those whose centres its
/// outline holds (a rectangle, an ellipse, a polygon, a sprite's image, as drawn), the cells
/// along a polyline, the standing cell of a point. An object too small to hold a cell's centre
/// gives its standing cell ([`Feet::standing_point`]).
pub fn object_cells(map: &Map, object: &MapObject, feet: &mut Feet) -> Result<Vec<IVec2>, Error> {
    let geometry = map.geometry();
    let tileset = object.tile.and_then(|tile| map.tileset_of(tile));
    let cells = match object_outline(&geometry, object, tileset) {
        Outline::Point(_) => vec![standing_cell(pixel_to_point(&geometry, object.pos))],
        Outline::Area(points) => cells_on_screen(&geometry, &points)?,
        Outline::Path(points) => {
            let points: Vec<DVec2> = points
                .into_iter()
                .map(|p| pixel_to_point(&geometry, geometry.screen_to_pixel(p)))
                .collect();
            path_cells(&geometry, &points, 1.0, None)?
        }
    };
    if cells.is_empty() {
        return Ok(vec![standing_cell(feet.standing_point(map, object)?)]);
    }
    Ok(cells)
}

/// What shapes are drawn on and with.
struct Drawing<'a> {
    map: &'a Map,
    names: &'a Names,
    context: &'a Context,
    geometry: MapGeometry,
    /// The map's cells; on a finite map no shape reaches outside them.
    bounds: IRect,
}

impl<'a> Drawing<'a> {
    fn new(map: &'a Map, names: &'a Names, context: &'a Context) -> Drawing<'a> {
        Drawing {
            map,
            names,
            context,
            geometry: map.geometry(),
            bounds: map_cells(map),
        }
    }

    /// Whether a cell may be in a shape: on the map, or anywhere on an infinite map.
    fn on_map(&self, cell: IVec2) -> bool {
        self.map.infinite || self.bounds.contains(cell)
    }

    /// The cells a shape is rasterised in: `reach`, cut to the map on a finite map.
    fn frame(&self, reach: IRect) -> Result<IRect, Error> {
        let frame = if self.map.infinite {
            reach
        } else {
            reach.intersection(self.bounds)
        };
        let cells = i64::from(frame.w.max(0)) * i64::from(frame.h.max(0));
        let cells = u64::try_from(cells).unwrap_or(u64::MAX);
        if cells > MOST_CELLS {
            return Err(Error::TooLarge {
                cells,
                limit: MOST_CELLS,
            });
        }
        Ok(frame)
    }

    fn shape_cells(&self, shape: &Shape) -> Result<Vec<IVec2>, Error> {
        let cells = match shape {
            Shape::Cells(cells) => cells.iter().copied().filter(|&c| self.on_map(c)).collect(),
            Shape::Rect(rect) => self.frame(*rect)?.points().collect(),
            Shape::All => self.frame(self.bounds)?.points().collect(),
            Shape::Ellipse {
                center,
                radii,
                rough,
            } => self.ellipse_cells(*center, *radii, rough.unwrap_or(0.0))?,
            Shape::Polygon(points) => self.polygon_cells(points)?,
            Shape::Path { points, width } => {
                let clip = (!self.map.infinite).then_some(self.bounds);
                path_cells(&self.geometry, points, *width, clip)?
            }
            Shape::Where(named) => self.where_cells(named)?,
            Shape::Under(layers) => {
                let mut feet = Feet::new();
                let mut cells = Vec::new();
                for layer in layers {
                    let id = self.names.object_layer(self.map, layer)?;
                    let objects = self
                        .map
                        .layer(id)
                        .and_then(|layer| layer.as_object_layer())
                        .map_or(&[][..], |layer| layer.objects.as_slice());
                    for object in objects.iter().filter(|o| o.visible) {
                        cells.extend(object_cells(self.map, object, &mut feet)?);
                    }
                }
                cells
            }
            Shape::Object(target) => {
                let mut feet = Feet::new();
                let mut cells = Vec::new();
                for id in self.names.objects(self.map, target, self.context)? {
                    if let Some(object) = self.map.object(id) {
                        cells.extend(object_cells(self.map, object, &mut feet)?);
                    }
                }
                cells
            }
            Shape::Selection => self.context.selected_cells()?.to_vec(),
        };
        Ok(sorted_cells(cells.into_iter().filter(|&c| self.on_map(c))))
    }

    /// `ellipse CX,CY RXxRY [rough R]`: the cells whose centres are inside, the radius moved
    /// around the ellipse by up to `rough` of itself.
    fn ellipse_cells(&self, center: DVec2, radii: DVec2, rough: f64) -> Result<Vec<IVec2>, Error> {
        if !(radii.x > 0.0 && radii.y > 0.0 && radii.is_finite()) {
            return Err(Error::NotPositiveRadius {
                radii: format!("{}x{}", number_text(radii.x), number_text(radii.y)),
            });
        }
        let wobble = Wobble::new(self.context.seed(), rough);
        let reach = radii * (1.0 + rough.abs()) + DVec2::ONE;
        let frame = self.frame(cell_rect(center - reach, center + reach))?;
        Ok(frame
            .points()
            .filter(|cell| {
                let offset = (cell.as_dvec2() + 0.5 - center) / radii;
                offset.length() <= wobble.radius_at(offset.y.atan2(offset.x))
            })
            .collect())
    }

    /// `poly X,Y …`: the cells whose centres are inside, by the even-odd rule.
    fn polygon_cells(&self, points: &[DVec2]) -> Result<Vec<IVec2>, Error> {
        if points.len() < 3 {
            return Err(Error::TooFewPoints {
                shape: "poly",
                needed: 3,
                found: points.len(),
            });
        }
        let frame = self.frame(points_rect(points, 0.0))?;
        Ok(frame
            .points()
            .filter(|cell| polygon_holds(points, cell.as_dvec2() + 0.5))
            .collect())
    }

    /// `where G,…`: the cells one of the grounds covers wholly.
    fn where_cells(&self, named: &[Named]) -> Result<Vec<IVec2>, Error> {
        let grounds = self.probes(named)?;
        let mut cells: Vec<IVec2> = Vec::new();
        let scanned: Vec<&Probe> = grounds
            .iter()
            .filter(|g| !matches!(g, Probe::Area(_)))
            .collect();
        if !scanned.is_empty() {
            cells.extend(
                self.frame(self.bounds)?
                    .points()
                    .filter(|&cell| scanned.iter().any(|g| g.covers(cell))),
            );
        }
        for ground in &grounds {
            if let Probe::Area(area) = ground {
                cells.extend(area.iter().copied());
            }
        }
        Ok(cells)
    }

    /// The grounds names stand for, ready to test cells with.
    fn probes(&self, named: &[Named]) -> Result<Vec<Probe<'a>>, Error> {
        let tile_layers: Vec<&'a TileLayer> = {
            let mut layers = Vec::new();
            self.map.for_each_layer(|layer, _| {
                layers.extend(layer.as_tile_layer());
            });
            layers
        };
        self.names
            .grounds(self.map, named)?
            .into_iter()
            .map(|ground| match ground {
                Ground::Terrain(named) => {
                    let terrain = bt_terrain::Terrain::new(self.map, named.slot, named.set)
                        .map_err(|error| Error::TerrainSet {
                            terrain: named.name.clone(),
                            error,
                        })?;
                    Ok(Probe::Terrain(TerrainProbe {
                        terrain,
                        color: named.color.0,
                        layers: tile_layers.clone(),
                    }))
                }
                Ground::Tiles(group) => {
                    let layers = match &group.layer {
                        Some(layer) => {
                            let id = self.names.tile_layer(self.map, layer)?;
                            self.map
                                .layer(id)
                                .and_then(|layer| layer.as_tile_layer())
                                .into_iter()
                                .collect()
                        }
                        None => tile_layers.clone(),
                    };
                    Ok(Probe::Tiles(TilesProbe {
                        tiles: group.tiles.iter().copied().collect(),
                        layers,
                    }))
                }
                Ground::Area { cells, .. } => Ok(Probe::Area(cells)),
            })
            .collect()
    }
}

/// A ground ready to test cells with.
enum Probe<'m> {
    Terrain(TerrainProbe<'m>),
    Tiles(TilesProbe<'m>),
    Area(Arc<CellHashSet<IVec2>>),
}

/// A terrain and the tile layers it is looked for on.
struct TerrainProbe<'m> {
    terrain: bt_terrain::Terrain,
    color: u8,
    layers: Vec<&'m TileLayer>,
}

/// Tiles and the tile layers they are looked for on.
struct TilesProbe<'m> {
    tiles: CellHashSet<Cell>,
    layers: Vec<&'m TileLayer>,
}

impl Probe<'_> {
    /// Whether the ground covers the whole cell.
    fn covers(&self, cell: IVec2) -> bool {
        match self {
            Probe::Terrain(probe) => probe.lanes_at(cell, |lanes| {
                lanes
                    .iter()
                    .zip(lane_use(probe.terrain.kind()))
                    .all(|(&lane, used)| !used || lane == probe.color)
            }),
            Probe::Tiles(probe) => probe.layers.iter().any(|layer| {
                let held = layer.cell(cell);
                held.slot()
                    .is_some_and(|slot| probe.tiles.contains(&Cell::new(slot, held.tile())))
            }),
            Probe::Area(cells) => cells.contains(&cell),
        }
    }

    /// Whether the ground is anywhere in the cell: a terrain on any lane.
    fn touches(&self, cell: IVec2) -> bool {
        match self {
            Probe::Terrain(probe) => probe.lanes_at(cell, |lanes| lanes.contains(&probe.color)),
            Probe::Tiles(_) | Probe::Area(_) => self.covers(cell),
        }
    }
}

impl TerrainProbe<'_> {
    /// Whether `test` holds for the cell's lanes on some tile layer.
    fn lanes_at(&self, cell: IVec2, test: impl Fn(&[u8; 8]) -> bool) -> bool {
        self.layers.iter().any(|layer| {
            let held = layer.cell(cell);
            !held.is_empty() && test(&self.terrain.wang_of_cell(held).lanes())
        })
    }
}

/// Which of the eight lanes a set of `kind` uses.
fn lane_use(kind: bt_core::WangSetType) -> [bool; 8] {
    bt_core::Lane::ALL.map(|lane| kind.uses(lane))
}

/// A radius that moves smoothly around an ellipse: one plus `rough` times a sum of sine waves
/// of seeded phases, at most `rough` either way.
struct Wobble {
    rough: f64,
    waves: [Wave; WAVES],
}

/// One sine wave around the ellipse.
#[derive(Clone, Copy)]
struct Wave {
    frequency: f64,
    amplitude: f64,
    phase: f64,
}

impl Wobble {
    fn new(seed: u64, rough: f64) -> Wobble {
        let mut rng = Rng::new(seed);
        let mut frequency = 1.0;
        let waves = [(); WAVES].map(|()| {
            frequency += 1.0;
            Wave {
                frequency,
                amplitude: 1.0 / frequency,
                phase: rng.next_f64() * std::f64::consts::TAU,
            }
        });
        Wobble { rough, waves }
    }

    /// The radius at `angle`, as a part of the ellipse's.
    fn radius_at(&self, angle: f64) -> f64 {
        if self.rough == 0.0 {
            return 1.0;
        }
        let total: f64 = self.waves.iter().map(|w| w.amplitude).sum();
        let sum: f64 = self
            .waves
            .iter()
            .map(|w| w.amplitude * (w.frequency * angle + w.phase).sin())
            .sum();
        1.0 + self.rough * sum / total
    }
}

/// `path X,Y … [width W]`: the cells whose centres are within half the width of the line, and
/// a line of cells sharing sides through the cells of its points, so a thin path never hangs
/// together by corners only; ends and bends are round. `clip` keeps it inside a finite map.
fn path_cells(
    geometry: &MapGeometry,
    points: &[DVec2],
    width: f64,
    clip: Option<IRect>,
) -> Result<Vec<IVec2>, Error> {
    let Some(&first) = points.first() else {
        return Err(Error::TooFewPoints {
            shape: "path",
            needed: 1,
            found: 0,
        });
    };
    if !(width > 0.0 && width.is_finite()) {
        return Err(Error::NotPositiveWidth {
            width: number_text(width),
        });
    }
    let half = width / 2.0;
    let segments: Vec<Segment> = if points.len() == 1 {
        vec![Segment {
            from: first,
            to: first,
        }]
    } else {
        points
            .windows(2)
            .filter_map(|pair| match pair {
                [from, to] => Some(Segment {
                    from: *from,
                    to: *to,
                }),
                _ => None,
            })
            .collect()
    };
    let reach = points_rect(points, half + 1.0);
    let frame = match clip {
        Some(bounds) => reach.intersection(bounds),
        None => reach,
    };
    let size = i64::from(frame.w.max(0)) * i64::from(frame.h.max(0));
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    if size > MOST_CELLS {
        return Err(Error::TooLarge {
            cells: size,
            limit: MOST_CELLS,
        });
    }
    let mut cells: Vec<IVec2> = frame
        .points()
        .filter(|cell| {
            let centre = cell.as_dvec2() + 0.5;
            segments.iter().any(|s| s.distance_to(centre) <= half)
        })
        .collect();
    cells.extend(side_line(geometry, points));
    let around = IRect::from_min_size(
        reach.min() - IVec2::splat(JOIN_ROOM),
        reach.size() + IVec2::splat(2 * JOIN_ROOM),
    );
    let around = match clip {
        Some(bounds) => around.intersection(bounds),
        None => around,
    };
    Ok(joined(geometry, sorted_cells(cells), around))
}

/// How far around a path the ways that join its parts may go, in cells.
const JOIN_ROOM: i32 = 3;

/// `cells` with the shortest ways between their parts added, through cells of `around`, so that
/// they hang together by sides: on grids whose rows or columns are shifted, a cell whose centre
/// is near the line can meet the line's cells at a corner only.
fn joined(geometry: &MapGeometry, cells: Vec<IVec2>, around: IRect) -> Vec<IVec2> {
    let mut parts = grid::areas(geometry, &cells).into_iter();
    let Some(largest) = parts.next() else {
        return cells;
    };
    let mut joined: CellHashSet<IVec2> = largest.into_iter().collect();
    for part in parts {
        let main: Vec<IVec2> = joined.iter().copied().collect();
        if let Some(way) = grid::way(geometry, &part, &main, |cell| around.contains(cell)) {
            joined.extend(way);
        }
        joined.extend(part);
    }
    sorted_cells(joined)
}

/// The cells of a line through the cells of `points`, each sharing a side with the next.
fn side_line(geometry: &MapGeometry, points: &[DVec2]) -> Vec<IVec2> {
    let stops: Vec<IVec2> = points
        .iter()
        .map(|p| IVec2::new(to_i32(p.x.floor()), to_i32(p.y.floor())))
        .collect();
    let mut line: Vec<IVec2> = stops.first().copied().into_iter().collect();
    for pair in stops.windows(2) {
        let [from, to] = pair else {
            continue;
        };
        for step in points_on_line(*from, *to, Connectivity::Eight)
            .into_iter()
            .skip(1)
        {
            let Some(&last) = line.last() else {
                continue;
            };
            if !grid::sides(geometry, last).any(|side| side == step) {
                // Steps between cells that meet at a corner go round by the cells between.
                let near = |cell: IVec2| (cell - last).abs().max_element() <= 3;
                if let Some(round) = grid::way(geometry, &[last], &[step], near) {
                    line.extend(round.iter().skip(1).take(round.len().saturating_sub(2)));
                }
            }
            line.push(step);
        }
    }
    line
}

/// The cells whose centres an outline on screen holds.
fn cells_on_screen(geometry: &MapGeometry, outline: &[DVec2]) -> Result<Vec<IVec2>, Error> {
    let Some(bounds) = DRect::from_points(outline.iter().copied()) else {
        return Ok(Vec::new());
    };
    let frame = geometry.tiles_in_rect(bounds);
    let size = i64::from(frame.w.max(0)) * i64::from(frame.h.max(0));
    let size = u64::try_from(size).unwrap_or(u64::MAX);
    if size > MOST_CELLS {
        return Err(Error::TooLarge {
            cells: size,
            limit: MOST_CELLS,
        });
    }
    Ok(frame
        .points()
        .filter(|&cell| {
            let centre = point_to_pixel(geometry, cell.as_dvec2() + 0.5);
            polygon_holds(outline, geometry.pixel_to_screen(centre))
        })
        .collect())
}

/// The cells from the one holding `min` to the one holding `max`.
fn cell_rect(min: DVec2, max: DVec2) -> IRect {
    IRect::from_corners(
        IVec2::new(to_i32(min.x.floor()), to_i32(min.y.floor())),
        IVec2::new(to_i32(max.x.floor()), to_i32(max.y.floor())),
    )
}

/// The cells around `points`, `margin` further out on every side.
fn points_rect(points: &[DVec2], margin: f64) -> IRect {
    let min = points.iter().fold(DVec2::INFINITY, |a, &p| a.min(p));
    let max = points.iter().fold(DVec2::NEG_INFINITY, |a, &p| a.max(p));
    cell_rect(min - margin, max + margin)
}

#[cfg(test)]
mod tests {
    use bt_core::{
        LayerId, MapObject, ObjectId, Orientation, Shape as ObjectShape, StaggerAxis, StaggerIndex,
        Tileset, WangSetType,
    };
    use bt_test_data::fixtures::{Atlas, MapBuilder, TerrainSetBuilder, combination_tile};
    use glam::UVec2;

    use super::*;
    use crate::context::Selection;
    use crate::script::{IdRange, LayerRef, Nearness, ObjectRef, TileIds};

    fn v(x: i32, y: i32) -> IVec2 {
        IVec2::new(x, y)
    }

    fn d(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn named(name: &str) -> Named {
        Named::Name(name.to_owned())
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect::from_min_size(v(x, y), v(w, h))
    }

    /// A 10×8 map of grass with water on the corners `water` says, as the terrain brush would
    /// leave them: each cell's tile has the terrains of its four corners. Layer 1 is the
    /// ground; layer 2 holds objects.
    fn terrain_map(water: impl Fn(i32, i32) -> bool, objects: Vec<MapObject>) -> Map {
        let set = TerrainSetBuilder::new(WangSetType::Corner)
            .name("Land")
            .terrains(&["Grass", "Water"])
            .every_combination();
        let atlas = Atlas::builder().name("ground").build_embedded().unwrap();
        let tileset = set.added_to(Tileset::clone(&atlas.tileset));
        let corner = |x: i32, y: i32| if water(x, y) { 2 } else { 1 };
        let rows: Vec<String> = (0..8)
            .map(|y| {
                let cells: Vec<String> = (0..10)
                    .map(|x| {
                        let tile = combination_tile(
                            2,
                            [
                                corner(x + 1, y),
                                corner(x + 1, y + 1),
                                corner(x, y + 1),
                                corner(x, y),
                            ],
                        )
                        .unwrap();
                        tile.0.to_string()
                    })
                    .collect();
                cells.join(" ")
            })
            .collect();
        MapBuilder::new(UVec2::new(16, 16), UVec2::new(10, 8))
            .tileset(Arc::new(tileset))
            .tile_layer("ground", &rows.join("\n"))
            .object_layer("things", objects)
            .build()
            .unwrap()
    }

    /// The corners of a pond: water on corners 3..=6 across and 2..=5 down, so cells 3..=5 by
    /// 2..=4 are wholly water and cells 2..=6 by 1..=5 have some.
    fn pond(x: i32, y: i32) -> bool {
        (3..=6).contains(&x) && (2..=5).contains(&y)
    }

    fn cells_of(map: &Map, area: &Area) -> Vec<IVec2> {
        area_cells(map, &Names::of(map), &Context::new(1), area).unwrap()
    }

    fn all_but(narrow: impl FnOnce(&mut Area)) -> Area {
        let mut area = Area::of(Shape::All);
        narrow(&mut area);
        area
    }

    fn nearness(name: &str, margin: u32) -> Nearness {
        Nearness {
            grounds: vec![named(name)],
            margin,
        }
    }

    #[test]
    fn cells_belong_to_a_shape_when_their_centres_do() {
        let map = terrain_map(|_, _| false, Vec::new());
        let square = Shape::Polygon(vec![d(1.0, 1.0), d(4.0, 1.0), d(4.0, 3.0), d(1.0, 3.0)]);
        assert_eq!(
            cells_of(&map, &Area::of(square)),
            rect(1, 1, 3, 2).points().collect::<Vec<_>>()
        );
        let ellipse = Shape::Ellipse {
            center: d(5.0, 5.0),
            radii: d(2.0, 1.0),
            rough: None,
        };
        assert_eq!(
            cells_of(&map, &Area::of(ellipse)),
            rect(3, 4, 4, 2).points().collect::<Vec<_>>()
        );
        // Inside the map only, and each once.
        let edge = Shape::Cells(vec![v(9, 7), v(10, 7), v(9, 7), v(-1, 0)]);
        assert_eq!(cells_of(&map, &Area::of(edge)), [v(9, 7)]);
        assert_eq!(cells_of(&map, &Area::of(Shape::All)).len(), 80);
        assert_eq!(
            cells_of(&map, &Area::of(Shape::Rect(rect(8, 6, 5, 5)))).len(),
            4
        );
        let flat = Shape::Ellipse {
            center: d(5.0, 5.0),
            radii: d(2.0, 0.0),
            rough: None,
        };
        let names = Names::of(&map);
        assert_eq!(
            area_cells(&map, &names, &Context::new(1), &Area::of(flat))
                .unwrap_err()
                .to_string(),
            "an ellipse's radii must be more than 0, and these are 2x0"
        );
    }

    #[test]
    fn rough_ellipses_wobble_by_their_seed_and_stay_near_the_ellipse() {
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(60, 60))
            .build()
            .unwrap();
        let names = Names::of(&map);
        let rough = |seed: u64| {
            let shape = Shape::Ellipse {
                center: d(30.0, 30.0),
                radii: d(20.0, 15.0),
                rough: Some(0.25),
            };
            shape_cells(&map, &names, &Context::new(seed), &shape).unwrap()
        };
        assert_eq!(rough(7), rough(7), "the same seed, the same outline");
        assert_ne!(rough(7), rough(8));
        for cell in rough(7) {
            let offset = (cell.as_dvec2() + 0.5 - d(30.0, 30.0)) / d(20.0, 15.0);
            assert!(offset.length() <= 1.25 + 1e-9, "{cell} is too far out");
        }
        let smooth = Shape::Ellipse {
            center: d(30.0, 30.0),
            radii: d(20.0, 15.0),
            rough: None,
        };
        let inner = shape_cells(&map, &names, &Context::new(7), &smooth).unwrap();
        let rough7: CellHashSet<IVec2> = rough(7).into_iter().collect();
        let near_middle = inner
            .iter()
            .filter(|c| ((c.as_dvec2() + 0.5 - d(30.0, 30.0)) / d(20.0, 15.0)).length() <= 0.75);
        assert!(near_middle.into_iter().all(|c| rough7.contains(c)));
    }

    #[test]
    fn thin_paths_hang_together_by_sides_on_every_grid() {
        let mut geometries = vec![MapGeometry::new(
            Orientation::Orthogonal,
            v(16, 16),
            v(30, 30),
        )];
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            let mut staggered = MapGeometry::new(Orientation::Staggered, v(64, 32), v(30, 30));
            staggered.stagger_axis = axis;
            staggered.stagger_index = StaggerIndex::Even;
            geometries.push(staggered);
        }
        for axis in [StaggerAxis::X, StaggerAxis::Y] {
            let mut hexagonal = MapGeometry::new(Orientation::Hexagonal, v(14, 12), v(30, 30));
            hexagonal.hex_side_length = 6;
            hexagonal.stagger_axis = axis;
            geometries.push(hexagonal);
        }
        geometries.push(MapGeometry::new(
            Orientation::Isometric,
            v(32, 16),
            v(30, 30),
        ));
        let mut rng = Rng::new(11);
        let mut coordinate = || (rng.next_f64() * 24.0 * 4.0).round() / 4.0;
        let paths: Vec<Vec<DVec2>> = (0..40)
            .map(|_| (0..3).map(|_| d(coordinate(), coordinate())).collect())
            .collect();
        for geometry in geometries {
            for points in &paths {
                for width in [0.5, 1.0, 1.5, 2.0] {
                    let cells = path_cells(&geometry, points, width, None).unwrap();
                    assert_eq!(
                        grid::areas(&geometry, &cells).len(),
                        1,
                        "{:?} {:?} {points:?} width {width}: {cells:?}",
                        geometry.orientation,
                        geometry.stagger_axis
                    );
                }
            }
        }
        let square = MapGeometry::new(Orientation::Orthogonal, v(16, 16), v(30, 30));
        let wide = path_cells(&square, &[d(0.0, 5.5), d(10.0, 5.5)], 3.0, None).unwrap();
        assert!(wide.contains(&v(5, 4)) && wide.contains(&v(5, 6)) && !wide.contains(&v(5, 7)));
        // Round ends: the corner cell beyond the end of a wide path is left out.
        assert!(wide.contains(&v(10, 5)) && !wide.contains(&v(11, 4)));
        assert!(matches!(
            path_cells(&square, &[d(1.0, 1.0)], 0.0, None),
            Err(Error::NotPositiveWidth { .. })
        ));
    }

    #[test]
    fn grounds_cover_cells_wholly_and_touch_their_edges() {
        let map = terrain_map(pond, Vec::new());
        let water = cells_of(&map, &Area::of(Shape::Where(vec![named("water")])));
        assert_eq!(water, rect(3, 2, 3, 3).points().collect::<Vec<_>>());
        assert_eq!(
            cells_of(&map, &all_but(|a| a.on = vec![named("water")])),
            water
        );
        // Inside: only the cell whose every neighbour is water too.
        assert_eq!(
            cells_of(&map, &all_but(|a| a.inside = vec![named("water")])),
            [v(4, 3)]
        );
        // Avoid: without a margin, the cells water touches go; with one, a ring more.
        assert_eq!(
            cells_of(&map, &all_but(|a| a.avoid = vec![nearness("water", 0)])).len(),
            80 - 25
        );
        assert_eq!(
            cells_of(&map, &all_but(|a| a.avoid = vec![nearness("water", 1)])).len(),
            80 - 49
        );
        let near = cells_of(&map, &all_but(|a| a.near = vec![nearness("water", 1)]));
        assert_eq!(near, rect(1, 0, 7, 7).points().collect::<Vec<_>>());
        // Tiles by id are a ground too: the grass tile is tile 0.
        let grass = Named::Tiles(TileIds {
            tileset: "ground".into(),
            ranges: vec![IdRange { first: 0, last: 0 }],
        });
        assert_eq!(
            cells_of(&map, &Area::of(Shape::Where(vec![grass]))).len(),
            80 - 25
        );
        // A defined area is a ground; minus takes shapes out.
        let mut names = Names::of(&map);
        names
            .define_area("corner", rect(0, 0, 2, 2).points())
            .unwrap();
        let area = Area {
            minus: vec![Shape::Cells(vec![v(0, 0)])],
            ..Area::of(Shape::Where(vec![named("corner")]))
        };
        assert_eq!(
            area_cells(&map, &names, &Context::new(1), &area).unwrap(),
            [v(1, 0), v(0, 1), v(1, 1)]
        );
    }

    #[test]
    fn inside_counts_the_maps_edge_as_no_constraint() {
        let map = terrain_map(|x, y| x <= 3 && y <= 3, Vec::new());
        let inside = cells_of(&map, &all_but(|a| a.inside = vec![named("water")]));
        assert_eq!(inside, rect(0, 0, 2, 2).points().collect::<Vec<_>>());
    }

    #[test]
    fn containing_keeps_the_connected_parts_holding_the_cells() {
        let map = terrain_map(
            |x, y| (x <= 2 && y <= 2) || ((6..=9).contains(&x) && (4..=7).contains(&y)),
            Vec::new(),
        );
        let ponds = Area::of(Shape::Where(vec![named("water")]));
        assert_eq!(cells_of(&map, &ponds).len(), 4 + 9);
        let one = Area {
            containing: vec![v(7, 5)],
            ..ponds.clone()
        };
        assert_eq!(
            cells_of(&map, &one),
            rect(6, 4, 3, 3).points().collect::<Vec<_>>()
        );
        let names = Names::of(&map);
        let wrong = Area {
            containing: vec![v(4, 4)],
            ..ponds
        };
        assert_eq!(
            area_cells(&map, &names, &Context::new(1), &wrong)
                .unwrap_err()
                .to_string(),
            "containing 4,4: that cell isn't one of the shape's cells (13 of them, before `containing`)"
        );
    }

    #[test]
    fn objects_cover_the_cells_their_shapes_hold() {
        let zone = MapObject {
            name: "zone".into(),
            pos: d(16.0, 16.0),
            size: d(48.0, 32.0),
            ..MapObject::default()
        };
        let spot = MapObject {
            name: "spot".into(),
            shape: ObjectShape::Point,
            pos: d(8.0 * 16.0 + 8.0, 6.0 * 16.0),
            ..MapObject::default()
        };
        let hidden = MapObject {
            visible: false,
            pos: d(0.0, 0.0),
            size: d(160.0, 16.0),
            ..MapObject::default()
        };
        let map = terrain_map(|_, _| false, vec![zone, spot, hidden]);
        let at = |name: &str| Area::of(Shape::Object(ObjectRef::Name(name.into())));
        assert_eq!(
            cells_of(&map, &at("zone")),
            rect(1, 1, 3, 2).points().collect::<Vec<_>>()
        );
        assert_eq!(
            cells_of(&map, &at("spot")),
            [v(8, 5)],
            "on the bottom edge of 8,5"
        );
        let under = Area::of(Shape::Under(vec![LayerRef::Name("things".into())]));
        assert_eq!(
            cells_of(&map, &under).len(),
            6 + 1,
            "the hidden one is left out"
        );
        let names = Names::of(&map);
        let wrong = Area::of(Shape::Under(vec![LayerRef::Name("ground".into())]));
        assert_eq!(
            area_cells(&map, &names, &Context::new(1), &wrong)
                .unwrap_err()
                .to_string(),
            "ground is a tile layer, not an object layer"
        );
        let picked = Context::new(1).with_selection(Selection {
            cells: vec![v(2, 2), v(30, 30)],
            objects: vec![ObjectId(2)],
        });
        let selection = Area::of(Shape::Selection);
        assert_eq!(
            area_cells(&map, &names, &picked, &selection).unwrap(),
            [v(2, 2)]
        );
        let selected = Area::of(Shape::Object(ObjectRef::Selected));
        assert_eq!(
            area_cells(&map, &names, &picked, &selected).unwrap(),
            [v(8, 5)]
        );
        assert!(matches!(
            area_cells(&map, &names, &Context::new(1), &selection),
            Err(Error::NotInEditor { .. })
        ));
        assert_eq!(map.layers[1].id, LayerId(2));
    }

    #[test]
    fn all_on_an_infinite_map_is_what_is_painted_or_its_saved_size() {
        let empty = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
            .infinite()
            .tile_layer("ground", "")
            .build()
            .unwrap();
        assert_eq!(map_cells(&empty), rect(0, 0, 12, 8));
        let atlas = Atlas::builder().build_embedded().unwrap();
        let painted = MapBuilder::new(UVec2::new(16, 16), UVec2::new(12, 8))
            .infinite()
            .tileset(atlas.tileset)
            .tile_layer_at("ground", v(-40, 90), "1 2\n3 .")
            .build()
            .unwrap();
        assert_eq!(map_cells(&painted), rect(-40, 90, 2, 2));
        assert_eq!(cells_of(&painted, &Area::of(Shape::All)).len(), 4);
        // Shapes are not cut to what is painted.
        assert_eq!(
            cells_of(&painted, &Area::of(Shape::Rect(rect(100, 100, 2, 2)))).len(),
            4
        );
        let huge = Area::of(Shape::Rect(rect(0, 0, 100_000, 100_000)));
        assert!(matches!(
            area_cells(&painted, &Names::of(&painted), &Context::new(1), &huge),
            Err(Error::TooLarge { .. })
        ));
    }
}
