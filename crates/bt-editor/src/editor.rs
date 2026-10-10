//! The editor without a window: what is open, how it changes, and what the window must do.
//! Three kinds of call make its surface: queries (`&self`) that panels read each frame,
//! [`Editor::apply`] for intents, and the effects it returns for the window to carry out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{LayerId, LayerKind, Map};
use bt_tmx::WriteRequest;

use crate::actions::{ActionId, Context, Keymap, KeymapError};
use crate::document::{Document, DocumentId, Documents, Selection, ViewState, ViewToggles};
use crate::effect::{Effect, FileRequest, ViewRequest};
use crate::error::EditError;
use crate::files::Files;
use crate::history::{Gesture, History};
use crate::intent::Intent;
use crate::notice::Notice;
use crate::recovery::{RecoveryPlan, Snapshot, SnapshotMeta};
use crate::session::Session;
use crate::settings::Settings;
use crate::store::{AssetId, AssetStore, NewAsset, Root};

/// Whether a command can run now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enabled {
    /// It can.
    Enabled,
    /// It cannot; the reason, when a key that does nothing would otherwise be a mystery.
    Disabled(Option<String>),
}

/// Sets up an [`Editor`].
#[derive(Clone, Debug)]
pub struct EditorBuilder {
    settings: Settings,
    recovery_folder: PathBuf,
    pid: u32,
    mac: bool,
    home: Option<PathBuf>,
}

impl EditorBuilder {
    /// The preferences (the defaults otherwise).
    pub fn settings(mut self, settings: Settings) -> EditorBuilder {
        self.settings = settings;
        self
    }

    /// Where recovery snapshots go (`<data>/recovery`).
    pub fn recovery_folder(mut self, folder: PathBuf) -> EditorBuilder {
        self.recovery_folder = folder;
        self
    }

    /// This run's process id, which names its snapshots.
    pub fn pid(mut self, pid: u32) -> EditorBuilder {
        self.pid = pid;
        self
    }

    /// Whether key bindings are macOS's.
    pub fn mac(mut self, mac: bool) -> EditorBuilder {
        self.mac = mac;
        self
    }

    /// The user's home folder: projects are not looked for at or above it.
    pub fn home(mut self, home: Option<PathBuf>) -> EditorBuilder {
        self.home = home;
        self
    }

    /// The editor; the settings' key bindings must name commands and keys.
    pub fn build(self) -> Result<Editor, KeymapError> {
        let keymap = Keymap::from_settings(&self.settings.keymap, self.mac)?;
        Ok(self.assemble(keymap))
    }

    fn assemble(self, keymap: Keymap) -> Editor {
        Editor {
            store: AssetStore::new(),
            documents: Documents::default(),
            gesture: None,
            keymap,
            files: Files {
                home: self.home,
                ..Files::default()
            },
            recovery: RecoveryPlan::new(self.recovery_folder, self.pid),
            session: Session::default(),
            session_generation: 0,
            restore_current: None,
            settings: self.settings,
        }
    }
}

/// The editor's state. Every call runs on the thread that owns it; work that blocks is the
/// caller's to run elsewhere, and its results come back as intents.
pub struct Editor {
    pub(crate) store: AssetStore,
    pub(crate) documents: Documents,
    pub(crate) gesture: Option<Gesture>,
    pub(crate) keymap: Keymap,
    pub(crate) files: Files,
    pub(crate) recovery: RecoveryPlan,
    pub(crate) session: Session,
    session_generation: u64,
    /// The file that was current in a session being restored, kept current as the others load.
    restore_current: Option<PathBuf>,
    pub(crate) settings: Settings,
}

impl Editor {
    /// A builder with the default settings, snapshots in the system's temporary folder, and
    /// this platform's key bindings.
    pub fn builder() -> EditorBuilder {
        EditorBuilder {
            settings: Settings::default(),
            recovery_folder: std::env::temp_dir().join("better-tiled-recovery"),
            pid: std::process::id(),
            mac: cfg!(target_os = "macos"),
            home: None,
        }
    }

    /// An editor with nothing open, the default settings and the default key bindings.
    pub fn new() -> Editor {
        let builder = Editor::builder();
        let keymap = Keymap::new(builder.mac);
        builder.assemble(keymap)
    }

