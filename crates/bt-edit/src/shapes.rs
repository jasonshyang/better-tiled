//! The areas of Shape Fill and Rectangular Select dragged from one cell to another (Tiled's
//! `ShapeFillTool::updateFillOverlay` and `TileSelectionTool::selectedArea`,
//! `reference/tiled/src/tiled`). Lines and ellipse outlines for strokes are
//! `bt_utils::raster`'s.

use bt_utils::IRect;
use bt_utils::raster::ellipse_region;
use glam::IVec2;

use crate::region::TileRegion;

/// The shape Shape Fill paints.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillShape {
    /// A rectangle.
    #[default]
    Rectangle,
    /// The ellipse inscribed in the rectangle.
    Ellipse,
}

/// Shape Fill's area dragged from `start` to `end`, both included. With `square` both sides
/// take the smaller one (Tiled's Shift); with `from_center` the shape grows from `start` in both
/// directions (Tiled's Alt).
pub fn shape_fill_region(
    shape: FillShape,
    start: IVec2,
    end: IVec2,
    square: bool,
    from_center: bool,
) -> TileRegion {
    let mut delta = end - start;
    if square {
        let side = delta.x.abs().min(delta.y.abs());
        delta = delta.signum() * side;
    }
    let corner = if from_center { start - delta } else { start };
    let area = IRect::from_corners(corner, start + delta);
    match shape {
        FillShape::Rectangle => TileRegion::from_rect(area),
        FillShape::Ellipse => TileRegion::from_rects(ellipse_region(area)),
    }
}

/// Rectangular Select's rectangle dragged from `start` to `end`, both included. With `square`
/// both sides take the larger one (a selection grows to the pointer, where a fill shrinks to
/// it, as in Tiled); with `from_center` it grows from `start` in both directions.
pub fn rect_select_rect(start: IVec2, end: IVec2, square: bool, from_center: bool) -> IRect {
    let mut delta = end - start;
    if square {
        let side = delta.x.abs().max(delta.y.abs());
        // A zero side goes right and down, as Tiled's does.
        delta = IVec2::new(
            if delta.x < 0 { -side } else { side },
            if delta.y < 0 { -side } else { side },
        );
    }
    let corner = if from_center { start - delta } else { start };
    IRect::from_corners(corner, start + delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    #[test]
    fn a_dragged_rectangle_includes_both_ends_in_any_direction() {
        let a = IVec2::new(5, 5);
        assert_eq!(
            rect_select_rect(a, IVec2::new(2, 7), false, false),
            rect(2, 5, 4, 3)
        );
        assert_eq!(
            shape_fill_region(FillShape::Rectangle, a, IVec2::new(2, 7), false, false),
            TileRegion::from_rect(rect(2, 5, 4, 3))
        );
    }

    #[test]
    fn square_fills_shrink_and_square_selections_grow() {
        let a = IVec2::new(0, 0);
        let b = IVec2::new(-4, 2);
        assert_eq!(
            shape_fill_region(FillShape::Rectangle, a, b, true, false).bounds(),
            Some(rect(-2, 0, 3, 3))
        );
        assert_eq!(rect_select_rect(a, b, true, false), rect(-4, 0, 5, 5));
        // A straight drag: the fill is a line, the selection a square going down.
        assert_eq!(
            shape_fill_region(FillShape::Rectangle, a, IVec2::new(3, 0), true, false).len(),
            1
        );
        assert_eq!(
            rect_select_rect(a, IVec2::new(3, 0), true, false),
            rect(0, 0, 4, 4)
        );
    }

    #[test]
    fn shapes_grow_from_the_center() {
        let a = IVec2::new(10, 10);
        assert_eq!(
            rect_select_rect(a, IVec2::new(12, 11), false, true),
            rect(8, 9, 5, 3)
        );
        let ellipse = shape_fill_region(FillShape::Ellipse, a, IVec2::new(12, 12), false, true);
        assert_eq!(ellipse.bounds(), Some(rect(8, 8, 5, 5)));
        assert!(
            ellipse.contains(a) && !ellipse.contains(IVec2::new(8, 8)),
            "corners are cut"
        );
    }
}
