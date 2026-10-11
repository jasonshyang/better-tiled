//! The map's grid as the resolved JSON says it: which cells the tile layers hold (an infinite
//! map's used area and its origin cell), what they cover on screen, and where every cell's
//! centre is, from `MapGeometry`: the steps between columns and rows, and the shift of the odd
//! rows or columns of staggered and hexagonal maps. A cell's centre is the middle of its
//! outline, the point bt writes `X.5,Y.5`.

use bt_core::{ClassUsage, Map, MapGeometry, Orientation, RenderOrder, StaggerAxis, StaggerIndex};
use bt_utils::IRect;
use glam::{DVec2, IVec2};

use super::document::{Bounds, CellCentres, MapEntry};
use super::resolving::Resolving;
use super::values::{color_text, resolved_properties};
use crate::feet::point_to_pixel;

/// The cells the tile layers are written for: a finite map's, or the smallest rectangle
/// holding every tile of an infinite map's tile layers (no cells when they hold none).
pub(crate) fn written_area(map: &Map) -> IRect {
    if !map.infinite {
        return map.tile_bounding_rect();
    }
    let mut area: Option<IRect> = None;
    map.for_each_layer(|layer, _| {
        if let Some(used) = layer.as_tile_layer().and_then(|tiles| tiles.used_bounds()) {
            area = Some(area.map_or(used, |area| area.union(used)));
        }
    });
    area.unwrap_or_default()
}

/// The map's entry: its grid, the cells `area` holds, what they cover on screen and where
/// their centres are, its class and properties.
pub(crate) fn map_entry(
    resolving: &Resolving,
    area: IRect,
    name: String,
    source: String,
) -> MapEntry {
    let map = resolving.map;
    let geometry = &resolving.geometry;
    let shifted = matches!(
        map.orientation,
        Orientation::Staggered | Orientation::Hexagonal
    );
    let screen = if area.w > 0 && area.h > 0 {
        geometry.bounding_rect(area)
    } else {
        IRect::default()
    };
    MapEntry {
        name,
        source,
        orientation: orientation_word(map.orientation),
        stagger_axis: shifted.then_some(match map.stagger_axis {
            StaggerAxis::X => "x",
            StaggerAxis::Y => "y",
        }),
        stagger_index: shifted.then_some(match map.stagger_index {
            StaggerIndex::Odd => "odd",
            StaggerIndex::Even => "even",
        }),
        hex_side_length: (map.orientation == Orientation::Hexagonal).then_some(map.hex_side_length),
        render_order: match map.render_order {
            RenderOrder::RightDown => "right-down",
            RenderOrder::RightUp => "right-up",
            RenderOrder::LeftDown => "left-down",
            RenderOrder::LeftUp => "left-up",
        },
        infinite: map.infinite,
        columns: area.w,
        rows: area.h,
        origin_cell: area.min(),
        cell_size: map.tile_size(),
        screen_rect: Bounds {
            left: screen.x,
            top: screen.y,
            width: screen.w,
            height: screen.h,
        },
        cell_centres: cell_centres(geometry),
        parallax_origin: map.parallax_origin,
        background_color: map.background_color.map(color_text),
        class: map.class.clone(),
        properties: resolved_properties(
            resolving.types,
            &map.class,
            ClassUsage::MAP,
            &map.properties,
        ),
    }
}

/// The orientation's word; oblique maps are refused before any is asked for.
fn orientation_word(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Orthogonal => "orthogonal",
        Orientation::Isometric => "isometric",
        Orientation::Staggered => "staggered",
        Orientation::Hexagonal => "hexagonal",
        Orientation::Oblique => "oblique",
    }
}

/// Where the cells' centres are: the steps between them measured on a few cells. Rows (or
/// columns) two apart line up on staggered and hexagonal maps, and the one between is shifted.
fn cell_centres(geometry: &MapGeometry) -> CellCentres {
    let first = cell_centre(geometry, IVec2::ZERO);
    let from_first = |x: i32, y: i32| tidy(cell_centre(geometry, IVec2::new(x, y)) - first);
    let staggered = geometry.is_hex_grid();
    if staggered && geometry.stagger_axis == StaggerAxis::Y {
        let row_step = tidy(from_first(0, 2) / 2.0);
        CellCentres {
            first,
            column_step: from_first(1, 0),
            row_step,
            odd_rows_shift: Some(tidy(from_first(0, 1) - row_step)),
            odd_columns_shift: None,
        }
    } else if staggered {
        let column_step = tidy(from_first(2, 0) / 2.0);
        CellCentres {
            first,
            column_step,
            row_step: from_first(0, 1),
            odd_rows_shift: None,
            odd_columns_shift: Some(tidy(from_first(1, 0) - column_step)),
        }
    } else {
        CellCentres {
            first,
            column_step: from_first(1, 0),
            row_step: from_first(0, 1),
            odd_rows_shift: None,
            odd_columns_shift: None,
        }
    }
}

