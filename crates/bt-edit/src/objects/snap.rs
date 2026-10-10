//! Snapping object positions (Tiled's `SnapHelper`, `reference/tiled/src/tiled/snaphelper.cpp`,
//! and how `ObjectSelectionTool` snaps a moved selection by its top-left-most position,
//! `objectselectiontool.cpp`). Every grid goes through `MapGeometry::snap_to_grid`, so snapping
//! works on every orientation.

use bt_core::{Map, MapGeometry, ObjectId, ObjectLayer};
use glam::DVec2;

use crate::error::EditError;

use super::find::{objects_by_id, objects_in};

/// How object positions snap. The caller decides what the keys mean (Tiled's Ctrl toggles grid
/// snapping and Ctrl+Shift the fine grid).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum SnapMode {
    /// Positions stay where they are put.
    #[default]
    None,
    /// To the grid: cells' corners (also their centres on hexagonal and staggered grids, as
    /// `MapGeometry::snap_to_grid` decides).
    Grid,
    /// To a finer grid of this many divisions per cell.
    FineGrid(u16),
    /// To whole screen pixels.
    Pixels,
}

/// `point` (pixel space) snapped by `mode`.
pub fn snap(geometry: &MapGeometry, point: DVec2, mode: SnapMode) -> DVec2 {
    match mode {
        SnapMode::None => point,
        SnapMode::Grid => geometry.snap_to_grid(point, 1),
        SnapMode::FineGrid(divisions) => geometry.snap_to_grid(point, i32::from(divisions)),
        SnapMode::Pixels => geometry.screen_to_pixel(geometry.pixel_to_screen(point).round()),
    }
}

/// How far a selection dragged by `delta` moves: its anchor (see [`move_anchor`]) is snapped
/// where the drag takes it and everything moves as far, so objects keep where they are against
/// each other.
pub fn snap_delta(geometry: &MapGeometry, anchor: DVec2, delta: DVec2, mode: SnapMode) -> DVec2 {
    match mode {
        SnapMode::None => delta,
        SnapMode::Grid | SnapMode::FineGrid(_) | SnapMode::Pixels => {
            snap(geometry, anchor + delta, mode) - anchor
        }
    }
}

/// The point a moved selection of `map` snaps by: the least x and the least y of the objects'
/// positions (Tiled's `mAlignPosition`). `None` for no ids.
pub fn move_anchor(map: &Map, ids: &[ObjectId]) -> Result<Option<DVec2>, EditError> {
    Ok(objects_by_id(map, ids)?
        .into_iter()
        .map(|o| o.pos)
        .reduce(DVec2::min))
}

/// The point a moved selection of a list snaps by.
pub fn move_anchor_in(objects: &ObjectLayer, ids: &[ObjectId]) -> Result<Option<DVec2>, EditError> {
    Ok(objects_in(objects, ids)?
        .into_iter()
        .map(|o| o.pos)
        .reduce(DVec2::min))
}

#[cfg(test)]
mod tests {
    use bt_core::{Orientation, StaggerAxis, StaggerIndex};
    use bt_test_data::fixtures::MapBuilder;
    use glam::{IVec2, UVec2};

    use super::*;
    use crate::testutil::{assert_near, rect_object};

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn positions_snap_to_every_grid() {
        let square = MapGeometry::new(Orientation::Orthogonal, IVec2::splat(16), IVec2::splat(8));
        assert_eq!(snap(&square, v(23.0, 9.0), SnapMode::None), v(23.0, 9.0));
        assert_eq!(snap(&square, v(23.0, 9.0), SnapMode::Grid), v(16.0, 16.0));
        assert_eq!(
            snap(&square, v(23.0, 9.0), SnapMode::FineGrid(4)),
            v(24.0, 8.0)
        );
        assert_eq!(snap(&square, v(23.4, 9.5), SnapMode::Pixels), v(23.0, 10.0));
        // Isometric pixel space measures both axes in tile heights.
        let diamond = MapGeometry::new(Orientation::Isometric, IVec2::new(64, 32), IVec2::splat(8));
        assert_eq!(snap(&diamond, v(40.0, 20.0), SnapMode::Grid), v(32.0, 32.0));
        assert_eq!(
            snap(&diamond, v(40.0, 20.0), SnapMode::FineGrid(4)),
            v(40.0, 24.0)
        );
        let pixel = snap(&diamond, v(10.3, 4.1), SnapMode::Pixels);
        let screen = diamond.pixel_to_screen(pixel);
        assert_near(screen, screen.round());
        // Hexagonal grids snap to cell corners, and to centres on a fine grid.
        let mut hex = MapGeometry::new(Orientation::Hexagonal, IVec2::new(32, 32), IVec2::splat(8));
        hex.hex_side_length = 16;
        hex.stagger_axis = StaggerAxis::Y;
        hex.stagger_index = StaggerIndex::Odd;
        let corner = snap(&hex, v(2.0, 9.0), SnapMode::Grid);
        assert_eq!(corner, v(0.0, 8.0));
        assert_eq!(
            snap(&hex, v(14.0, 15.0), SnapMode::FineGrid(2)),
            v(16.0, 16.0)
        );
    }

    #[test]
    fn a_moved_selection_snaps_by_its_anchor() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(8))
            .object_layer(
                "things",
                vec![
                    rect_object(5.0, 30.0, 4.0, 4.0),
                    rect_object(20.0, 3.0, 4.0, 4.0),
                ],
            )
            .build()
            .unwrap();
        let anchor = move_anchor(&map, &[ObjectId(1), ObjectId(2)])
            .unwrap()
            .unwrap();
        assert_eq!(anchor, v(5.0, 3.0));
        let geometry = map.geometry();
        assert_eq!(
            snap_delta(&geometry, anchor, v(10.0, 10.0), SnapMode::Grid),
            v(11.0, 13.0),
            "the anchor lands on (16, 16)"
        );
        assert_eq!(
            snap_delta(&geometry, anchor, v(10.0, 10.0), SnapMode::None),
            v(10.0, 10.0)
        );
        assert_eq!(move_anchor(&map, &[]), Ok(None));
        let layer = map.layers[0].as_object_layer().unwrap();
        assert_eq!(
            move_anchor_in(layer, &[ObjectId(2)]),
            Ok(Some(v(20.0, 3.0)))
        );
    }
}
