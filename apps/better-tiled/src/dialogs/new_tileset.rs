//! New Tileset: a name, an image cut into tiles (or a collection of images, or an RPG Maker A2
//! sheet of autotiles), the tile size, the margin and spacing, and a colour drawn as transparent.
//! The tileset is embedded in the map that is current. Browse… asks for the image; its size is
//! read to show the tiles it holds. An RPG Maker sheet's tile size is guessed from its shape,
//! and a sheet that would not be put together right says why; its tiles are put together on a
//! worker when the dialog is done.

use std::path::PathBuf;

use bt_core::Color;
use bt_editor::{DocumentId, FileRequest, Intent, NewTileset, NewTilesetSource};
use bt_terrain::rpg_maker;
use glam::UVec2;

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};

/// Where the tiles come from, as the dialog offers it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    /// An image cut into a grid.
    Image,
    /// One image per tile.
    Collection,
    /// An RPG Maker A2 sheet of floor autotiles.
    Autotiles,
}

/// The New Tileset dialog's state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewTilesetDialog {
    doc: DocumentId,
    name: String,
    kind: SourceKind,
    image: Option<PathBuf>,
    /// The image's size, or why it could not be read.
    image_size: Option<Result<UVec2, String>>,
    transparent: Option<egui::Color32>,
    tile_size: UVec2,
    margin: u32,
    spacing: u32,
    /// The name was typed, so a new image does not replace it.
    name_typed: bool,
}

impl NewTilesetDialog {
    /// A dialog adding a tileset to the map of `doc`, with tiles of the map's size.
    pub(crate) fn new(doc: DocumentId, tile_size: UVec2) -> NewTilesetDialog {
        NewTilesetDialog {
            doc,
            name: String::new(),
            kind: SourceKind::Image,
            image: None,
            image_size: None,
            transparent: None,
            tile_size: tile_size.max(UVec2::ONE),
            margin: 0,
            spacing: 0,
            name_typed: false,
        }
    }

    /// The image the user chose, with its size read now.
    pub(crate) fn set_image(&mut self, path: PathBuf) {
        self.image_size = Some(bt_image::probe_size(&path).map_err(|error| error.to_string()));
        if !self.name_typed
            && let Some(stem) = path.file_stem()
        {
            self.name = stem.to_string_lossy().into_owned();
        }
        self.image = Some(path);
        self.guess_autotile_size();
    }

    /// For an RPG Maker sheet, the tile size its shape suggests (RPG Maker's 48 or 32 pixels on
    /// a whole sheet or a single autotile), square.
    fn guess_autotile_size(&mut self) {
        if self.kind != SourceKind::Autotiles {
            return;
        }
        if let Some(Ok(size)) = &self.image_size
            && let Some(tile) = rpg_maker::guess_tile(size.x, size.y, self.tile_size.x)
        {
            self.tile_size = UVec2::splat(tile);
        }
    }

    /// The image chosen, or why there is none.
    fn chosen_image(&self) -> Result<PathBuf, String> {
        self.image
            .clone()
            .ok_or_else(|| "Choose the tileset's image".to_owned())
    }

    /// The size of the image chosen, or why it is not known.
    fn chosen_size(&self) -> Result<UVec2, String> {
        match &self.image_size {
            Some(Ok(size)) => Ok(*size),
            Some(Err(problem)) => Err(problem.clone()),
            None => Err("Choose the tileset's image".to_owned()),
        }
    }

    /// The tileset as the dialog stands, or why it cannot be made.
    fn tileset(&self) -> Result<NewTileset, String> {
        let source = match self.kind {
            SourceKind::Collection => NewTilesetSource::Collection,
            SourceKind::Image => NewTilesetSource::Atlas {
                image: self.chosen_image()?,
                image_size: self.chosen_size()?,
                transparent: self
                    .transparent
                    .map(|c| Color::rgba(c.r(), c.g(), c.b(), 255)),
                margin: self.margin,
                spacing: self.spacing,
            },
            SourceKind::Autotiles => NewTilesetSource::RpgMakerAutotiles {
                sheet: self.chosen_image()?,
                sheet_size: self.chosen_size()?,
            },
        };
        let new = NewTileset {
            name: self.name.clone(),
            tile_size: self.tile_size,
            source,
        };
        match new.problem() {
            Some(problem) => Err(problem),
            None => Ok(new),
        }
    }

