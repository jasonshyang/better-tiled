//! The open project (Tiled's `ProjectManager`, `reference/tiled/src/tiled/projectmanager.cpp`):
//! the nearest `.tiled-project` above the first map opened, or, once custom types are made with
//! no project open, an untitled project that lives for the run, as Tiled's does. A project
//! found later takes the untitled one's place.

use std::path::Path;
use std::sync::Arc;

use crate::editor::Editor;
use crate::effect::Effect;
use crate::notice::Notice;
use crate::store::{AssetId, NewAsset, Root};

impl Editor {
    /// The open project's file; none for an untitled project.
    pub fn project_path(&self) -> Option<&Path> {
        self.store.get(self.files.project?)?.path()
    }

    /// The project asset edited by Custom Types and Edit Commands.
    pub(crate) fn project_for_edits(&mut self) -> AssetId {
        if let Some(id) = self.files.project
            && self.store.project(id).is_some()
        {
            return id;
        }
        let id = self.store.insert(NewAsset {
            root: Root::Project(Arc::new(bt_core::Project::default())),
            path: None,
            content_hash: None,
            warnings: Vec::new(),
        });
        self.files.project = Some(id);
        id
    }
    /// The open project's asset, when it has a file.
    pub(crate) fn project_file(&self) -> Option<AssetId> {
        self.files.project.filter(|id| {
            self.store
                .get(*id)
                .is_some_and(|asset| asset.path().is_some())
        })
    }

    /// The untitled project gives way to the project found at `found`; a warning when it held
    /// custom types, which are lost.
    pub(crate) fn drop_untitled_project(&mut self, found: &Path) -> Vec<Effect> {
        let Some(id) = self.files.project.take() else {
            return Vec::new();
        };
        let had_settings = self.store.project(id).is_some_and(|project| {
            !project.custom_types.is_empty() || !project.commands.is_empty()
        });
        self.store.remove(id);
        if !had_settings {
            return Vec::new();
        }
        vec![Effect::Notify(Notice::warning(format!(
            "The project settings (custom types and commands) made with no project open gave way to those of {}",
            bt_utils::paths::display_name(found)
        )))]
    }
}
