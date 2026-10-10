//! New Map: the grid's shape, how layers are written, the size in tiles (or infinite) and the
//! size of a tile, as Tiled's dialog asks them. OK makes the map; Escape cancels.

use bt_core::{LayerDataFormat, Orientation, RenderOrder};
use bt_editor::{Intent, NewMap};
use glam::UVec2;

use super::form::{ButtonsAnswer, DialogKeys, choice, dialog_buttons};

/// The New Map dialog's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewMapDialog {
    new: NewMap,
}

const ORIENTATIONS: [(Orientation, &str); 5] = [
    (Orientation::Orthogonal, "Orthogonal"),
    (Orientation::Isometric, "Isometric"),
    (Orientation::Staggered, "Isometric (Staggered)"),
    (Orientation::Hexagonal, "Hexagonal (Staggered)"),
    (Orientation::Oblique, "Oblique"),
];

const FORMATS: [(LayerDataFormat, &str); 5] = [
    (LayerDataFormat::Csv, "CSV"),
    (LayerDataFormat::Base64, "Base64 (uncompressed)"),
    (LayerDataFormat::Base64Gzip, "Base64 (gzip compressed)"),
    (LayerDataFormat::Base64Zlib, "Base64 (zlib compressed)"),
    (LayerDataFormat::Base64Zstd, "Base64 (Zstandard compressed)"),
];

const RENDER_ORDERS: [(RenderOrder, &str); 4] = [
    (RenderOrder::RightDown, "Right Down"),
    (RenderOrder::RightUp, "Right Up"),
    (RenderOrder::LeftDown, "Left Down"),
    (RenderOrder::LeftUp, "Left Up"),
];

/// The largest map side the dialog offers, in tiles, and the largest tile side, in pixels.
const MAX_TILES: u32 = 100_000;
const MAX_TILE_PIXELS: u32 = 4096;

impl NewMapDialog {
    /// Shows the dialog; the new map goes to `out`. Whether it closed.
    pub(crate) fn show(&mut self, ctx: &egui::Context, out: &mut Vec<Intent>) -> bool {
        let keys = DialogKeys::take(ctx);
        let problem = self.new.problem();
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("new_map")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("New Map");
            ui.add_space(6.0);
            egui::Grid::new("new_map_fields")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Orientation");
                    choice(ui, "orientation", &mut self.new.orientation, &ORIENTATIONS);
                    ui.end_row();
                    ui.label("Tile layer format");
                    choice(ui, "layer_format", &mut self.new.layer_format, &FORMATS);
                    ui.end_row();
                    ui.label("Tile render order");
                    choice(
                        ui,
                        "render_order",
                        &mut self.new.render_order,
                        &RENDER_ORDERS,
                    );
                    ui.end_row();
                    ui.label("Map size");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.new.infinite, false, "Fixed");
                        ui.radio_value(&mut self.new.infinite, true, "Infinite");
                    });
                    ui.end_row();
                    if !self.new.infinite {
                        ui.label("");
                        ui.horizontal(|ui| {
                            size_fields(ui, &mut self.new.size, MAX_TILES, " tiles");
                        });
                        ui.end_row();
                    }
                    ui.label("Tile size");
                    ui.horizontal(|ui| {
                        size_fields(ui, &mut self.new.tile_size, MAX_TILE_PIXELS, " px");
                    });
                    ui.end_row();
                });
            ui.add_space(6.0);
            match &problem {
                Some(problem) => {
                    ui.colored_label(ui.visuals().error_fg_color, problem);
                }
                None if !self.new.infinite => {
                    let pixels = self.new.map().bounding_rect();
                    ui.weak(format!("{} × {} pixels", pixels.w, pixels.h));
                }
                None => {
                    ui.weak("The map grows wherever tiles are painted");
                }
            }
            ui.add_space(6.0);
            answer = dialog_buttons(ui, problem.is_none());
        });
        let ok = problem.is_none() && (answer == ButtonsAnswer::Ok || keys.enter);
        if ok {
            out.push(Intent::CreateMap(self.new.clone()));
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }
}

/// Two fields for a width and a height.
fn size_fields(ui: &mut egui::Ui, size: &mut UVec2, max: u32, suffix: &str) {
    ui.add(
        egui::DragValue::new(&mut size.x)
            .range(1..=max)
            .suffix(suffix),
    );
    ui.label("×");
    ui.add(
        egui::DragValue::new(&mut size.y)
            .range(1..=max)
            .suffix(suffix),
    );
}
