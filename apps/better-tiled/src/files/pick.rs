//! File dialogs, through `rfd`'s asynchronous dialogs waited for on a worker so a frame never
//! blocks; the choice comes back as [`Intent::FilePicked`].

use bt_editor::{FileRequest, Intent};

use crate::workers::Workers;

/// The map formats the dialogs offer.
const MAP_EXTENSIONS: [&str; 3] = ["tmx", "tmj", "json"];

/// The tileset formats the dialogs offer.
const TILESET_EXTENSIONS: [&str; 2] = ["tsx", "tsj"];

/// The image formats the dialogs offer (what `bt-image` decodes).
const IMAGE_EXTENSIONS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "tga", "tif", "tiff", "ico",
];

/// How the window asks for files: with the system's dialogs, or (in the window's tests, which
/// have no system to show them) by recording what it asked, for the test to answer.
#[derive(Debug)]
pub(crate) enum FileDialogs {
    /// The system's dialogs, on workers.
    System,
    /// The requests made, unanswered.
    #[cfg(test)]
    Recorded(Vec<FileRequest>),
}

impl FileDialogs {
    /// Asks for a file.
    pub(crate) fn ask(
        &mut self,
        workers: &mut Workers,
        request: FileRequest,
    ) -> Result<(), String> {
        match self {
            FileDialogs::System => pick_file(workers, request),
            #[cfg(test)]
            FileDialogs::Recorded(requests) => {
                requests.push(request);
                Ok(())
            }
        }
    }
}

/// Asks for a file on a worker.
fn pick_file(workers: &mut Workers, request: FileRequest) -> Result<(), String> {
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
        FileRequest::TilesetImage => {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Tileset Image")
                .add_filter("Images", &IMAGE_EXTENSIONS)
                .add_filter("All files", &["*"]);
            let picked = pollster::block_on(dialog.pick_file());
            vec![Intent::FilePicked {
                request: request.clone(),
                path: picked.map(|file| file.path().to_path_buf()),
            }]
        }
        FileRequest::ExternalTileset(_) => {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title("Add External Tileset")
                .add_filter("Tiled tilesets", &TILESET_EXTENSIONS)
                .add_filter("All files", &["*"]);
            let picked = pollster::block_on(dialog.pick_file());
            vec![Intent::FilePicked {
                request: request.clone(),
                path: picked.map(|file| file.path().to_path_buf()),
            }]
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