    /// Applies an intent; what the window must do in return.
    pub fn apply(&mut self, intent: Intent) -> Vec<Effect> {
        match intent {
            Intent::Run(id) => self.run(id),
            Intent::Open(path) => self.open(&path),
            Intent::Loaded(loaded) => {
                let recovered = loaded.recovered_from.is_some();
                let effects = self.finish_load(*loaded);
                if recovered {
                    // Work the user chose to recover is what they want to see.
                    self.restore_current = None;
                } else {
                    self.keep_restored_current();
                }
                effects
            }
            Intent::FilePicked { request, path } => self.file_picked(request, path),
            Intent::Decided { decision, choice } => self.decide(decision, choice),
            Intent::SelectDocument(doc) => {
                self.documents.set_current(doc);
                self.restore_current = None;
                self.session_changed();
                Vec::new()
            }
            Intent::CloseDocument(doc) => self.close_document(doc),
            Intent::SelectLayer { doc, layer } => {
                if let Some(document) = self.documents.get_mut(doc) {
                    document.selection = Selection::of_layer(layer);
                }
                self.session_changed();
                Vec::new()
            }
            Intent::SetLayerVisible {
                doc,
                layer,
                visible,
            } => {
                let label = if visible { "Show Layer" } else { "Hide Layer" };
                self.edit_layer(doc, layer, label, |l| l.visible = visible)
            }
            Intent::SetLayerLocked { doc, layer, locked } => {
                let label = if locked { "Lock Layer" } else { "Unlock Layer" };
                self.edit_layer(doc, layer, label, |l| l.locked = locked)
            }
            Intent::RenameLayer { doc, layer, name } => {
                self.edit_layer(doc, layer, "Rename Layer", |l| l.name = name)
            }
            Intent::SetCamera { doc, view } => {
                if let Some(document) = self.documents.get_mut(doc) {
                    document.camera = Some(view);
                }
                self.session_changed();
                Vec::new()
            }
            Intent::Recoverable(orphans) => self.offer_recovery(orphans),
            Intent::Tick { now_ms } => self.tick(now_ms),
        }
    }

