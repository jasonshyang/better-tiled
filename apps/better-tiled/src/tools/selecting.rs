//! What the selection tools share: the mode the modifiers at a press choose, and how a region
//! of cells is drawn as a preview.

use bt_core::{Color, MapGeometry};
use bt_editor::{SelectionMode, TileRegion};
use bt_render::VectorItem;
use egui::Modifiers;
use glam::DVec2;

/// The mode a press with `modifiers` selects in, as in Tiled: Shift adds, Command subtracts,
/// both intersect; otherwise the toolbar's mode.
pub(crate) fn mode_at_press(modifiers: Modifiers, default: SelectionMode) -> SelectionMode {
    match (modifiers.shift, modifiers.command) {
        (true, true) => SelectionMode::Intersect,
        (true, false) => SelectionMode::Add,
        (false, true) => SelectionMode::Subtract,
        (false, false) => default,
    }
}

/// The outline of a region on screen, as closed loops in the map's pixels: from its runs on
/// grids of squares and diamonds, cell by cell on hexagonal and staggered grids.
pub(crate) fn region_polygons(geometry: &MapGeometry, region: &TileRegion) -> Vec<Vec<DVec2>> {
    if geometry.is_hex_grid() {
        return geometry.region_outline(region.iter());
    }
    region
        .outline()
        .into_iter()
        .map(|points| {
            points
                .into_iter()
                .map(|p| geometry.tile_to_screen(p.as_dvec2()))
                .collect()
        })
        .collect()
}

/// A region drawn as a selection preview: filled faintly and outlined.
pub(crate) fn region_preview(
    geometry: &MapGeometry,
    region: &TileRegion,
    color: Color,
    line: f32,
) -> Vec<VectorItem> {
    region_polygons(geometry, region)
        .into_iter()
        .map(|points| {
            VectorItem::polygon(points)
                .fill(Color::rgba(color.r, color.g, color.b, 48))
                .stroke(Color::rgba(color.r, color.g, color.b, 220), line)
        })
        .collect()
}

/// The hint the selection tools share before a press.
pub(crate) fn selection_hint(what: &str, command: &str) -> String {
    format!(
        "{what} · Shift adds, {command} subtracts, both intersect · right-click clears the selection"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_at_the_press_choose_the_mode() {
        let default = SelectionMode::Replace;
        assert_eq!(
            mode_at_press(Modifiers::NONE, default),
            SelectionMode::Replace
        );
        assert_eq!(mode_at_press(Modifiers::SHIFT, default), SelectionMode::Add);
        assert_eq!(
            mode_at_press(Modifiers::COMMAND, default),
            SelectionMode::Subtract
        );
        assert_eq!(
            mode_at_press(Modifiers::SHIFT | Modifiers::COMMAND, default),
            SelectionMode::Intersect
        );
        assert_eq!(
            mode_at_press(Modifiers::NONE, SelectionMode::Add),
            SelectionMode::Add
        );
    }
}