/// The centre of `cell` on screen: where the point `X.5,Y.5` is drawn.
fn cell_centre(geometry: &MapGeometry, cell: IVec2) -> DVec2 {
    let point = cell.as_dvec2() + DVec2::splat(0.5);
    tidy(geometry.pixel_to_screen(point_to_pixel(geometry, point)))
}

/// `point` without the noise of projecting it: each coordinate within a billionth of a
/// millionth is that millionth, so 15.999999999999998 is 16.
pub(crate) fn tidy(point: DVec2) -> DVec2 {
    let tidy_one = |c: f64| {
        let rounded = (c * 1e6).round() / 1e6;
        if (rounded - c).abs() < 1e-9 {
            rounded
        } else {
            c
        }
    };
    DVec2::new(tidy_one(point.x), tidy_one(point.y))
}

#[cfg(test)]
mod tests {
    use bt_core::Map;
    use bt_utils::DRect;
    use glam::UVec2;

    use super::*;

    /// The grids the centres are checked on: odd sizes, both stagger axes and indices, and
    /// hexagons of every proportion.
    fn grids() -> Vec<MapGeometry> {
        let geometry =
            |orientation, tile: IVec2| MapGeometry::new(orientation, tile, IVec2::new(9, 7));
        let shifted = |orientation, tile, axis, index, side| MapGeometry {
            stagger_axis: axis,
            stagger_index: index,
            hex_side_length: side,
            ..geometry(orientation, tile)
        };
        vec![
            geometry(Orientation::Orthogonal, IVec2::new(32, 16)),
            geometry(Orientation::Isometric, IVec2::new(64, 32)),
            geometry(Orientation::Isometric, IVec2::new(63, 31)),
            shifted(
                Orientation::Staggered,
                IVec2::new(64, 32),
                StaggerAxis::Y,
                StaggerIndex::Odd,
                0,
            ),
            shifted(
                Orientation::Staggered,
                IVec2::new(63, 31),
                StaggerAxis::X,
                StaggerIndex::Even,
                0,
            ),
            shifted(
                Orientation::Hexagonal,
                IVec2::new(14, 12),
                StaggerAxis::Y,
                StaggerIndex::Odd,
                6,
            ),
            shifted(
                Orientation::Hexagonal,
                IVec2::new(14, 14),
                StaggerAxis::Y,
                StaggerIndex::Even,
                7,
            ),
            shifted(
                Orientation::Hexagonal,
                IVec2::new(15, 13),
                StaggerAxis::X,
                StaggerIndex::Even,
                7,
            ),
            shifted(
                Orientation::Hexagonal,
                IVec2::new(60, 60),
                StaggerAxis::X,
                StaggerIndex::Odd,
                30,
            ),
        ]
    }

    /// The centre of `cell` as `centres` says it is.
    fn centre_from(centres: &CellCentres, cell: IVec2) -> DVec2 {
        let mut centre = centres.first
            + centres.column_step * f64::from(cell.x)
            + centres.row_step * f64::from(cell.y);
        if cell.y.rem_euclid(2) == 1 {
            centre += centres.odd_rows_shift.unwrap_or(DVec2::ZERO);
        }
        if cell.x.rem_euclid(2) == 1 {
            centre += centres.odd_columns_shift.unwrap_or(DVec2::ZERO);
        }
        centre
    }

    #[test]
    fn every_cells_centre_is_the_middle_of_its_outline_on_every_grid() {
        for geometry in grids() {
            let centres = cell_centres(&geometry);
            for cell in IRect::from_min_size(IVec2::splat(-7), IVec2::splat(17)).points() {
                let outline = DRect::from_points(geometry.tile_polygon(cell)).unwrap();
                let middle = (outline.min + outline.max) * 0.5;
                let said = centre_from(&centres, cell);
                assert!(
                    said.distance(middle) < 1e-9,
                    "{:?}: cell {cell}: {said} is not {middle}",
                    geometry.orientation
                );
                assert!(cell_centre(&geometry, cell).distance(middle) < 1e-9);
                assert_eq!(geometry.tile_at(middle), cell, "{:?}", geometry.orientation);
            }
        }
    }

    #[test]
    fn an_infinite_map_writes_the_area_its_tiles_use() {
        let mut map = Map {
            infinite: true,
            width: 4,
            height: 4,
            tile_width: 16,
            tile_height: 16,
            ..Map::default()
        };
        assert_eq!(written_area(&map), IRect::default(), "no tiles, no cells");
        map.infinite = false;
        assert_eq!(
            written_area(&map),
            IRect::from_min_size(IVec2::ZERO, UVec2::new(4, 4).as_ivec2())
        );
        assert_eq!(
            tidy(DVec2::new(15.999_999_999_999_998, 0.5)),
            DVec2::new(16.0, 0.5)
        );
        assert_eq!(
            tidy(DVec2::new(0.123_456_7, 1.0)),
            DVec2::new(0.123_456_7, 1.0)
        );
    }
}
