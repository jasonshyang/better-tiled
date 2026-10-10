//! What the selection tools share: the mode the modifiers at a press choose, and how a region
//! of cells is drawn as a preview.

use bt_core::{Color, MapGeometry};
use bt_editor::{ActionId, Intent, SelectionMode, TileRegion};
use bt_render::VectorItem;
use egui::Modifiers;
use glam::DVec2;

use super::carry_drag::CarryDrag;
use super::tool::ToolContext;

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

/// Cells of the current layer drawn as a selection preview: filled faintly and outlined.
pub(crate) fn region_preview(
    cx: &ToolContext<'_>,
    region: &TileRegion,
    color: Color,
    line: f32,
) -> Vec<VectorItem> {
    region_polygons(&cx.map.geometry(), region)
        .into_iter()
        .map(|points| {
            VectorItem::polygon(points)
                .fill(Color::rgba(color.r, color.g, color.b, 48))
                .stroke(Color::rgba(color.r, color.g, color.b, 220), line)
                .translated(cx.layer_offset)
        })
        .collect()
}

/// The hint the selection tools share before a press.
pub(crate) fn selection_hint(what: &str, command: &str) -> String {
    format!(
        "{what} · Shift adds, {command} subtracts, both intersect · drag the selection to move its tiles, X and Y mirror them, Z turns them · right-click clears the selection"
    )
}

/// What the status bar says while the selected tiles are dragged or carried; `None` otherwise.
pub(crate) fn carry_hint(carry: Option<CarryDrag>, cx: &ToolContext<'_>) -> Option<String> {
    if carry.is_some_and(|c| c.is_moving()) {
        return Some("Moving the selected tiles · Escape puts them back".to_owned());
    }
    let carrying = cx.selection().is_some_and(|s| s.carries_tiles());
    carrying.then(|| {
        "The selection carries its tiles: drag to move them on, X and Y mirror, Z turns · what they cover comes back as they move · Escape lets them go where they are".to_owned()
    })
}

/// Escape with nothing under way: carried tiles are let go where they are, and the selection
/// with them.
pub(crate) fn let_go(cx: &mut ToolContext<'_>) {
    if cx.selection().is_some_and(|s| s.carries_tiles()) {
        cx.out.push(Intent::Run(ActionId::SelectNone));
    }
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
