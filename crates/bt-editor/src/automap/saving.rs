//! AutoMap layers kept with the map (`05-editor-core.md`, §8 and §16): a map with them on is
//! saved as the rules' result, and its project file (`better-tiled.json`) keeps the setup and
//! what was painted, so opening the map again brings back what was painted; with them off, a
//! record left from before is removed.

use std::path::Path;
use std::sync::Arc;

use bt_automap::layers::{AutoLayers, Restored, painted_record};
use bt_core::Map;
use bt_project::{AutoMapRecord, EntryPlace, ProjectFile, paths};

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::Effect;
use crate::notice::Notice;
use crate::store::AssetId;

/// What a map with AutoMap layers on is saved from.
#[derive(Clone, Debug)]
pub(crate) struct LayersToSave {
    /// The layers: the rules' result over what was painted, or what was painted alone.
    pub(crate) layers: AutoLayers,
    /// Whether the rules' result is in them: not while the rules are being read again or cannot
    /// be read, when what was painted is saved as it is.
    pub(crate) with_result: bool,
}

impl Editor {
    /// What the map `asset` is saved from while its AutoMap layers are on; `None` while they are
    /// off. The rules' result counts only if it was made from the map as it is now.
    pub(crate) fn layers_to_save(&self, asset: AssetId) -> Option<LayersToSave> {
        let document = self.documents.in_order().find(|d| d.asset() == asset)?;
        document.automap.as_ref()?;
        let painted = self.store.map(asset)?;
        let live = self
            .automap
            .live
            .get(&document.id())
            .filter(|live| Arc::ptr_eq(live.layers.painted(), painted));
        Some(match live {
            Some(live) => LayersToSave {
                layers: live.layers.clone(),
                with_result: true,
            },
            None => LayersToSave {
                layers: AutoLayers::painted_only(Arc::clone(painted)),
                with_result: false,
            },
        })
    }

    /// Writes the AutoMap record of the map `asset`, saved at `map_path`, into its project file:
    /// `save` while its AutoMap layers are on; with them off, removes a record left from before.
    pub(crate) fn write_layers_record(
        &self,
        asset: AssetId,
        map_path: &Path,
        save: Option<&LayersToSave>,
    ) -> Result<(), String> {
        let home = self.files.home.as_deref();
        let setup = self
            .documents
            .in_order()
            .find(|d| d.asset() == asset)
            .and_then(|d| d.automap.as_ref());
        let (Some(setup), Some(save)) = (setup, save) else {
            let Some(file) = ProjectFile::find(map_path, home).map_err(|e| e.to_string())? else {
                return Ok(());
            };
            let key = paths::key(&file, map_path).map_err(|e| e.to_string())?;
            let kept = ProjectFile::load(&file).map_err(|e| e.to_string())?;
            if kept.automap(&key).is_none() {
                return Ok(());
            }
            let lock = self.lock_folder()?;
            return ProjectFile::edit(&file, lock, |document| {
                document.remove_automap(&key);
            })
            .map_err(|e| e.to_string());
        };
        let place =
            EntryPlace::for_map(map_path, self.project_path(), home).map_err(|e| e.to_string())?;
        let rules = paths::key(&place.file, &setup.rules).map_err(|e| e.to_string())?;
        let record = painted_record(setup, rules, &save.layers);
        let lock = self.lock_folder()?;
        ProjectFile::edit(&place.file, lock, |document| {
            document.set_automap(&place.key, record);
        })
        .map_err(|e| e.to_string())
    }

    /// Save As: the map's entry in its project file moves to the new place (inside one file, or
    /// from one file to another), so what was painted under its AutoMap layers goes along.
    pub(crate) fn move_layers_record(&self, asset: AssetId, to: &Path) -> Vec<Effect> {
        let Some(from) = self.store.get(asset).and_then(|a| a.path()) else {
            return Vec::new();
        };
        if from == to {
            return Vec::new();
        }
        match self.move_entry(from, to) {
            Ok(_) => Vec::new(),
            Err(problem) => vec![Effect::Notify(Notice::warning(format!(
                "The project file's entry for {} did not move with it: {problem}",
                bt_utils::paths::display_name(from)
            )))],
        }
    }

