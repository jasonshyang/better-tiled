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

/// The workspace requested after the current project's save/discard decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProjectSwitch {
    Open(std::path::PathBuf),
    New(std::path::PathBuf),
    Close,
}

impl Editor {
    pub(crate) fn compatibility_version(&self) -> bt_core::CompatibilityVersion {
        self.project_asset()
            .and_then(|id| self.store.project(id))
            .map_or(bt_core::CompatibilityVersion::Latest, |project| {
                project.compatibility_version
            })
    }

    /// The current project asset, including an untitled project.
    pub fn project_asset(&self) -> Option<AssetId> {
        self.files
            .project
            .filter(|id| self.store.project(*id).is_some())
    }

    /// The project settings document, opened on demand so project edits have normal history.
    pub(crate) fn show_project_document(&mut self) -> Option<crate::DocumentId> {
        let project = self.project_asset()?;
        let shown = self
            .documents
            .in_order()
            .find(|doc| doc.asset() == project)
            .map(|doc| doc.id());
        if let Some(doc) = shown {
            self.documents.set_current(doc);
            Some(doc)
        } else {
            Some(self.open_document(project))
        }
    }

    pub(crate) fn set_project(
        &mut self,
        doc: crate::DocumentId,
        project: AssetId,
        value: bt_core::Project,
    ) -> Vec<Effect> {
        if self.files.project != Some(project) {
            return vec![Effect::Notify(Notice::error(
                "That project is no longer open",
            ))];
        }
        self.transact(doc, "Edit Project Properties", |tx| {
            *tx.project_mut(project)? = value;
            Ok::<_, crate::EditError>(())
        })
        .err()
        .map(|error| Effect::Notify(Notice::error(error.to_string())))
        .into_iter()
        .collect()
    }

