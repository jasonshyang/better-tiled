//! Dialogs and modals: questions from the editor, errors and dialogs, one at a time.

mod choose_layer;
mod form;
mod modal;
mod new_map;
mod new_tileset;
mod offset_map;
mod palette;
mod preferences;
mod resize_map;

pub(crate) use choose_layer::ChooseLayerDialog;
pub(crate) use modal::{Modal, Modals};
pub(crate) use new_map::NewMapDialog;
pub(crate) use new_tileset::{DialogRequest, NewTilesetDialog};
pub(crate) use offset_map::OffsetMapDialog;
pub(crate) use palette::PaletteDialog;
pub(crate) use preferences::PreferencesDialog;
pub(crate) use resize_map::ResizeMapDialog;