    /// Moves the project file's entry of the map at `from` to `to`; whether there was one.
    fn move_entry(&self, from: &Path, to: &Path) -> Result<bool, String> {
        let home = self.files.home.as_deref();
        let Some(file) = ProjectFile::find(from, home).map_err(|e| e.to_string())? else {
            return Ok(false);
        };
        let key = paths::key(&file, from).map_err(|e| e.to_string())?;
        if ProjectFile::load(&file)
            .map_err(|e| e.to_string())?
            .entry(&key)
            .is_none()
        {
            return Ok(false);
        }
        let from = EntryPlace { file, key };
        let to = EntryPlace::for_map(to, self.project_path(), home).map_err(|e| e.to_string())?;
        ProjectFile::move_entry(&from, &to, self.lock_folder()?).map_err(|e| e.to_string())
    }

    /// Puts AutoMap layers read with a map into its new document: the setup, and the map as its
    /// file holds it (none for recovered work, which is unsaved anyway), to check against the
    /// first result; entries that did not apply are said.
    pub(crate) fn restore_layers(
        &mut self,
        doc: DocumentId,
        restored: Restored,
        saved: Option<Arc<Map>>,
    ) -> Vec<Effect> {
        if let Some(document) = self.documents.get_mut(doc) {
            document.automap = Some(restored.setup);
        }
        if let Some(saved) = saved {
            self.automap.saved.insert(doc, saved);
        }
        if let Some(asset) = self.document(doc).map(|doc| doc.asset()) {
            self.automap
                .restoration
                .insert(asset, restored.notes.clone());
        }
        let Some(first) = restored.notes.first() else {
            return Vec::new();
        };
        let details: Vec<&str> = restored.notes.iter().map(|n| n.text.as_str()).collect();
        vec![Effect::Notify(
            Notice::warning(format!("AutoMap layers: {}", first.text))
                .with_detail(details.join("\n")),
        )]
    }

    /// The setup of the map `asset`'s AutoMap layers for its recovery snapshot, which holds what
    /// was painted: the record the project file would keep, with the rules file's full path and
    /// no painted entries.
    pub(crate) fn snapshot_layers(&self, asset: AssetId) -> Option<AutoMapRecord> {
        let setup = self
            .documents
            .in_order()
            .find(|d| d.asset() == asset)?
            .automap
            .as_ref()?;
        let painted = AutoLayers::painted_only(Arc::clone(self.store.map(asset)?));
        let rules = setup.rules.to_string_lossy().into_owned();
        Some(painted_record(setup, rules, &painted))
    }

    /// The AutoMap layers of the map document `doc` shows, while they are on, as the project
    /// file would keep them for the map as it shows, the rules file a full path: what `bt`
    /// rebuilds what was painted from, beside the map's text.
    pub fn layers_record(&self, doc: DocumentId) -> Option<AutoMapRecord> {
        let document = self.documents.get(doc)?;
        let setup = document.automap.as_ref()?;
        let save = self.layers_to_save(document.asset())?;
        let rules = setup.rules.to_string_lossy().into_owned();
        Some(painted_record(setup, rules, &save.layers))
    }

    /// Where project files are locked while they are written.
    fn lock_folder(&self) -> Result<&Path, String> {
        self.files
            .lock_folder
            .as_deref()
            .ok_or_else(|| "there is no folder to lock the project file in".to_owned())
    }
}

/// The AutoMap layers the project file keeps for the map at `map_path` (its canonical path, as
/// the editor names the map's asset), rebuilt on `saved`, the map as its file holds it; `None`
/// when no project file keeps any for it.
pub(crate) fn read_layers(
    map_path: &Path,
    home: Option<&Path>,
    saved: &Map,
) -> Result<Option<Restored>, String> {
    bt_automap::layers::read_from_project(map_path, home, saved).map_err(|e| e.to_string())
}
