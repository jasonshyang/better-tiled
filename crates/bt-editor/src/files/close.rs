//! Closing documents and quitting: unsaved work is a question, not a dialog, and the answer
//! comes back as an intent.

use std::path::Path;

use crate::document::DocumentId;
use crate::editor::Editor;
use crate::effect::{Choice, Decision, DecisionId, Effect, FileRequest, Purpose, Question};
use crate::files::{LoadSource, Pending};
use crate::notice::Notice;
use crate::recovery::Orphan;

impl Editor {
    /// Closes a document, asking first when closing would lose unsaved work.
    pub(crate) fn close_document(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return Vec::new();
        };
        let shown_elsewhere = self
            .documents
            .in_order()
            .any(|d| d.id() != doc && d.asset() == asset);
        let dirty = self.store.get(asset).is_some_and(|a| a.is_dirty());
        if dirty && !shown_elsewhere && !self.map_uses(asset) {
            let names = self
                .store
                .get(asset)
                .map(|a| a.display_name())
                .into_iter()
                .collect();
            return vec![self.ask(
                Question::Unsaved {
                    names,
                    purpose: Purpose::Close,
                },
                Pending::Close(doc),
            )];
        }
        self.close_now(doc)
    }

    /// Closes a document without asking: its asset goes when nothing else shows it.
    pub(crate) fn close_now(&mut self, doc: DocumentId) -> Vec<Effect> {
        let Some(document) = self.documents.remove(doc) else {
            return Vec::new();
        };
        self.forget_external(doc);
        let asset = document.asset();
        let mut effects = Vec::new();
        if let Some(path) = self
            .store
            .get(asset)
            .and_then(|a| a.path().map(Path::to_path_buf))
        {
            self.session.remember_view(
                &path,
                document.camera(),
                document.selection().current_layer,
            );
        }
        if !self.documents.in_order().any(|d| d.asset() == asset) && !self.map_uses(asset) {
            if let Some(path) = self.store.get(asset).and_then(|asset| asset.path()) {
                self.files.world_requested.remove(path);
            }
            self.store.remove(asset);
            self.files.loaders.remove(&asset);
            self.forget_rules(asset);
            self.recovery.forget(asset);
            effects.push(Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)));
            self.drop_unused_tilesets();
        }
        self.session_changed();
        effects
    }

    /// Whether an open map uses a tileset asset (which then stays, its changes saved with it).
    fn map_uses(&self, asset: crate::store::AssetId) -> bool {
        self.store
            .assets()
            .any(|other| self.store.dependencies_of(other.id()).contains(&asset))
    }

    /// Quits, asking first when anything is unsaved.
    pub(crate) fn quit(&mut self) -> Vec<Effect> {
        let unsaved = self.store.unsaved();
        if unsaved.is_empty() {
            return self.quit_now();
        }
        let names = unsaved
            .iter()
            .filter_map(|id| self.store.get(*id))
            .map(|asset| asset.display_name())
            .collect();
        vec![self.ask(
            Question::Unsaved {
                names,
                purpose: Purpose::Quit,
            },
            Pending::Quit,
        )]
    }

    /// Quits: the window closes; the session stays as it is for the next start.
    fn quit_now(&mut self) -> Vec<Effect> {
        let mut effects: Vec<Effect> = self
            .recovery
            .tracked()
            .into_iter()
            .map(|asset| Effect::DeleteSnapshot(self.recovery.snapshot_path(asset)))
            .collect();
        effects.push(Effect::Quit);
        effects
    }

    /// Continues a quit the user chose to save before: saves what has a file, asks where to
    /// save what has none, and quits once nothing is unsaved.
    pub(crate) fn continue_quit(&mut self) -> Vec<Effect> {
        let mut effects = self.save_all();
        if effects.iter().any(|effect| matches!(effect, Effect::Notify(n) if n.severity == crate::notice::Severity::Error)) {
            self.files.quitting = false;
            effects.push(Effect::Notify(Notice::error("Quitting stopped: something could not be saved")));
            return effects;
        }
        let untitled = self
            .documents
            .in_order()
            .find(|d| {
                self.store
                    .get(d.asset())
                    .is_some_and(|a| a.is_dirty() && a.path().is_none())
            })
            .map(|d| d.id());
        match untitled {
            Some(doc) => effects.push(self.save_as_request(doc)),
            None if self.store.unsaved().is_empty() => {
                self.files.quitting = false;
                effects.extend(self.quit_now());
            }
            None => {
                // Unsaved assets without a document (a tileset) were written by save_all; what
                // is left could not be.
                self.files.quitting = false;
                effects.push(Effect::Notify(Notice::error(
                    "Quitting stopped: something is still unsaved",
                )));
            }
        }
        effects
    }

    /// Asks a question, remembering what it is for.
    pub(crate) fn ask(&mut self, question: Question, pending: Pending) -> Effect {
        self.files.next_decision += 1;
        let id = DecisionId(self.files.next_decision);
        let choices = match &question {
            Question::Unsaved { .. } => vec![Choice::Save, Choice::DontSave, Choice::Cancel],
            Question::Recover { .. } => vec![Choice::Recover, Choice::Discard],
            Question::Conflict { .. } => vec![Choice::KeepMine, Choice::Reload],
        };
        self.files.pending.insert(id, pending);
        Effect::Ask(Decision {
            id,
            question,
            choices,
        })
    }

    /// Takes in an answer.
    pub(crate) fn decide(&mut self, decision: DecisionId, choice: Choice) -> Vec<Effect> {
        let Some(pending) = self.files.pending.remove(&decision) else {
            return Vec::new();
        };
        match pending {
            Pending::Close(doc) => match choice {
                Choice::Save => {
                    let mut effects = self.save(doc);
                    let saved = self
                        .documents
                        .get(doc)
                        .and_then(|d| self.store.get(d.asset()))
                        .is_some_and(|a| !a.is_dirty());
                    if saved {
                        effects.extend(self.close_now(doc));
                    }
                    effects
                }
                Choice::DontSave => self.close_now(doc),
                Choice::Cancel
                | Choice::Recover
                | Choice::Discard
                | Choice::KeepMine
                | Choice::Reload => Vec::new(),
            },
            Pending::Quit => match choice {
                Choice::Save => {
                    self.files.quitting = true;
                    self.continue_quit()
                }
                Choice::DontSave => self.quit_now(),
                Choice::Cancel
                | Choice::Recover
                | Choice::Discard
                | Choice::KeepMine
                | Choice::Reload => Vec::new(),
            },
            Pending::Recover(orphans) => match choice {
                Choice::Recover => self.recover(orphans),
                Choice::Discard => orphans
                    .into_iter()
                    .map(|orphan| Effect::DeleteSnapshot(orphan.snapshot))
                    .collect(),
                Choice::Save
                | Choice::DontSave
                | Choice::Cancel
                | Choice::KeepMine
                | Choice::Reload => Vec::new(),
            },
            Pending::Conflict {
                asset,
                content_hash,
            } => self.decide_conflict(asset, content_hash, choice),
        }
    }

    /// Asks whether to open an earlier run's snapshots again.
    pub(crate) fn offer_recovery(&mut self, orphans: Vec<Orphan>) -> Vec<Effect> {
        if orphans.is_empty() {
            return Vec::new();
        }
        let names = orphans.iter().map(Orphan::name).collect();
        vec![self.ask(Question::Recover { names }, Pending::Recover(orphans))]
    }

    /// Loads snapshots as unsaved maps.
    fn recover(&mut self, orphans: Vec<Orphan>) -> Vec<Effect> {
        orphans
            .into_iter()
            .map(|orphan| {
                let path = orphan.original_path();
                let mut job = self.load_job(path, LoadSource::Snapshot(orphan.snapshot));
                job.recovered_layers = orphan.meta.automap;
                Effect::Load(job)
            })
            .collect()
    }

    /// Takes in a picked file.
    pub(crate) fn file_picked(
        &mut self,
        request: FileRequest,
        path: Option<std::path::PathBuf>,
    ) -> Vec<Effect> {
        let Some(path) = path else {
            if self.files.quitting {
                self.files.quitting = false;
            }
            return Vec::new();
        };
        match request {
            FileRequest::StampsFolder => {
                let mut settings = self.settings().clone();
                settings.general.stamps_folder = Some(path);
                self.change_settings(settings)
            }
            FileRequest::GodotScene { doc, .. } => vec![Effect::Godot {
                doc,
                operation: crate::GodotOperation::ExportTo(path),
            }],
            FileRequest::Open => self.open(&path),
            // The window's own request: the New Tileset dialog takes the answer.
            FileRequest::AttributeImage { .. }
            | FileRequest::ExportImage { .. }
            | FileRequest::TilesetImage
            | FileRequest::ImportShortcuts { .. }
            | FileRequest::ExportShortcuts { .. } => Vec::new(),
            FileRequest::ExternalTileset(doc) => self.load_external_tileset(doc, &path),
            FileRequest::SaveAs { doc, .. } => {
                let mut effects = self.save_as(doc, &path);
                if self.files.quitting {
                    effects.extend(self.continue_quit());
                }
                effects
            }
            FileRequest::PropertyFile {
                doc,
                inspected,
                path: property,
            } => self.property_file_picked(doc, &inspected, property, &path),
        }
    }
}
