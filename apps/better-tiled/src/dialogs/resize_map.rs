//! Resize Map: the new size in tiles, where the old map sits in it, and whether objects left
//! outside go (Tiled's `ResizeDialog`, `reference/tiled/src/tiled/resizedialog.cpp`). The preview
//! follows Tiled's `ResizeHelper` (`tiled/resizehelper.cpp`) with rectangles for the minimap: the
//! new bounds, and inside them the old map, which dragging moves by whole cells. The offset stays
//! where the old map still covers the new bounds on every side it can. OK resizes the map as one
//! step when the size or the place changed; Escape cancels.

use bt_editor::{DocumentId, Intent};
use glam::{IVec2, UVec2};

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};

/// The largest side the dialog offers, in tiles (Tiled's).
const MAX_TILES: u32 = 99_999;

/// The preview's size, in points.
const PREVIEW_SIZE: egui::Vec2 = egui::vec2(380.0, 200.0);

/// The Resize Map dialog's state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResizeMapDialog {
    doc: DocumentId,
    /// The map's size now, in tiles.
    old_size: UVec2,
    /// The size asked for.
    new_size: UVec2,
    /// Where the old map's top-left cell goes in the new bounds.
    offset: IVec2,
    /// Whether objects that end up outside the map are removed (Tiled's default).
    remove_objects_outside: bool,
    /// The offset when a drag of the preview started.
    drag_start: Option<IVec2>,
}

/// The offsets the old map may take, from `min` to `max` with both included: along a side the
/// new map grows, the old one stays inside it; along a side it shrinks, the old one covers it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct OffsetLimits {
    min: IVec2,
    max: IVec2,
}

impl OffsetLimits {
    /// The limits for resizing a map of `old` tiles to `new` (`ResizeHelper::recalculateMinMaxOffset`).
    fn between(old: UVec2, new: UVec2) -> OffsetLimits {
        let growth = new.as_ivec2() - old.as_ivec2();
        OffsetLimits {
            min: growth.min(IVec2::ZERO),
            max: growth.max(IVec2::ZERO),
        }
    }

    /// `offset` moved inside the limits.
    fn clamp(self, offset: IVec2) -> IVec2 {
        offset.clamp(self.min, self.max)
    }
}

/// Where the preview draws, in points.
#[derive(Copy, Clone, Debug, PartialEq)]
struct PreviewLayout {
    /// Points per tile.
    scale: f32,
    /// The new bounds.
    new_map: egui::Rect,
    /// The old map, at the offset.
    old_map: egui::Rect,
}

impl PreviewLayout {
    /// The layout in `area` (`ResizeHelper::recalculateScale` and `paintEvent`): the scale
    /// leaves room for the old map at either end of its limits, and the new bounds are centred.
    fn of(area: egui::Rect, old: UVec2, new: UVec2, offset: IVec2) -> PreviewLayout {
        let old = old.as_vec2();
        let new = new.as_vec2();
        let span = glam::Vec2::select(old.cmplt(new), new, 2.0 * old - new).max(glam::Vec2::ONE);
        let scale = (area.width() / span.x).min(area.height() / span.y);
        let new_size = egui::vec2(new.x, new.y) * scale;
        let origin = area.center() - new_size / 2.0;
        let old_min = origin + egui::vec2(offset.x as f32, offset.y as f32) * scale;
        PreviewLayout {
            scale,
            new_map: egui::Rect::from_min_size(origin, new_size),
            old_map: egui::Rect::from_min_size(old_min, egui::vec2(old.x, old.y) * scale),
        }
    }

    /// The offset a drag of `delta` points makes of `start`, in whole cells.
    fn dragged_offset(&self, start: IVec2, delta: egui::Vec2) -> IVec2 {
        if self.scale <= 0.0 {
            return start;
        }
        let cells = delta / self.scale;
        start + IVec2::new(cells.x.round() as i32, cells.y.round() as i32)
    }
}

impl ResizeMapDialog {
    /// A dialog for the map of `doc`, which is `size` tiles now.
    pub(crate) fn new(doc: DocumentId, size: UVec2) -> ResizeMapDialog {
        ResizeMapDialog {
            doc,
            old_size: size,
            new_size: size,
            offset: IVec2::ZERO,
            remove_objects_outside: true,
            drag_start: None,
        }
    }

    /// What OK asks for: nothing when the size and the place stay, as in Tiled.
    fn intent(&self) -> Option<Intent> {
        if self.new_size == self.old_size && self.offset == IVec2::ZERO {
            return None;
        }
        Some(Intent::ResizeMap {
            doc: self.doc,
            size: self.new_size,
            offset: self.offset,
            remove_objects_outside: self.remove_objects_outside,
        })
    }