    /// Shows the dialog; the new tileset goes to `out`. Whether it closed; a request for the
    /// window goes to `requests`.
    pub(crate) fn show(
        &mut self,
        ctx: &egui::Context,
        out: &mut Vec<Intent>,
        requests: &mut Vec<FileRequest>,
    ) -> bool {
        let keys = DialogKeys::take(ctx);
        let made = self.tileset();
        let mut answer = ButtonsAnswer::None;
        let response = egui::Modal::new(egui::Id::new("new_tileset")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("New Tileset");
            ui.add_space(6.0);
            egui::Grid::new("new_tileset_fields")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| self.show_fields(ui, requests));
            ui.add_space(6.0);
            match &made {
                Ok(new) => self.show_outcome(ui, new),
                Err(problem) => {
                    ui.colored_label(ui.visuals().error_fg_color, problem);
                }
            }
            ui.add_space(6.0);
            answer = dialog_buttons(ui, made.is_ok());
        });
        if let Ok(new) = made
            && (answer == ButtonsAnswer::Ok || keys.enter)
        {
            out.push(Intent::CreateTileset {
                doc: self.doc,
                tileset: new,
            });
            return true;
        }
        answer == ButtonsAnswer::Cancel || keys.escape || response.should_close()
    }

    /// The fields, row by row.
    fn show_fields(&mut self, ui: &mut egui::Ui, requests: &mut Vec<FileRequest>) {
        ui.label("Name");
        if ui.text_edit_singleline(&mut self.name).changed() {
            self.name_typed = true;
        }
        ui.end_row();
        ui.label("Type");
        ui.vertical(|ui| {
            let before = self.kind;
            ui.radio_value(&mut self.kind, SourceKind::Image, "Based on an image");
            ui.radio_value(
                &mut self.kind,
                SourceKind::Collection,
                "Collection of images",
            );
            ui.radio_value(&mut self.kind, SourceKind::Autotiles, "RPG Maker autotiles")
                .on_hover_text(
                    "An RPG Maker A2 sheet of floors: each autotile is put together into the 47 \
                     tiles of a blob set, a terrain of one set, written as a new image beside \
                     the sheet",
                );
            if self.kind != before {
                self.guess_autotile_size();
            }
        });
        ui.end_row();
        if self.kind != SourceKind::Collection {
            ui.label(if self.kind == SourceKind::Autotiles {
                "Sheet"
            } else {
                "Image"
            });
            ui.horizontal(|ui| {
                let shown = self
                    .image
                    .as_deref()
                    .map(bt_utils::paths::display_name)
                    .unwrap_or_else(|| "none".to_owned());
                ui.label(shown);
                if ui.button("Browse…").clicked() {
                    requests.push(FileRequest::TilesetImage);
                }
            });
            ui.end_row();
        }
        if self.kind == SourceKind::Image {
            ui.label("Transparent colour");
            ui.horizontal(|ui| {
                let mut keyed = self.transparent.is_some();
                if ui.checkbox(&mut keyed, "").changed() {
                    self.transparent = keyed.then_some(egui::Color32::from_rgb(255, 0, 255));
                }
                if let Some(color) = &mut self.transparent {
                    egui::color_picker::color_edit_button_srgba(
                        ui,
                        color,
                        egui::color_picker::Alpha::Opaque,
                    );
                }
            });
            ui.end_row();
        }
        ui.label("Tile size");
        ui.horizontal(|ui| {
            if self.kind == SourceKind::Autotiles {
                // Square: one size.
                let mut side = self.tile_size.x;
                ui.add(
                    egui::DragValue::new(&mut side)
                        .range(2..=4096)
                        .suffix(" px"),
                );
                self.tile_size = UVec2::splat(side);
            } else {
                ui.add(
                    egui::DragValue::new(&mut self.tile_size.x)
                        .range(1..=4096)
                        .suffix(" px"),
                );
                ui.label("×");
                ui.add(
                    egui::DragValue::new(&mut self.tile_size.y)
                        .range(1..=4096)
                        .suffix(" px"),
                );
            }
        });
        ui.end_row();
        if self.kind == SourceKind::Image {
            ui.label("Margin, spacing");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.margin)
                        .range(0..=1024)
                        .suffix(" px"),
                );
                ui.add(
                    egui::DragValue::new(&mut self.spacing)
                        .range(0..=1024)
                        .suffix(" px"),
                );
            });
            ui.end_row();
        }
    }

    /// What the tileset will be, below the fields.
    fn show_outcome(&self, ui: &mut egui::Ui, new: &NewTileset) {
        match &new.source {
            NewTilesetSource::Collection => {
                ui.weak("Add images to it from the Tiles panel");
            }
            NewTilesetSource::Atlas { .. } => {
                if let Some(tileset) = new.tileset() {
                    ui.weak(format!(
                        "{} tiles in {} columns, embedded in the map",
                        tileset.tile_count(),
                        tileset.columns()
                    ));
                }
            }
            NewTilesetSource::RpgMakerAutotiles { sheet, sheet_size } => {
                let count = rpg_maker::autotiles(sheet_size.x, sheet_size.y, new.tile_size.x);
                let stem = sheet
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.weak(format!(
                    "{} ({}×{}), each a terrain of one set: their tiles are written to “{stem} \
                     blob.png” beside the sheet, embedded in the map",
                    bt_utils::text::plural(
                        usize::try_from(count.x * count.y).unwrap_or(usize::MAX),
                        "autotile",
                        "autotiles"
                    ),
                    count.x,
                    count.y
                ));
                if let Some(warning) = new.warning() {
                    ui.colored_label(ui.visuals().warn_fg_color, warning);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rpg_maker_sheet_gets_the_tile_size_its_shape_says() {
        let mut dialog = NewTilesetDialog::new(DocumentId(1), UVec2::splat(32));
        dialog.kind = SourceKind::Autotiles;
        dialog.image = Some(PathBuf::from("/sheets/Outside_A2.png"));
        dialog.image_size = Some(Ok(UVec2::new(768, 576)));
        dialog.name = "Outside".into();
        dialog.guess_autotile_size();
        assert_eq!(dialog.tile_size, UVec2::splat(48), "an RPG Maker MV sheet");
        let new = dialog.tileset().unwrap();
        assert_eq!(
            new.source,
            NewTilesetSource::RpgMakerAutotiles {
                sheet: PathBuf::from("/sheets/Outside_A2.png"),
                sheet_size: UVec2::new(768, 576)
            }
        );
        dialog.tile_size = UVec2::splat(47);
        assert!(dialog.tileset().unwrap_err().contains("even"));
    }
}
