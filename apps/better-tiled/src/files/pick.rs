//! File dialogs, through `rfd`'s asynchronous dialogs waited for on a worker so a frame never
//! blocks; the choice comes back as [`Intent::FilePicked`].

use bt_editor::{FileRequest, Intent};

use crate::workers::Workers;

/// The map formats the dialogs offer.
const MAP_EXTENSIONS: [&str; 3] = ["tmx", "tmj", "json"];

/// Asks for a file on a worker.
pub(crate) fn pick_file(workers: &mut Workers, request: FileRequest) -> Result<(), String> {
    workers.spawn("file-dialog", move || match &request {
        FileRequest::Open => {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Open")
                .add_filter("Tiled maps", &MAP_EXTENSIONS)
                .add_filter("All files", &["*"]);
            let picked = pollster::block_on(dialog.pick_files()).unwrap_or_default();
            if picked.is_empty() {
                return vec![Intent::FilePicked {
                    request: request.clone(),
                    path: None,
                }];
            }
            picked
                .into_iter()
                .map(|file| Intent::FilePicked {
                    request: request.clone(),
                    path: Some(file.path().to_path_buf()),
                })
                .collect()
        }
        FileRequest::SaveAs { suggested, .. } => {
            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title("Save As")
                .add_filter("Tiled maps", &MAP_EXTENSIONS);
            if let Some(folder) = suggested.parent().filter(|f| !f.as_os_str().is_empty()) {
                dialog = dialog.set_directory(folder);
            }
            if let Some(name) = suggested.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            let picked = pollster::block_on(dialog.save_file());
            vec![Intent::FilePicked {
                request: request.clone(),
                path: picked.map(|file| file.path().to_path_buf()),
            }]
        }
    })
}
