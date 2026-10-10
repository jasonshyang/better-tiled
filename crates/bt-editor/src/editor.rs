//! The editor without a window: what is open, how it changes, and what the window must do.
//! Three kinds of call make its surface: queries (`&self`) that panels read each frame,
//! [`Editor::apply`] for intents, and the effects it returns for the window to carry out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_core::{LayerId, LayerKind, Map};
use bt_tmx::WriteRequest;

use crate::actions::{Keymap, KeymapError, PaletteState};
use crate::clipboard::Clipboard;
use crate::document::{Document, DocumentId, Documents, Selection, ViewState, ViewToggles};
use crate::effect::Effect;
use crate::error::EditError;
use crate::files::Files;
use crate::history::{Gesture, History};
use crate::intent::Intent;
use crate::notice::Notice;
use crate::recovery::{RecoveryPlan, Snapshot, SnapshotMeta};
use crate::session::Session;
use crate::settings::Settings;
use crate::store::{AssetId, AssetStore, NewAsset, Root};
use crate::tools::{StrokeState, ToolState};
use crate::watch::WatchModel;

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
            stroke: None,
            keymap,
            tools: ToolState::default(),
            palette: PaletteState::default(),
            clipboard: Clipboard::default(),
            files: Files {
                home: self.home,
                ..Files::default()
            },
            recovery: RecoveryPlan::new(self.recovery_folder, self.pid),
            watch: WatchModel::default(),
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
    /// A tile tool's stroke under way.
    pub(crate) stroke: Option<StrokeState>,
    pub(crate) keymap: Keymap,
    pub(crate) tools: ToolState,
    pub(crate) palette: PaletteState,
    pub(crate) clipboard: Clipboard,
    pub(crate) files: Files,
    pub(crate) recovery: RecoveryPlan,
    pub(crate) watch: WatchModel,
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
            Intent::RunPalette(target) => self.run_palette(target),
            Intent::CreateMap(new) => self.create_map(&new),
            Intent::CreateTileset { doc, tileset } => self.create_tileset(doc, &tileset),
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
                self.select_layer(doc, layer);
                Vec::new()
            }
            Intent::SelectTiles { doc, region, mode } => {
                self.select_tiles(doc, &region, mode);
                Vec::new()
            }
            Intent::PickTiles { doc, cells } => self.pick_tiles(doc, &cells),
            Intent::Stroke(step) => self.stroke(step),
            Intent::Fill {
                doc,
                start,
                extra,
                whole_selection,
            } => self.fill(doc, start, &extra, whole_selection),
            Intent::ShapeFill { doc, region } => self.shape_fill(doc, &region),
            Intent::PickFromMap { doc, at } => self.pick_from_map(doc, at),
            Intent::Capture { doc, rect, cut } => self.capture(doc, rect, cut),
            Intent::SetSelectionMode(mode) => {
                self.tools.selection_mode = mode;
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
            Intent::Observed(observations) => self.observe(observations),
            Intent::ChangeSettings(settings) => self.change_settings(*settings),
            Intent::Tick { now_ms } => self.tick(now_ms),
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

    /// Takes new preferences: the key bindings must name commands and keys. The window writes
    /// them out.
    pub(crate) fn change_settings(&mut self, settings: Settings) -> Vec<Effect> {
        let keymap = match Keymap::from_settings(&settings.keymap, self.keymap.is_mac()) {
            Ok(keymap) => keymap,
            Err(error) => {
                return vec![Effect::Notify(Notice::error(format!(
                    "The preferences were not changed: {error}"
                )))];
            }
        };
        if settings == self.settings {
            return Vec::new();
        }
        self.keymap = keymap;
        self.settings = settings;
        for document in self.documents.iter_mut() {
            document
                .history
                .set_limit(self.settings.general.history_limit);
        }
        vec![Effect::WriteSettings(Box::new(self.settings.clone()))]
    }

    /// The tool and its options.
    pub fn tools(&self) -> &ToolState {
        &self.tools
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

pub(crate) fn warn(error: EditError) -> Effect {
    Effect::Notify(Notice::warning(error.to_string()))
}

/// The topmost layer that is not a group, depth first.
pub(crate) fn topmost_leaf(map: &Map) -> Option<LayerId> {
    let mut found = None;
    map.for_each_layer(|layer, _| {
        if !matches!(layer.kind, LayerKind::Group(_)) {
            found = Some(layer.id);
        }
    });
    found
}
