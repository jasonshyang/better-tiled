//! File work for the Preferences draft's Tiled shortcut schemes, off the frame thread.

use std::path::PathBuf;

use bt_editor::{Intent, Notice};
use bt_utils::atomic_write::{WriteMode, write_file};

use super::App;

impl App {
    pub(super) fn start_shortcut_import(&mut self, dialog: u64, path: PathBuf) {
        let spawned = self.workers.spawn("shortcut import", move || {
            vec![Intent::ShortcutsRead {
                dialog,
                result: std::fs::read_to_string(&path)
                    .map_err(|error| format!("Cannot read {}: {error}", path.display())),
            }]
        });
        if let Err(error) = spawned {
            self.notify(Notice::error(error));
        }
    }

    pub(super) fn start_shortcut_export(&mut self, path: PathBuf, scheme: String) {
        let spawned = self.workers.spawn("shortcut export", move || {
            let notice = match write_file(&path, scheme.as_bytes(), WriteMode::Atomic) {
                Ok(()) => Notice::info(format!("Exported shortcuts to {}", path.display())),
                Err(error) => Notice::error(format!(
                    "Cannot export shortcuts to {}: {error}",
                    path.display()
                )),
            };
            vec![Intent::Report(notice)]
        });
        if let Err(error) = spawned {
            self.notify(Notice::error(error));
        }
    }
}
