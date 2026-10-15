//! Dialogs and modals: questions from the editor, errors and dialogs, one at a time.

mod choose_layer;
mod commands;
mod custom_types;
mod export_image;
mod form;
mod modal;
mod new_map;
mod new_tileset;
mod offset_map;
mod palette;
mod preferences;
mod resize_map;

pub(crate) use choose_layer::ChooseLayerDialog;
pub(crate) use custom_types::CustomTypesDialog;
pub(crate) use modal::{Modal, Modals};
pub(crate) use new_map::NewMapDialog;
pub(crate) use new_tileset::NewTilesetDialog;
pub(crate) use offset_map::OffsetMapDialog;
pub(crate) use palette::PaletteDialog;
pub(crate) use resize_map::ResizeMapDialog;

pub(crate) use commands::CommandsDialog;
pub(crate) use export_image::ExportImageDialog;