    pub(crate) fn add_project_folder(&mut self, project: AssetId, path: &Path) -> Vec<Effect> {
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => return vec![Effect::Notify(Notice::error(error.to_string()))],
        };
        let Some(source) = self.store.project(project) else {
            return Vec::new();
        };
        if source.folders.iter().any(|folder| folder.path == path) {
            return Vec::new();
        }
        let mut value = (**source).clone();
        let name = self
            .project_path()
            .and_then(Path::parent)
            .map(|folder| bt_utils::paths::reference_path(folder, &path))
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        value.folders.push(bt_core::FileRef::new(name, path));
        let Some(doc) = self.current().or_else(|| self.show_project_document()) else {
            return Vec::new();
        };
        self.set_project(doc, project, value)
    }

    pub(crate) fn remove_project_folder(&mut self, project: AssetId, path: &Path) -> Vec<Effect> {
        let Some(source) = self.store.project(project) else {
            return Vec::new();
        };
        let mut value = (**source).clone();
        value.folders.retain(|folder| folder.path != path);
        let Some(doc) = self.current().or_else(|| self.show_project_document()) else {
            return Vec::new();
        };
        self.set_project(doc, project, value)
    }

    pub(crate) fn begin_project_switch(&mut self, destination: ProjectSwitch) -> Vec<Effect> {
        if let ProjectSwitch::Open(path) = &destination
            && self.project_path() == Some(path.as_path())
        {
            self.show_project_document();
            return Vec::new();
        }
        let mut unsaved = self.store.unsaved();
        if let Some(project) = self.project_asset().filter(|id| {
            self.store
                .get(*id)
                .is_some_and(|asset| asset.path().is_none())
        }) && self.store.project(project).is_some_and(|project| {
            !project.custom_types.is_empty()
                || !project.commands.is_empty()
                || !project.properties.is_empty()
                || !project.folders.is_empty()
        }) {
            unsaved.push(project);
        }
        if unsaved.is_empty() {
            return self.finish_project_switch(destination);
        }
        let names = unsaved
            .into_iter()
            .filter_map(|id| self.store.get(id).map(|asset| asset.display_name()))
            .collect();
        vec![self.ask(
            crate::Question::Unsaved {
                names,
                purpose: crate::Purpose::SwitchProject,
            },
            super::Pending::ProjectSwitch(destination),
        )]
    }

    pub(crate) fn continue_project_switch(&mut self) -> Vec<Effect> {
        let mut effects = self.save_all();
        if effects.iter().any(|effect| matches!(effect, Effect::Notify(notice) if notice.severity == crate::Severity::Error)) {
            self.files.switching_project = None;
            effects.push(Effect::Notify(Notice::error("The project switch stopped because something could not be saved")));
            return effects;
        }
        if let Some(doc) = self
            .documents
            .in_order()
            .find(|doc| {
                self.store.get(doc.asset()).is_some_and(|asset| {
                    asset.is_dirty()
                        && asset.path().is_none()
                        && asset.kind() != crate::AssetKind::Project
                })
            })
            .map(|doc| doc.id())
        {
            effects.push(self.save_as_request(doc));
            return effects;
        }
        if let Some(project) = self.project_asset().filter(|id| {
            self.store
                .get(*id)
                .is_some_and(|asset| asset.path().is_none())
        }) {
            effects.push(Effect::PickFile(crate::FileRequest::ProjectAs(project)));
            return effects;
        }
        if let Some(destination) = self.files.switching_project.take() {
            effects.extend(self.finish_project_switch(destination));
        }
        effects
    }

    pub(crate) fn finish_project_switch(&mut self, destination: ProjectSwitch) -> Vec<Effect> {
        self.remember_project_documents();
        self.files.epoch += 1;
        self.restore_current = None;
        self.files.restore_project = None;
        self.files.switching_project = None;
        let mut effects = Vec::new();
        for doc in self.documents.ids() {
            effects.extend(self.close_now(doc));
        }
        let assets = self
            .store
            .assets()
            .map(|asset| asset.id())
            .collect::<Vec<_>>();
        for asset in assets {
            self.store.remove(asset);
            self.recovery.forget(asset);
            effects.push(Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)));
        }
        self.files.project = None;
        self.files.loaders.clear();
        self.files.active_world = None;
        self.files.world_patterns.clear();
        self.files.world_pattern_requests.clear();
        self.files.world_pattern_results.clear();
        self.files.world_requested.clear();
        self.files.project_scan = None;
        self.files.project_scan_request = None;
        self.tools.template = None;
        match destination {
            ProjectSwitch::Open(path) => {
                effects.push(Effect::Load(self.load_job(path, super::LoadSource::File)))
            }
            ProjectSwitch::New(path) => {
                let path = match bt_utils::paths::absolute_path(&path) {
                    Ok(path) => path,
                    Err(error) => {
                        effects.push(Effect::Notify(Notice::error(error.to_string())));
                        return effects;
                    }
                };
                let Some(folder) = path.parent() else {
                    return effects;
                };
                let value = bt_core::Project {
                    folders: vec![bt_core::FileRef::new(".", folder)],
                    extensions_path: bt_core::FileRef::new("extensions", folder.join("extensions")),
                    ..bt_core::Project::default()
                };
                let asset = self.store.insert(NewAsset {
                    root: Root::Project(Arc::new(value)),
                    path: Some(path.clone()),
                    content_hash: None,
                    warnings: Vec::new(),
                });
                self.files.project = Some(asset);
                self.show_project_document();
                self.remember_recent(&path);
                if let Err(notice) = self.write_auxiliary(asset) {
                    effects.push(Effect::Notify(notice));
                }
            }
            ProjectSwitch::Close => {}
        }
        self.session_changed();
        effects
    }

    pub(crate) fn save_project_as(&mut self, project: AssetId, path: &Path) -> Vec<Effect> {
        if self.files.project != Some(project) {
            return vec![Effect::Notify(Notice::error(
                "The project that opened Save As is no longer open",
            ))];
        }
        let path = match bt_utils::paths::absolute_path(path) {
            Ok(path) => path,
            Err(error) => return vec![Effect::Notify(Notice::error(error.to_string()))],
        };
        if self.store.by_path(&path).is_some_and(|id| id != project) {
            return vec![Effect::Notify(Notice::error(
                "That path is already used by another loaded asset",
            ))];
        }
        let Some(source) = self.store.project(project) else {
            return Vec::new();
        };
        let mut value = (**source).clone();
        if value.extensions_path.path.as_os_str().is_empty()
            && let Some(folder) = path.parent()
        {
            value.extensions_path.path = folder.join(&value.extensions_path.source);
        }
        if let Some(from) = self.project_path().and_then(Path::parent)
            && let Some(to) = path.parent()
        {
            bt_tmx::rebase_project_references(&mut value, from, to);
        }
        let Some(doc) = self.show_project_document() else {
            return Vec::new();
        };
        if let Err(error) = self.transact(doc, "Save Project As", |tx| {
            *tx.project_mut(project)? = value;
            Ok::<_, crate::EditError>(())
        }) {
            return vec![Effect::Notify(Notice::error(error.to_string()))];
        }
        self.store.retarget(project, path.clone());
        self.remember_recent(&path);
        self.session_changed();
        match self.write_auxiliary(project) {
            Ok(()) => vec![Effect::DeleteSnapshot(self.recovery.snapshot_path(project))],
            Err(notice) => vec![Effect::Notify(notice)],
        }
    }

    fn remember_project_documents(&mut self) {
        let Some(path) = self.project_path().map(Path::to_path_buf) else {
            return;
        };
        let session = self.session();
        self.session.projects.retain(|project| project.path != path);
        self.session.projects.push(crate::ProjectSession {
            path,
            open_files: session.open_files,
            current: session.current,
            worlds: session.worlds,
        });
    }

    pub(super) fn restore_project_documents(&mut self, path: &Path) -> Vec<Effect> {
        if let Some(session) = self.files.restore_project.take() {
            let mut effects = Vec::new();
            for world in session.worlds {
                effects.extend(self.open(&world));
            }
            for file in session.open_files {
                effects.extend(self.open(&file));
            }
            return effects;
        }
        let Some(project) = self
            .session
            .projects
            .iter()
            .find(|project| project.path == path)
            .cloned()
        else {
            return Vec::new();
        };
        self.restore_current = project.current;
        let mut effects = Vec::new();
        for world in project.worlds {
            effects.extend(self.open(&world));
        }
        for file in project.open_files {
            effects.extend(self.open(&file));
        }
        effects
    }
}
