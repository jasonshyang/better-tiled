//! Remembering an export belongs to the map's undo history; the window performs exports.
use crate::{DocumentId, EditError, Editor};
use std::path::Path;
impl Editor {
    /// Records the target as a single transaction, without saving or exporting the map.
    pub fn remember_export(&mut self, doc: DocumentId, target: &Path) -> Result<(), EditError> {
        let asset = self
            .document(doc)
            .map(|d| d.asset())
            .ok_or(EditError::NoSuchDocument(doc))?;
        let source = self.path_of(doc);
        let folder = source
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        let reference =
            bt_core::FileRef::new(bt_utils::paths::reference_path(folder, target), target);
        self.transact(doc, "Remember Godot export", |tx| {
            let map = tx.map_mut(asset)?;
            map.file.export.format = "tscn".into();
            map.file.export.target = Some(reference);
            Ok(())
        })
    }
}

impl Editor {
    /// The project file where this map's export settings belong.
    pub fn export_project_file(&self, doc: DocumentId) -> Result<std::path::PathBuf, String> {
        let path = self
            .path_of(doc)
            .ok_or("Save the map before configuring Godot")?;
        bt_project::ProjectFile::location(&path, self.project_path(), self.files.home.as_deref())
            .map_err(|e| e.to_string())
    }
    /// The configured folder for locks shared by the editor and command line.
    pub fn project_lock_folder(&self) -> Option<&Path> {
        self.files.lock_folder.as_deref()
    }
}
