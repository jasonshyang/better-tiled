//! The brush's geometry: the target under the pointer, the targets a drag paints and the targets
//! of a line, as Tiled's `WangBrush` finds them (`reference/tiled/src/tiled/wangbrush.cpp`,
//! `mouseMoved`, `updateBrush`). Positions are map pixels with the layer's offset taken off.

use bt_core::{Map, WangSetType};
use bt_utils::raster::{Connectivity, points_on_line};
use glam::{DVec2, IVec2};

use crate::request::BrushTarget;
use crate::topology::Topology;
use crate::wangid::{is_corner, lane_by_grid, lane_of};

/// Where the brush is: the point painting happens around (a vertex's cell, or a cell) and the
/// lane there, `None` at the centre of a mixed set's tile.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Hover {
    point: IVec2,
    lane: Option<usize>,
}

/// The target under the pointer at `position` for a brush painting a terrain the set paints as
/// `kind` ([`crate::Terrain::brush_kind`]), or whole tiles with `full_tile`: a corner set snaps
/// to the nearest vertex, an edge set picks the triangle of the tile split along both diagonals,
/// and a mixed set uses a 3×3 grid whose corners snap to their vertex, whose sides are edges, and
/// whose centre is nothing.
pub fn hit_test(
    map: &Map,
    position: DVec2,
    full_tile: bool,
    kind: WangSetType,
) -> Option<BrushTarget> {
    let stroke = Stroke::new(map, kind, full_tile);
    let hover = stroke.locate(position, false);
    target_of(hover, kind, full_tile)
}

/// The targets of a line from one target's point to another's (a Shift stroke from the last
/// painted point): on corner and edge sets the vertices or edges along a four-connected line, the
/// edges following the line's steps; on mixed sets, and in full-tile mode, whole tiles along an
/// eight-connected line, as in Tiled.
pub fn line_targets(
    from: BrushTarget,
    to: BrushTarget,
    full_tile: bool,
    kind: WangSetType,
) -> Vec<BrushTarget> {
    let tiles = full_tile || kind == WangSetType::Mixed;
    let connectivity = if tiles {
        Connectivity::Eight
    } else {
        Connectivity::Four
    };
    let points = points_on_line(from.point(), to.point(), connectivity);
    if tiles {
        return points.into_iter().map(BrushTarget::Tile).collect();
    }
    match kind {
        WangSetType::Edge if points.len() > 1 => steps_as_edges(&points),
        WangSetType::Edge => vec![to],
        _ => points.into_iter().map(BrushTarget::Corner).collect(),
    }
}

/// A drag of the terrain brush (Tiled's `WangBrush` while painting): [`Stroke::press`] paints
/// the target under the pointer, and each [`Stroke::drag_to`] the targets since the previous
/// event. On corner sets the vertices along the way are four-connected; on edge sets the painted
/// edge follows the drag, so a drag draws a path through tile centres, and staying inside a tile
/// keeps the current edge; on mixed sets only the target under the pointer is painted; whole
/// tiles are eight-connected.
#[derive(Clone, Debug)]
pub struct Stroke {
    topology: Topology,
    kind: WangSetType,
    full_tile: bool,
    hover: Option<Hover>,
}

impl Stroke {
    /// A stroke of a terrain the set paints as `kind`, or of whole tiles with `full_tile`.
    pub fn new(map: &Map, kind: WangSetType, full_tile: bool) -> Stroke {
        Stroke {
            topology: Topology::of(map),
            kind,
            full_tile,
            hover: None,
        }
    }

    /// The pointer goes down at `position`: the target under it, if there is one.
    pub fn press(&mut self, position: DVec2) -> Vec<BrushTarget> {
        let hover = self.locate(position, false);
        self.hover = Some(hover);
        target_of(hover, self.kind, self.full_tile)
            .into_iter()
            .collect()
    }

    /// The pointer moves to `position`: the targets to paint since the last event, none when
    /// the brush has not moved.
    pub fn drag_to(&mut self, position: DVec2) -> Vec<BrushTarget> {
        let Some(previous) = self.hover else {
            return self.press(position);
        };
        let next = self.locate(position, true);
        if next == previous {
            return Vec::new();
        }
        self.hover = Some(next);
        let along = self.full_tile || self.kind != WangSetType::Mixed;
        if !along {
            return target_of(next, self.kind, self.full_tile)
                .into_iter()
                .collect();
        }
        let connectivity = if self.full_tile {
            Connectivity::Eight
        } else {
            Connectivity::Four
        };
        let points = points_on_line(previous.point, next.point, connectivity);
        if points.len() > 1 && self.kind == WangSetType::Edge && !self.full_tile {
            let targets = steps_as_edges(&points);
            if let Some(BrushTarget::Edge { side, .. }) = targets.last() {
                self.hover = Some(Hover {
                    point: next.point,
                    lane: Some(side.index()),
                });
            }
            return targets;
        }
        // The first point was painted by the event before.
        let skip = usize::from(points.len() > 1);
        points
            .into_iter()
            .skip(skip)
            .filter_map(|point| target_of(Hover { point, ..next }, self.kind, self.full_tile))
            .collect()
    }

