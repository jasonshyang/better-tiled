//! Dialogs and modals: questions from the editor, errors and dialogs, one at a time.

mod modal;
mod palette;

pub(crate) use modal::{Modal, Modals};
pub(crate) use palette::PaletteDialog;
