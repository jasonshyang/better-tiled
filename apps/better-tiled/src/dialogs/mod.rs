//! Dialogs and modals: questions from the editor, errors and dialogs, one at a time.

mod choose_layer;
mod form;
mod modal;
mod new_map;
mod new_tileset;
mod palette;
mod preferences;

pub(crate) use choose_layer::ChooseLayerDialog;
pub(crate) use modal::{Modal, Modals};
pub(crate) use new_map::NewMapDialog;
pub(crate) use new_tileset::{DialogRequest, NewTilesetDialog};
pub(crate) use palette::PaletteDialog;
pub(crate) use preferences::PreferencesDialog;