    /// Where the brush is for a pointer at `position` (Tiled's `mouseMoved`); while `painting`
    /// on an edge set, a move along a row or column keeps or turns the current edge.
    fn locate(&self, position: DVec2, painting: bool) -> Hover {
        let geometry = self.topology.geometry();
        let in_tiles = geometry.screen_to_tile(position);
        let mut tile = in_tiles.floor().as_ivec2();
        if self.full_tile {
            return Hover {
                point: tile,
                lane: None,
            };
        }
        let local = in_tiles - tile.as_dvec2();
        let grid = |v: f64| ((v * 3.0).floor() as i32).clamp(0, 2);
        let mut lane = lane_by_grid(grid(local.x), grid(local.y));
        let hex = self.topology.is_hex();
        match self.kind {
            WangSetType::Corner => {
                if hex {
                    if local.x >= 0.5 {
                        tile = geometry.bottom_right(tile);
                    }
                    if local.y >= 0.5 {
                        tile = geometry.bottom_left(tile);
                    }
                } else {
                    tile += IVec2::new(i32::from(local.x >= 0.5), i32::from(local.y >= 0.5));
                }
                lane = Some(7);
            }
            WangSetType::Edge => {
                let current = self
                    .hover
                    .filter(|h| painting && (tile.x == h.point.x || tile.y == h.point.y));
                lane = Some(match current {
                    Some(h) if tile.x > h.point.x => 6,
                    Some(h) if tile.x < h.point.x => 2,
                    Some(h) if tile.y > h.point.y => 0,
                    Some(h) if tile.y < h.point.y => 4,
                    Some(h) => h.lane.unwrap_or(0),
                    None if local.y > local.x && local.y > 1.0 - local.x => 4,
                    None if local.y > local.x => 6,
                    None if local.y > 1.0 - local.x => 2,
                    None => 0,
                });
            }
            WangSetType::Mixed => match lane {
                Some(3) => {
                    tile = if hex {
                        geometry.bottom_left(geometry.bottom_right(tile))
                    } else {
                        tile + IVec2::ONE
                    };
                    lane = Some(7);
                }
                Some(5) => {
                    tile = if hex {
                        geometry.bottom_left(tile)
                    } else {
                        tile + IVec2::Y
                    };
                    lane = Some(7);
                }
                Some(1) => {
                    tile = if hex {
                        geometry.bottom_right(tile)
                    } else {
                        tile + IVec2::X
                    };
                    lane = Some(7);
                }
                _ => {}
            },
        }
        Hover { point: tile, lane }
    }
}

/// The target a hover stands for.
fn target_of(hover: Hover, kind: WangSetType, full_tile: bool) -> Option<BrushTarget> {
    if full_tile {
        return Some(BrushTarget::Tile(hover.point));
    }
    let lane = hover.lane?;
    if kind == WangSetType::Corner || is_corner(lane) {
        Some(BrushTarget::Corner(hover.point))
    } else {
        Some(BrushTarget::Edge {
            cell: hover.point,
            side: lane_of(lane),
        })
    }
}

