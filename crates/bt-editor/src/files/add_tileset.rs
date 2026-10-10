//! Add External Tileset: the file chosen is read on a worker as any load is, then the map refers
//! to it as one step (Tiled's `MainWindow::addExternalTileset` and
//! `MapEditor::handleExternalTilesetsAndImages`, `reference/tiled/src/tiled/mainwindow.cpp` and
//! `tiled/mapeditor.cpp`). A file the store has already (another map uses it, or its tab is
//! open) is that asset, unsaved changes and all; a tileset the map has already is reported, not
//! added twice (Tiled leaves it out without a word). One file at a time, so one step each.

use std::path::Path;
use std::sync::Arc;

use bt_core::{FileRef, TilesetRef};
use bt_edit::{add_tileset, find_tileset};

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::files::{
    LoadJob, LoadKind, LoadPurpose, LoadSource, Loaded, LoadedContent, ProjectChoice,
};
use crate::notice::Notice;
use crate::store::{NewAsset, Root};

impl Editor {
    /// A load of the tileset file `path` for adding to `doc`'s map, its properties read with
    /// the custom types the map's were.
    pub(crate) fn load_external_tileset(&self, doc: DocumentId, path: &Path) -> Vec<Effect> {
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => {
                return vec![Effect::Notify(Notice::error(format!(
                    "Cannot add {}: {error}",
                    path.display()
                )))];
            }
        };
        vec![Effect::Load(LoadJob {
            path,
            source: LoadSource::File,
            project: ProjectChoice::Known(Arc::new(self.custom_types().clone())),
            kind: LoadKind::Tileset,
            purpose: LoadPurpose::AddToMap(doc),
            home: self.files.home.clone(),
            recovered_layers: None,
        })]
    }

    /// Takes in a tileset read for `doc`'s map: the map refers to its file as one step, unless
    /// it does already.
    pub(crate) fn finish_adding_tileset(&mut self, doc: DocumentId, loaded: Loaded) -> Vec<Effect> {
        let name = bt_utils::paths::display_name(&loaded.path);
        let files = match loaded.outcome {
            Ok(files) => files,
            Err(message) => {
                return vec![Effect::Notify(
                    Notice::error(format!("Cannot add {name}")).with_detail(message),
                )];
            }
        };
        let LoadedContent::Tileset(read) = files.content else {
            return vec![Effect::Notify(Notice::error(format!(
                "Cannot add {name}: it is not a tileset"
            )))];
        };
        // The document may have closed while the file was read.
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let known = [&loaded.path, &loaded.canonical]
            .into_iter()
            .filter_map(|path| self.store.by_path(path))
            .find(|id| self.store.tileset(*id).is_some());
        let file = known
            .and_then(|id| self.store.get(id))
            .and_then(|known| known.path())
            .map_or_else(|| loaded.canonical.clone(), Path::to_path_buf);
        let tileset = known
            .and_then(|id| self.store.tileset(id))
            .map_or_else(|| Arc::clone(&read), Arc::clone);
        let reference = TilesetRef {
            // No written form yet: the writer writes it relative to the map.
            file: Some(FileRef::new("", file.clone())),
            first_gid: None,
            tileset,
        };
        let Some(map) = self.store.map(asset) else {
            return vec![warn(EditError::NoSuchDocument(doc))];
        };
        let has_it = [&file, &loaded.path, &loaded.canonical]
            .into_iter()
            .any(|path| {
                let same_file = TilesetRef {
                    file: Some(FileRef::new("", path.clone())),
                    ..reference.clone()
                };
                find_tileset(map, &same_file).is_some()
            });
        if has_it {
            return vec![Effect::Notify(Notice::info(format!(
                "The map has {name} already"
            )))];
        }
        let warning_count = files.warnings.len();
        if known.is_none() {
            self.store.insert(NewAsset {
                root: Root::Tileset(read),
                path: Some(file),
                content_hash: files.content_hash,
                warnings: files.warnings,
            });
        }
        let result = self.transact(doc, "Add External Tileset", |tx| {
            let map = tx.map_mut(asset)?;
            add_tileset(map, reference).map_err(EditError::from)
        });
        if let Err(error) = result {
            return vec![warn(error)];
        }
        if warning_count == 0 {
            return Vec::new();
        }
        vec![Effect::Notify(Notice::warning(format!(
            "{name} was added with {}",
            bt_utils::text::plural(warning_count, "warning", "warnings")
        )))]
    }
}
