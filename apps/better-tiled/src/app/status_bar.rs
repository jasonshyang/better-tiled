//! The status bar: the zoom and the map's size on the right; on the left the flash notice, or
//! the cell and pixel under the pointer.

use bt_core::Map;
use bt_editor::{Notice, Severity};

use crate::canvas::Hover;

/// What the status bar shows.
pub(crate) struct StatusInput<'a> {
    /// A notice being flashed.
    pub(crate) flash: Option<&'a Notice>,
    /// What is under the pointer.
    pub(crate) hover: Option<Hover>,
    /// The current canvas's zoom.
    pub(crate) zoom: Option<f64>,
    /// The current map.
    pub(crate) map: Option<&'a Map>,
}

/// Draws the status bar.
pub(crate) fn status_bar(ui: &mut egui::Ui, input: &StatusInput<'_>) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(zoom) = input.zoom {
                ui.label(format!("{:.0} %", zoom * 100.0));
            }
            if let Some(map) = input.map {
                ui.separator();
                ui.label(map_size(map));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if let Some(notice) = input.flash {
                    let color = match notice.severity {
                        Severity::Info => ui.visuals().text_color(),
                        Severity::Warning => ui.visuals().warn_fg_color,
                        Severity::Error => ui.visuals().error_fg_color,
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(&notice.text).color(color)).truncate(),
                    );
                } else if let Some(hover) = input.hover {
                    ui.label(format!(
                        "{}, {}  ·  {:.1}, {:.1} px",
                        hover.cell.x, hover.cell.y, hover.pixel.x, hover.pixel.y
                    ));
                }
            });
        });
    });
}

/// A map's size in cells (and in pixels), or that it is infinite.
fn map_size(map: &Map) -> String {
    if map.infinite {
        return format!("infinite · {}×{} px tiles", map.tile_width, map.tile_height);
    }
    format!(
        "{} × {} tiles of {}×{} px",
        map.width, map.height, map.tile_width, map.tile_height
    )
}