/// The edges a path of cells crosses: each step's edge on the cell it enters.
fn steps_as_edges(points: &[IVec2]) -> Vec<BrushTarget> {
    points
        .windows(2)
        .filter_map(|pair| {
            let [from, to] = pair else { return None };
            let lane = if to.x > from.x {
                6
            } else if to.x < from.x {
                2
            } else if to.y > from.y {
                0
            } else {
                4
            };
            Some(BrushTarget::Edge {
                cell: *to,
                side: lane_of(lane),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::{Lane, Orientation, StaggerAxis, StaggerIndex};
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    fn square() -> Map {
        MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .build()
            .unwrap()
    }

    fn at(x: f64, y: f64) -> DVec2 {
        DVec2::new(x * 16.0, y * 16.0)
    }

    #[test]
    fn corner_sets_snap_to_the_nearest_vertex() {
        let map = square();
        let hit = |x, y| hit_test(&map, at(x, y), false, WangSetType::Corner);
        assert_eq!(hit(2.2, 3.3), Some(BrushTarget::Corner(IVec2::new(2, 3))));
        assert_eq!(hit(2.7, 3.3), Some(BrushTarget::Corner(IVec2::new(3, 3))));
        assert_eq!(hit(2.7, 3.6), Some(BrushTarget::Corner(IVec2::new(3, 4))));
        assert_eq!(
            hit_test(&map, at(2.7, 3.6), true, WangSetType::Corner),
            Some(BrushTarget::Tile(IVec2::new(2, 3)))
        );
    }

    #[test]
    fn edge_sets_pick_the_triangle_and_mixed_sets_the_grid() {
        let map = square();
        let edge = |x, y| hit_test(&map, at(x, y), false, WangSetType::Edge);
        let side = |cell: IVec2, side| Some(BrushTarget::Edge { cell, side });
        assert_eq!(edge(2.5, 3.1), side(IVec2::new(2, 3), Lane::Top));
        assert_eq!(edge(2.9, 3.5), side(IVec2::new(2, 3), Lane::Right));
        assert_eq!(edge(2.5, 3.9), side(IVec2::new(2, 3), Lane::Bottom));
        assert_eq!(edge(2.1, 3.5), side(IVec2::new(2, 3), Lane::Left));
        let mixed = |x, y| hit_test(&map, at(x, y), false, WangSetType::Mixed);
        assert_eq!(mixed(2.5, 3.5), None, "the centre does nothing");
        assert_eq!(mixed(2.5, 3.1), side(IVec2::new(2, 3), Lane::Top));
        assert_eq!(mixed(2.9, 3.9), Some(BrushTarget::Corner(IVec2::new(3, 4))));
        assert_eq!(mixed(2.1, 3.1), Some(BrushTarget::Corner(IVec2::new(2, 3))));
    }

    #[test]
    fn a_corner_drag_paints_each_vertex_on_the_way_four_connected() {
        let map = square();
        let mut stroke = Stroke::new(&map, WangSetType::Corner, false);
        assert_eq!(
            stroke.press(at(1.1, 1.1)),
            [BrushTarget::Corner(IVec2::new(1, 1))]
        );
        assert!(stroke.drag_to(at(1.2, 1.2)).is_empty(), "the same vertex");
        let targets = stroke.drag_to(at(3.1, 2.1));
        assert_eq!(
            targets,
            [IVec2::new(2, 1), IVec2::new(2, 2), IVec2::new(3, 2)].map(BrushTarget::Corner)
        );
    }

    #[test]
    fn an_edge_drag_follows_the_pointer_through_tile_centres() {
        let map = square();
        let mut stroke = Stroke::new(&map, WangSetType::Edge, false);
        let first = stroke.press(at(1.5, 1.1));
        assert_eq!(
            first,
            [BrushTarget::Edge {
                cell: IVec2::ONE,
                side: Lane::Top
            }]
        );
        // Staying inside the tile keeps the edge; moving right paints the edge crossed.
        assert!(stroke.drag_to(at(1.9, 1.5)).is_empty());
        assert_eq!(
            stroke.drag_to(at(2.5, 1.5)),
            [BrushTarget::Edge {
                cell: IVec2::new(2, 1),
                side: Lane::Left
            }]
        );
        // Down two tiles at once: the edges of each step.
        assert_eq!(
            stroke.drag_to(at(2.5, 3.5)),
            [
                BrushTarget::Edge {
                    cell: IVec2::new(2, 2),
                    side: Lane::Top
                },
                BrushTarget::Edge {
                    cell: IVec2::new(2, 3),
                    side: Lane::Top
                },
            ]
        );
    }

    #[test]
    fn whole_tile_drags_and_mixed_lines_are_eight_connected() {
        let map = square();
        let mut stroke = Stroke::new(&map, WangSetType::Corner, true);
        stroke.press(at(0.5, 0.5));
        assert_eq!(
            stroke.drag_to(at(2.5, 2.5)),
            [IVec2::ONE, IVec2::splat(2)].map(BrushTarget::Tile)
        );
        let line = line_targets(
            BrushTarget::Corner(IVec2::ZERO),
            BrushTarget::Corner(IVec2::new(2, 1)),
            false,
            WangSetType::Mixed,
        );
        assert_eq!(line.len(), 3);
        assert!(line.iter().all(|t| matches!(t, BrushTarget::Tile(_))));
        let corners = line_targets(
            BrushTarget::Corner(IVec2::ZERO),
            BrushTarget::Corner(IVec2::new(2, 1)),
            false,
            WangSetType::Corner,
        );
        assert_eq!(corners.len(), 4, "four-connected, the first included");
    }

    #[test]
    fn staggered_maps_snap_corners_across_the_diamond() {
        let map = MapBuilder::new(UVec2::new(32, 16), UVec2::splat(6))
            .orientation(Orientation::Staggered)
            .stagger(StaggerAxis::Y, StaggerIndex::Odd)
            .build()
            .unwrap();
        // Near the right point of the diamond of (2, 2): one of its own corners.
        let polygon = map.geometry().tile_polygon(IVec2::new(2, 2));
        let centre = polygon.iter().copied().sum::<DVec2>() / polygon.len() as f64;
        let right = polygon
            .iter()
            .copied()
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        let hit = hit_test(
            &map,
            centre + (right - centre) * 0.8,
            false,
            WangSetType::Corner,
        )
        .unwrap();
        assert!(hit.cells(&map).contains(&IVec2::new(2, 2)), "{hit:?}");
    }
}
