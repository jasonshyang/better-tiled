//! New Tileset: a name, an image cut into tiles (or a collection of images), the tile size, the
//! margin and spacing, and a colour drawn as transparent. The tileset is embedded in the map
//! that is current. Browse… asks for the image; its size is read to show the tiles it holds.

use std::path::PathBuf;

use bt_core::Color;
use bt_editor::{DocumentId, Intent, NewTileset, NewTilesetSource};
use glam::UVec2;

use super::form::{ButtonsAnswer, DialogKeys, dialog_buttons};

/// What the dialog needs the window to do for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DialogRequest {
    /// Ask for the tileset's image.
    PickTilesetImage,
}

/// The New Tileset dialog's state.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NewTilesetDialog {
    doc: DocumentId,
    name: String,
    collection: bool,
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
            collection: false,
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
    }

    /// The tileset as the dialog stands, or why it cannot be made.
    fn tileset(&self) -> Result<NewTileset, String> {
        let source = if self.collection {
            NewTilesetSource::Collection
        } else {
            let image = self
                .image
                .clone()
                .ok_or_else(|| "Choose the tileset's image".to_owned())?;
            let image_size = match &self.image_size {
                Some(Ok(size)) => *size,
                Some(Err(problem)) => return Err(problem.clone()),
                None => return Err("Choose the tileset's image".to_owned()),
            };
            NewTilesetSource::Atlas {
                image,
                image_size,
                transparent: self
                    .transparent
                    .map(|c| Color::rgba(c.r(), c.g(), c.b(), 255)),
                margin: self.margin,
                spacing: self.spacing,
            }
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
        requests: &mut Vec<DialogRequest>,
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
                .show(ui, |ui| {
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut self.name).changed() {
                        self.name_typed = true;
                    }
                    ui.end_row();
                    ui.label("Type");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.collection, false, "Based on an image");
                        ui.radio_value(&mut self.collection, true, "Collection of images");
                    });
                    ui.end_row();
                    if !self.collection {
                        ui.label("Image");
                        ui.horizontal(|ui| {
                            let shown = self
                                .image
                                .as_deref()
                                .map(bt_utils::paths::display_name)
                                .unwrap_or_else(|| "none".to_owned());
                            ui.label(shown);
                            if ui.button("Browse…").clicked() {
                                requests.push(DialogRequest::PickTilesetImage);
                            }
                        });
                        ui.end_row();
                        ui.label("Transparent colour");
                        ui.horizontal(|ui| {
                            let mut keyed = self.transparent.is_some();
                            if ui.checkbox(&mut keyed, "").changed() {
                                self.transparent =
                                    keyed.then_some(egui::Color32::from_rgb(255, 0, 255));
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
                    });
                    ui.end_row();
                    if !self.collection {
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
                });
            ui.add_space(6.0);
            match &made {
                Ok(new) => {
                    let tileset = new.tileset();
                    if self.collection {
                        ui.weak("Add images to it from the Tiles panel");
                    } else {
                        ui.weak(format!(
                            "{} tiles in {} columns, embedded in the map",
                            tileset.tile_count(),
                            tileset.columns()
                        ));
                    }
                }
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
}
