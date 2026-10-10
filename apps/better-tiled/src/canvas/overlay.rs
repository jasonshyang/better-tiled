//! What the canvas draws over the map: the map's border, the tile grid and the hovered cell,
//! as vector items in world pixels with strokes in device pixels.

use std::sync::Arc;

use bt_core::{Color, Map};
use bt_editor::Settings;
use bt_render::{Dash, GridStyle, VectorItem, grid_vectors};
use bt_utils::DRect;
use glam::{DVec2, IVec2};

/// What the overlays of a frame depend on.
pub(crate) struct OverlayInput<'a> {
    /// The map shown.
    pub(crate) map: &'a Map,
    /// The world rectangle in view.
    pub(crate) visible: DRect,
    /// Device pixels per world pixel.
    pub(crate) zoom_px: f64,
    /// Device pixels per screen point.
    pub(crate) pixels_per_point: f32,
    /// Whether the grid is shown.
    pub(crate) grid: bool,
    /// The cell under the pointer.
    pub(crate) hover: Option<IVec2>,
    /// The preferences (the grid's colour and major lines, the selection colour).
    pub(crate) settings: &'a Settings,
    /// The selected cells' outline, in the map's pixels on screen.
    pub(crate) selection: &'a [Vec<DVec2>],
    /// What the current tool draws.
    pub(crate) tool: &'a [VectorItem],
}

/// The dashes of the selection's outline, in device pixels.
const ANTS: Dash = Dash {
    on: 4.0,
    off: 4.0,
    phase: 0.0,
};

/// The overlays' vector items.
pub(crate) fn overlay_vectors(input: &OverlayInput<'_>) -> Arc<[VectorItem]> {
    let geometry = input.map.geometry();
    let line = input.pixels_per_point.max(1.0);
    let mut items = Vec::new();
    if input.grid {
        let major = input.settings.interface.major_grid;
        let style = GridStyle::new(input.settings.interface.grid_color.0, line).major(IVec2::new(
            i32::try_from(major.columns).unwrap_or(0),
            i32::try_from(major.rows).unwrap_or(0),
        ));
        items.extend(grid_vectors(&geometry, input.visible, input.zoom_px, style));
    }
    let map_cells = input.map.tile_bounding_rect();
    if !input.map.infinite {
        let border = with_alpha(input.settings.interface.grid_color.0, 160);
        for outline in geometry.rect_outline(map_cells) {
            items.push(VectorItem::polygon(outline).stroke(border, line));
        }
    }
    let selection = input.settings.theme.selection_color.0;
    for outline in input.selection {
        // Ants: dark dashes over a light line, visible on any tiles.
        items.push(
            VectorItem::polygon(outline.clone()).stroke(Color::rgba(255, 255, 255, 220), line),
        );
        items.push(
            VectorItem::polygon(outline.clone())
                .stroke(with_alpha(selection, 255), line)
                .dashed(ANTS),
        );
    }
    items.extend(input.tool.iter().cloned());
    if let Some(cell) = input.hover
        && (input.map.infinite || map_cells.contains(cell))
    {
        items.push(
            VectorItem::polygon(geometry.tile_polygon(cell))
                .fill(with_alpha(selection, 48))
                .stroke(with_alpha(selection, 200), line),
        );
    }
    Arc::from(items)
}

fn with_alpha(color: Color, alpha: u8) -> Color {
    Color::rgba(color.r, color.g, color.b, alpha)
}

#[cfg(test)]
mod tests {
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    fn input<'a>(map: &'a Map, settings: &'a Settings, grid: bool) -> OverlayInput<'a> {
        OverlayInput {
            map,
            visible: DRect::from_min_size(DVec2::ZERO, DVec2::splat(64.0)),
            zoom_px: 2.0,
            pixels_per_point: 2.0,
            grid,
            hover: Some(IVec2::new(1, 1)),
            settings,
            selection: &[],
            tool: &[],
        }
    }

    #[test]
    fn the_border_the_grid_and_the_hovered_cell_are_drawn() {
        let map = MapBuilder::new(UVec2::splat(16), UVec2::splat(4))
            .build()
            .unwrap();
        let settings = Settings::default();
        let plain = overlay_vectors(&input(&map, &settings, false));
        assert_eq!(plain.len(), 2, "the border and the hovered cell");
        let with_grid = overlay_vectors(&input(&map, &settings, true));
        assert!(with_grid.len() > plain.len());
        let outside = OverlayInput {
            hover: Some(IVec2::new(9, 9)),
            ..input(&map, &settings, false)
        };
        assert_eq!(
            overlay_vectors(&outside).len(),
            1,
            "no cell outside the map"
        );
        let outline = vec![vec![DVec2::ZERO, DVec2::X, DVec2::ONE]];
        let selected = OverlayInput {
            selection: &outline,
            ..input(&map, &settings, false)
        };
        assert_eq!(
            overlay_vectors(&selected).len(),
            4,
            "a selection is a light line under dark dashes"
        );
    }
}
