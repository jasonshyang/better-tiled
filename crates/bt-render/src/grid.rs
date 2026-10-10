//! The tile grid as Tiled draws it: dashed cosmetic lines that fade out as the cells get small
//! on screen, major lines darker.

use bt_core::{Color, MapGeometry};
use bt_utils::DRect;
use glam::IVec2;

use crate::vector::{Dash, VectorItem};

/// How the grid is drawn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridStyle {
    color: Color,
    major: IVec2,
    pixel_ratio: f32,
}

impl GridStyle {
    /// Lines in `color` (its alpha is Tiled's, set by the zoom); `pixel_ratio` device pixels a
    /// point, so the dashes are as long on every screen.
    pub fn new(color: Color, pixel_ratio: f32) -> GridStyle {
        GridStyle {
            color,
            major: IVec2::ZERO,
            pixel_ratio: pixel_ratio.max(1.0),
        }
    }

    /// Every `major.x`-th column and `major.y`-th row is major (0 for none).
    pub fn major(mut self, major: IVec2) -> GridStyle {
        self.major = major;
        self
    }
}

/// The grid lines in `visible` (world pixels) at `zoom` device pixels per world pixel.
pub fn grid_vectors(
    geometry: &MapGeometry,
    visible: DRect,
    zoom: f64,
    style: GridStyle,
) -> Vec<VectorItem> {
    let scale = zoom / f64::from(style.pixel_ratio);
    let fade = |size: f64| ((size * scale - 3.0) / 17.0).clamp(0.0, 1.0);
    let grid_size = f64::from(geometry.grid_fade_size());
    let minor_alpha = 96.0 * fade(grid_size);
    let major_size = f64::from(style.major.max_element().max(0)) * grid_size;
    let major_alpha = minor_alpha + 96.0 * fade(major_size);
    if major_alpha < 1.0 {
        return Vec::new();
    }
    let with_alpha = |alpha: f64| {
        Color::rgba(
            style.color.r,
            style.color.g,
            style.color.b,
            alpha.round() as u8,
        )
    };
    let dash = Dash {
        on: 2.0 * style.pixel_ratio,
        off: 2.0 * style.pixel_ratio,
        phase: 0.0,
    };
    geometry
        .grid_lines(visible, style.major)
        .into_iter()
        .filter(|line| line.major || minor_alpha >= 1.0)
        .map(|line| {
            let alpha = if line.major { major_alpha } else { minor_alpha };
            VectorItem::polyline(vec![line.from, line.to])
                .stroke(with_alpha(alpha), style.pixel_ratio)
                .dashed(dash)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bt_core::Orientation;
    use glam::DVec2;

    use super::*;

    #[test]
    fn the_grid_fades_with_the_zoom() {
        let geometry = MapGeometry::new(
            Orientation::Orthogonal,
            IVec2::new(16, 16),
            IVec2::new(4, 4),
        );
        let visible = DRect::from_min_size(DVec2::ZERO, DVec2::splat(64.0));
        let style = GridStyle::new(Color::BLACK, 1.0);
        let near = grid_vectors(&geometry, visible, 2.0, style);
        assert!(!near.is_empty());
        assert_eq!(near[0].stroke.unwrap().color.a, 96);
        // 16 px cells at 1/8 zoom are 2 px: gone.
        assert!(grid_vectors(&geometry, visible, 0.125, style).is_empty());
        let half = grid_vectors(&geometry, visible, 0.75, style);
        assert!(half[0].stroke.unwrap().color.a < 96);
    }
}
