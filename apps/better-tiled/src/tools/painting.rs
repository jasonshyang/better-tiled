//! What the painting tools share: previewed tiles above the current layer, the outline of
//! cells, and the cells a drag passes between two points.

use bt_core::{Color, PlacedCell};
use bt_editor::TileRegion;
use bt_render::PreviewTile;
use bt_utils::raster::{Connectivity, points_on_line};
use glam::IVec2;

use super::selecting::region_preview;
use super::tool::{Overlay, ToolContext};

/// How opaque previewed tiles are.
const PREVIEW_OPACITY: f32 = 0.7;

/// Shows `cells` above the current layer, faintly, with their outline.
pub(crate) fn preview_cells(cx: &ToolContext<'_>, cells: &[PlacedCell], out: &mut Overlay) {
    let Some(layer) = cx.selection().and_then(|s| s.current_layer) else {
        return;
    };
    out.preview_layer = Some(layer);
    out.previews.extend(cells.iter().map(|placed| PreviewTile {
        at: placed.at,
        cell: placed.cell,
        opacity: PREVIEW_OPACITY,
    }));
}

/// Outlines `region` in `color`, faintly filled.
pub(crate) fn outline_cells(
    cx: &ToolContext<'_>,
    region: &TileRegion,
    color: Color,
    out: &mut Overlay,
) {
    out.vectors.extend(region_preview(cx, region, color, 1.0));
}

/// The cells from `from` to `to` as Tiled's tools pass them, without `from` (painted already).
pub(crate) fn cells_after(from: IVec2, to: IVec2) -> Vec<IVec2> {
    points_on_line(from, to, Connectivity::Eight)
        .into_iter()
        .skip(1)
        .collect()
}

/// The colour of what is erased.
pub(crate) const ERASE_COLOR: Color = Color::rgba(230, 70, 70, 255);