    /// Shows the dialog; the resize goes to `out`. Whether it closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context, out: &mut Vec<Intent>) -> bool {
        let keys = DialogKeys::take(ctx);
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("resize_map")).show(ctx, |ui| {
            ui.set_width(PREVIEW_SIZE.x + 16.0);
            ui.heading("Resize Map");
            ui.add_space(6.0);
            egui::Grid::new("resize_map_fields")
                .num_columns(4)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.strong("Size");
                    ui.end_row();
                    let tiles = 1..=MAX_TILES;
                    number_field(ui, "Width", &mut self.new_size.x, tiles.clone());
                    number_field(ui, "Height", &mut self.new_size.y, tiles);
                    ui.end_row();
                    let limits = OffsetLimits::between(self.old_size, self.new_size);
                    self.offset = limits.clamp(self.offset);
                    ui.strong("Offset");
                    ui.end_row();
                    number_field(ui, "X", &mut self.offset.x, limits.min.x..=limits.max.x);
                    number_field(ui, "Y", &mut self.offset.y, limits.min.y..=limits.max.y);
                    ui.end_row();
                });
            ui.add_space(6.0);
            self.show_preview(ui);
            ui.add_space(6.0);
            ui.checkbox(
                &mut self.remove_objects_outside,
                "Remove objects outside the map",
            );
            ui.add_space(6.0);
            answer = dialog_buttons(ui, true);
        });
        if answer == ButtonsAnswer::Ok || keys.enter {
            out.extend(self.intent());
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }

    /// The new bounds and the old map in them; dragging moves the old map by whole cells.
    fn show_preview(&mut self, ui: &mut egui::Ui) {
        let (area, response) = ui.allocate_exact_size(PREVIEW_SIZE, egui::Sense::drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Map preview")
        });
        let limits = OffsetLimits::between(self.old_size, self.new_size);
        let layout = PreviewLayout::of(area.shrink(1.0), self.old_size, self.new_size, self.offset);
        if response.drag_started() {
            self.drag_start = Some(self.offset);
        }
        if let (Some(start), Some(delta)) = (self.drag_start, response.total_drag_delta()) {
            self.offset = limits.clamp(layout.dragged_offset(start, delta));
        }
        if response.drag_stopped() {
            self.drag_start = None;
        }
        let layout = PreviewLayout::of(area.shrink(1.0), self.old_size, self.new_size, self.offset);
        let visuals = ui.visuals();
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 2.0, visuals.extreme_bg_color);
        painter.rect_stroke(
            layout.new_map,
            0.0,
            visuals.widgets.inactive.fg_stroke,
            egui::StrokeKind::Inside,
        );
        let selection = visuals.selection.bg_fill;
        painter.rect_filled(layout.old_map, 0.0, selection.gamma_multiply(0.5));
        painter.rect_stroke(
            layout.old_map,
            0.0,
            egui::Stroke::new(1.0, selection),
            egui::StrokeKind::Inside,
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        }
    }
}

/// A labelled number of tiles within `range`.
fn number_field<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
) {
    let label = ui.label(label);
    ui.add(egui::DragValue::new(value).range(range).suffix(" tiles"))
        .labelled_by(label.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_map_stays_inside_a_larger_map_and_covers_a_smaller_one() {
        let limits = OffsetLimits::between(UVec2::new(10, 10), UVec2::new(14, 6));
        assert_eq!(limits.min, IVec2::new(0, -4));
        assert_eq!(limits.max, IVec2::new(4, 0));
        assert_eq!(limits.clamp(IVec2::new(9, 3)), IVec2::new(4, 0));
        assert_eq!(limits.clamp(IVec2::new(-2, -9)), IVec2::new(0, -4));
    }

    #[test]
    fn the_preview_centres_the_new_bounds_and_drags_by_whole_cells() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
        // Growing from 10×5 to 20×10: the new bounds fill the height, 10 points a tile.
        let layout = PreviewLayout::of(
            area,
            UVec2::new(10, 5),
            UVec2::new(20, 10),
            IVec2::new(2, 1),
        );
        assert_eq!(layout.scale, 10.0);
        assert_eq!(layout.new_map, area);
        assert_eq!(layout.old_map.min, egui::pos2(20.0, 10.0));
        assert_eq!(layout.old_map.size(), egui::vec2(100.0, 50.0));
        assert_eq!(
            layout.dragged_offset(IVec2::new(2, 1), egui::vec2(26.0, -14.0)),
            IVec2::new(5, 0),
            "2.6 cells round to 3, -1.4 to -1"
        );
        // Shrinking from 20 to 10 wide: room for the old map at either end, 2 × 20 − 10 tiles.
        let layout = PreviewLayout::of(area, UVec2::new(20, 5), UVec2::new(10, 5), IVec2::ZERO);
        assert_eq!(layout.scale, 200.0 / 30.0);
        assert!((layout.new_map.center() - area.center()).length() < 1e-3);
    }

    #[test]
    fn ok_resizes_only_when_the_size_or_the_place_changed() {
        let doc = DocumentId(1);
        let mut dialog = ResizeMapDialog::new(doc, UVec2::new(8, 6));
        assert!(dialog.intent().is_none());
        dialog.new_size = UVec2::new(10, 6);
        dialog.offset = IVec2::new(2, 0);
        assert!(matches!(
            dialog.intent(),
            Some(Intent::ResizeMap {
                size,
                offset,
                remove_objects_outside: true,
                ..
            }) if size == UVec2::new(10, 6) && offset == IVec2::new(2, 0)
        ));
    }
}