    /// Runs a command.
    fn run(&mut self, id: ActionId) -> Vec<Effect> {
        if let Enabled::Disabled(reason) = self.enabled(id) {
            return reason
                .map(|r| Effect::Notify(Notice::info(r)))
                .into_iter()
                .collect();
        }
        let current = self.documents.current();
        let on_document = |editor: &mut Editor, f: fn(&mut Editor, DocumentId) -> Vec<Effect>| {
            current.map(|doc| f(editor, doc)).unwrap_or_default()
        };
        match id {
            ActionId::Open => vec![Effect::PickFile(FileRequest::Open)],
            ActionId::Save => on_document(self, Editor::save),
            ActionId::SaveAs => on_document(self, |editor, doc| vec![editor.save_as_request(doc)]),
            ActionId::SaveAll => {
                let mut effects = self.save_all();
                let untitled = self
                    .documents
                    .in_order()
                    .find(|d| {
                        self.store
                            .get(d.asset())
                            .is_some_and(|a| a.is_dirty() && a.path().is_none())
                    })
                    .map(|d| d.id());
                if let Some(doc) = untitled {
                    effects.push(self.save_as_request(doc));
                }
                effects
            }
            ActionId::Close => on_document(self, Editor::close_document),
            ActionId::Quit => self.quit(),
            ActionId::Undo => on_document(self, |editor, doc| {
                editor.undo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::Redo => on_document(self, |editor, doc| {
                editor.redo(doc).err().map(warn).into_iter().collect()
            }),
            ActionId::ZoomIn => vec![Effect::View(ViewRequest::ZoomIn)],
            ActionId::ZoomOut => vec![Effect::View(ViewRequest::ZoomOut)],
            ActionId::ZoomNormal => vec![Effect::View(ViewRequest::ZoomNormal)],
            ActionId::FitInView => vec![Effect::View(ViewRequest::Fit)],
            ActionId::ShowGrid => {
                self.toggle_view(|view| view.grid = !view.grid);
                Vec::new()
            }
            ActionId::HighlightCurrentLayer => {
                self.toggle_view(|view| {
                    view.highlight_current_layer = !view.highlight_current_layer
                });
                Vec::new()
            }
            ActionId::NextDocument | ActionId::PreviousDocument => {
                let step = if id == ActionId::NextDocument { 1 } else { -1 };
                if let Some(doc) = self.documents.neighbour(step) {
                    self.documents.set_current(doc);
                    self.session_changed();
                }
                Vec::new()
            }
        }
    }

    /// Whether a command can run now.
    pub fn enabled(&self, id: ActionId) -> Enabled {
        let document = self
            .documents
            .current()
            .and_then(|doc| self.documents.get(doc));
        if id.spec().context == Context::Document && document.is_none() {
            return Enabled::Disabled(None);
        }
        let history = document.map(Document::history);
        match id {
            ActionId::Undo if history.is_none_or(|h| h.undoable().is_none()) => {
                Enabled::Disabled(Some("There is nothing to undo".to_owned()))
            }
            ActionId::Redo if history.is_none_or(|h| h.redoable().is_none()) => {
                Enabled::Disabled(Some("There is nothing to redo".to_owned()))
            }
            ActionId::NextDocument | ActionId::PreviousDocument if self.documents.len() < 2 => {
                Enabled::Disabled(None)
            }
            _ => Enabled::Enabled,
        }
    }

    /// Whether a toggle is on; `None` for commands that are not toggles.
    pub fn checked(&self, id: ActionId) -> Option<bool> {
        let view = self
            .documents
            .current()
            .and_then(|doc| self.documents.get(doc))
            .map(Document::view)
            .unwrap_or_default();
        match id {
            ActionId::ShowGrid => Some(view.grid),
            ActionId::HighlightCurrentLayer => Some(view.highlight_current_layer),
            _ => None,
        }
    }

    fn toggle_view(&mut self, toggle: impl FnOnce(&mut ViewToggles)) {
        if let Some(doc) = self.documents.current()
            && let Some(document) = self.documents.get_mut(doc)
        {
            toggle(&mut document.view);
        }
    }

    /// Changes a layer as a transaction.
    fn edit_layer(
        &mut self,
        doc: DocumentId,
        layer: LayerId,
        label: &str,
        change: impl FnOnce(&mut bt_core::Layer),
    ) -> Vec<Effect> {
        let Some(asset) = self.documents.get(doc).map(Document::asset) else {
            return Vec::new();
        };
        let result = self.transact(doc, label, |tx| {
            let target = tx
                .map_mut(asset)?
                .layer_mut(layer)
                .ok_or(EditError::NoSuchLayer(layer))?;
            change(target);
            Ok::<_, EditError>(())
        });
        result.err().map(warn).into_iter().collect()
    }

    /// The recovery schedule: snapshots due and snapshots to delete.
    fn tick(&mut self, now_ms: u64) -> Vec<Effect> {
        if !self.settings.general.recovery {
            return Vec::new();
        }
        let due = self.recovery.due(&self.store, now_ms);
        let mut effects: Vec<Effect> = due
            .delete
            .iter()
            .map(|id| Effect::DeleteSnapshot(self.recovery.snapshot_path(*id)))
            .collect();
        effects.extend(
            due.snapshot
                .into_iter()
                .filter_map(|id| self.snapshot_of(id))
                .map(Effect::WriteSnapshot),
        );
        effects
    }

    /// The run is ending without a quit anyone answered (the system ended it: the app menu, the
    /// Dock, a logout): a snapshot of every unsaved map whose latest change has none yet, for the
    /// window to write before it goes. They are kept, so the next start offers them back.
    pub fn snapshots_for_unanswered_exit(&mut self) -> Vec<Snapshot> {
        if !self.settings.general.recovery {
            return Vec::new();
        }
        self.recovery
            .take_unsnapshotted(&self.store)
            .into_iter()
            .filter_map(|id| self.snapshot_of(id))
            .collect()
    }

    /// A snapshot of what Save would write for a map.
    fn snapshot_of(&self, id: AssetId) -> Option<Snapshot> {
        let asset = self.store.get(id)?;
        let map = self.store.map(id)?;
        let original = asset.path().map(Path::to_path_buf);
        let path_for_references = original
            .clone()
            .unwrap_or_else(|| PathBuf::from("untitled.tmx"));
        let mut request = WriteRequest::for_map(map, &path_for_references);
        if let Some(loader) = self.files.loaders.get(&id) {
            request = request.loader(loader);
        }
        match request.render() {
            Ok(rendered) => Some(Snapshot {
                path: self.recovery.snapshot_path(id),
                text: rendered.text,
                meta: SnapshotMeta {
                    original,
                    pid: self.recovery.pid(),
                },
            }),
            Err(error) => {
                log::warn!("no recovery snapshot of {}: {error}", asset.display_name());
                None
            }
        }
    }

    /// The file this run's heartbeat goes in.
    pub fn heartbeat_path(&self) -> PathBuf {
        self.recovery.heartbeat_path()
    }

    /// The open assets.
    pub fn store(&self) -> &AssetStore {
        &self.store
    }

    /// The open documents.
    pub fn documents(&self) -> &Documents {
        &self.documents
    }

    /// A document.
    pub fn document(&self, doc: DocumentId) -> Option<&Document> {
        self.documents.get(doc)
    }

    /// The current document.
    pub fn current(&self) -> Option<DocumentId> {
        self.documents.current()
    }

    /// The map a document shows.
    pub fn map_of(&self, doc: DocumentId) -> Option<&Arc<Map>> {
        self.store.map(self.documents.get(doc)?.asset)
    }

    /// The key bindings.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The preferences.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether any asset is unsaved (the window's quit guard reads this).
    pub fn has_unsaved(&self) -> bool {
        !self.store.unsaved().is_empty()
    }

    /// The session as it is now: the open files and their view states, with what was kept.
    pub fn session(&self) -> Session {
        let mut session = self.session.clone();
        session.open_files = self
            .documents
            .in_order()
            .filter_map(|doc| self.store.get(doc.asset())?.path().map(Path::to_path_buf))
            .collect();
        session.current = self.documents.current().and_then(|doc| self.path_of(doc));
        for doc in self.documents.in_order() {
            if let Some(path) = self.store.get(doc.asset()).and_then(|a| a.path()) {
                session.remember_view(path, doc.camera(), doc.selection().current_layer);
            }
        }
        session
    }

    /// Bumped whenever the session changes, so the window knows to write it.
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// Keeps the window's panel layout in the session.
    pub fn set_layout(&mut self, layout: serde_json::Value) {
        if self.session.layout != layout {
            self.session.layout = layout;
            self.session_changed();
        }
    }

    /// Starts from a session: its recent files, view states and layout, and loads of its files.
    pub fn restore_session(&mut self, session: Session) -> Vec<Effect> {
        let open = session.open_files.clone();
        self.restore_current = session.current.clone();
        self.session = session;
        open.iter().flat_map(|path| self.open(path)).collect()
    }

    /// While a restored session's files load, its current file stays current.
    fn keep_restored_current(&mut self) {
        let Some(path) = self.restore_current.clone() else {
            return;
        };
        let Some(asset) = self.store.by_path(&path) else {
            return;
        };
        let found = self
            .documents
            .in_order()
            .find(|d| d.asset() == asset)
            .map(Document::id);
        if let Some(doc) = found {
            self.documents.set_current(doc);
        }
    }

    pub(crate) fn session_changed(&mut self) {
        self.session_generation += 1;
    }

    pub(crate) fn remember_recent(&mut self, path: &Path) {
        self.session.remember_recent(path);
    }

    /// Opens a document onto a map asset, current, with its topmost layer selected.
    pub(crate) fn open_document(&mut self, asset: AssetId) -> DocumentId {
        let id = self.documents.next_id();
        let current_layer = self.store.map(asset).and_then(|map| topmost_leaf(map));
        let mut history = History::default();
        history.set_limit(self.settings.general.history_limit);
        self.documents.add(Document {
            id,
            asset,
            selection: current_layer.map(Selection::of_layer).unwrap_or_default(),
            camera: None,
            history,
            view: ViewToggles::default(),
        });
        id
    }

    /// Opens a new, unsaved map in a document.
    pub fn open_new_map(&mut self, map: Map) -> DocumentId {
        let asset = self.store.insert(NewAsset {
            root: Root::Map(Arc::new(map)),
            path: None,
            content_hash: None,
            warnings: Vec::new(),
        });
        self.session_changed();
        self.open_document(asset)
    }

    /// The file of a document's asset.
    pub fn path_of(&self, doc: DocumentId) -> Option<PathBuf> {
        let asset = self.documents.get(doc)?.asset;
        self.store.get(asset)?.path().map(PathBuf::from)
    }

    /// Where a document's camera looks, for the window to restore.
    pub fn camera_of(&self, doc: DocumentId) -> Option<ViewState> {
        self.documents.get(doc)?.camera()
    }
}

impl Default for Editor {
    fn default() -> Editor {
        Editor::new()
    }
}

fn warn(error: EditError) -> Effect {
    Effect::Notify(Notice::warning(error.to_string()))
}

/// The topmost layer that is not a group, depth first.
fn topmost_leaf(map: &Map) -> Option<LayerId> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if !matches!(layer.kind, LayerKind::Group(_)) {
            found = Some(layer.id);
        }
    });
    found
}
