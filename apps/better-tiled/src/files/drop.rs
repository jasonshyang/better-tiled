//! Files dropped on the window open like files chosen in the Open dialog.

use std::path::PathBuf;

/// The files dropped on the window this frame.
pub(crate) fn dropped_files(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    })
}
