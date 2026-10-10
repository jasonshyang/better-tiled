//! The window's state and its frame. `logic` delivers what the workers finished and ticks the
//! editor (recovery snapshots), even while the window is hidden; `ui` handles a request to
//! close, routes the keyboard, draws the menu bar, status bar, panels, tabs and canvas and the
//! modals, then applies the intents the frame collected, in order, and carries out the effects
//! the editor returns.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bt_editor::{ActionId, DocumentId, Editor, FileRequest, Intent, Notice, Severity};
use bt_image::ImageStore;

use super::menu::menu_bar;
use super::startup::{RunOutcome, Startup};
use super::status_bar::{StatusInput, status_bar};
use super::tabs::tab_strip;
use super::toolbar::toolbar;
use crate::canvas::{Canvas, CanvasFrame, CanvasGpu, CanvasRole};
use crate::dev::{BenchFrame, BenchRun, DevRun, DevStep, Readiness, ScreenshotRun};
use crate::dialogs::{DialogRequest, Modals};
use crate::files::{BackgroundWriter, FileDialogs, WATCH_INTERVAL, Watcher, dropped_files};
use crate::focus::{enforce_focus, is_typing, shortcuts};
use crate::panels::{LayersPanel, ObjectsPanel, TileImages, TilesPanel};
use crate::platform::{QuitGuard, install_system_quit_guard};
use crate::style::apply_theme;
use crate::tileset_editor::{TilesetEditorView, TilesetFrame};
use crate::tools::ToolBox;
use crate::workers::Workers;

/// How long the session waits after a change before it is written.
const SESSION_DELAY: Duration = Duration::from_secs(1);
/// How often the recovery schedule runs while something is unsaved.
const RECOVERY_TICK: Duration = Duration::from_secs(1);
/// How long a notice stays in the status bar.
const FLASH_FOR: Duration = Duration::from_secs(5);
/// How long a warning or error stays in the status bar.
const FLASH_PROBLEM_FOR: Duration = Duration::from_secs(10);

/// The window.
pub(crate) struct App {
    pub(super) editor: Editor,
    pub(super) canvases: HashMap<DocumentId, Canvas>,
    layers_panel: LayersPanel,
    objects_panel: ObjectsPanel,
    /// The panel the left group shows.
    left_tab: LeftTab,
    /// The canvas's tools.
    toolbox: ToolBox,
    tiles_panel: TilesPanel,
    /// The tileset editor, shown in place of a map or in a tileset's tab.
    pub(super) tileset_editor: TilesetEditorView,
    pub(super) tile_images: TileImages,
    pub(super) modals: Modals,
    pub(super) workers: Workers,
    /// How files are asked for.
    pub(super) file_dialogs: FileDialogs,
    pub(super) writer: BackgroundWriter,
    /// Looks at the files on disk for changes.
    watcher: Watcher,
    /// Where the preferences are written.
    pub(super) settings_path: PathBuf,
    /// When the watcher was last told which files to look at.
    watch_listed: Instant,
    /// Whether intents were applied since then, so the list may be out of date.
    watch_list_stale: bool,
    pub(super) gpu: Option<CanvasGpu>,
    images: Arc<Mutex<ImageStore>>,
    pub(super) intents: Vec<Intent>,
    pub(super) flash: Option<Flash>,
    session: SessionSync,
    pub(super) quit_allowed: bool,
    quit_guard: Arc<QuitGuard>,
    pub(super) dev: Option<DevRun>,
    pub(super) outcome: RunOutcome,
    title: String,
    started: Instant,
    frame_started: Instant,
    expects_document: bool,
}

/// The panels of the left group, shown one at a time as tabs.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum LeftTab {
    /// The Layers panel.
    #[default]
    Layers,
    /// The Objects panel.
    Objects,
}

impl LeftTab {
    /// The tab's title.
    fn title(self) -> &'static str {
        match self {
            LeftTab::Layers => "Layers",
            LeftTab::Objects => "Objects",
        }
    }
}

/// A notice in the status bar until a time.
pub(crate) struct Flash {
    pub(super) notice: Notice,
    until: Instant,
}

/// When the session was last written, and since when it has changed.
struct SessionSync {
    path: PathBuf,
    written: u64,
    changed_since: Option<Instant>,
    enabled: bool,
}

impl App {
    /// The window, drawing with the wgpu device eframe made.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Result<App, String> {
        let render_state = cc.wgpu_render_state.as_ref().ok_or_else(|| {
            "better-tiled draws maps with wgpu, and the window has no wgpu device".to_owned()
        })?;
        let checking = startup.screenshot.is_some() || startup.bench;
        let mut app = App::assemble(&cc.egui_ctx, startup)?;
        app.gpu = Some(CanvasGpu::install(render_state, Arc::clone(&app.images)));
        if !checking && let Err(problem) = app.watcher.start(cc.egui_ctx.clone(), WATCH_INTERVAL) {
            app.notify(Notice::warning(format!(
                "Files changed by other programs will not be noticed: {problem}"
            )));
        }
        if let Err(reason) = install_system_quit_guard(Arc::clone(&app.quit_guard)) {
            app.notify(Notice::warning(format!(
                "Quitting from the app menu or the Dock will not ask about unsaved work: {reason}"
            )));
        }
        Ok(app)
    }

    /// The window without a GPU: the canvas draws no map (for the window's tests).
    #[cfg(test)]
    pub(crate) fn without_gpu(ctx: &egui::Context, startup: Startup) -> Result<App, String> {
        let mut app = App::assemble(ctx, startup)?;
        app.file_dialogs = FileDialogs::Recorded(Vec::new());
        Ok(app)
    }

    /// The file dialogs asked for and not answered yet (for the window's tests).
    #[cfg(test)]
    pub(crate) fn take_file_requests(&mut self) -> Vec<FileRequest> {
        match &mut self.file_dialogs {
            FileDialogs::Recorded(requests) => std::mem::take(requests),
            FileDialogs::System => Vec::new(),
        }
    }

    /// The editor (for the window's tests).
    #[cfg(test)]
    pub(crate) fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Whether work is under way: a worker running or intents waiting (for the window's tests).
    #[cfg(test)]
    pub(crate) fn is_busy(&self) -> bool {
        self.workers.is_busy() || !self.intents.is_empty()
    }

    /// Looks at the watched files now, as the watcher's thread would (for the window's tests).
    #[cfg(test)]
    pub(crate) fn poll_files_now(&mut self) {
        self.watcher.set_watched(self.editor.watched());
        self.watcher.poll_now();
    }

    /// Where the middle of a cell of the current map is on screen (for the window's tests).
    #[cfg(test)]
    pub(crate) fn cell_on_screen(&self, cell: glam::IVec2) -> Option<egui::Pos2> {
        let doc = self.editor.current()?;
        let map = self.editor.map_of(doc)?;
        self.canvases.get(&doc)?.cell_on_screen(map, cell)
    }

    /// Where a point of the current map's screen space is on screen (for the window's tests).
    #[cfg(test)]
    pub(crate) fn world_on_screen(&self, world: glam::DVec2) -> Option<egui::Pos2> {
        let doc = self.editor.current()?;
        Some(self.canvases.get(&doc)?.last_view()?.world_to_screen(world))
    }

    /// Where the middle of a tile of the current document's tileset editor is on screen (for
    /// the window's tests).
    #[cfg(test)]
    pub(crate) fn tileset_tile_on_screen(&self, tile: bt_core::TileId) -> Option<egui::Pos2> {
        self.tileset_editor
            .tile_on_screen(self.editor.current()?, tile)
    }

    /// Where a point of the current document's collision view is on screen (for the window's
    /// tests).
    #[cfg(test)]
    pub(crate) fn collision_world_on_screen(&self, world: glam::DVec2) -> Option<egui::Pos2> {
        self.tileset_editor
            .collision_world_on_screen(self.editor.current()?, world)
    }

    /// Where the middle of a tile of the Tiles panel is on screen (for the window's tests).
    #[cfg(test)]
    pub(crate) fn tiles_panel_tile_on_screen(&self, tile: bt_core::TileId) -> Option<egui::Pos2> {
        self.tiles_panel.tile_on_screen(tile)
    }

    /// Where the middle of a tile of the animation editor's palette is on screen (for the
    /// window's tests).
    #[cfg(test)]
    pub(crate) fn palette_tile_on_screen(&self, tile: bt_core::TileId) -> Option<egui::Pos2> {
        self.tileset_editor.palette_tile_on_screen(tile)
    }

    /// Delivers intents as a worker would (for the window's tests: a file dialog's answer).
    #[cfg(test)]
    pub(crate) fn inject_worker_result(&mut self, intents: Vec<Intent>) {
        self.workers.inject(intents);
    }

    /// Whether a modal is open (for the window's tests).
    #[cfg(test)]
    pub(crate) fn has_modal(&self) -> bool {
        self.modals.is_open()
    }

    /// Whether the window is closing (for the window's tests).
    #[cfg(test)]
    pub(crate) fn is_closing(&self) -> bool {
        self.quit_allowed
    }

    /// What answers a quit the system asks for (for the window's tests).
    #[cfg(test)]
    pub(crate) fn quit_guard(&self) -> &QuitGuard {
        &self.quit_guard
    }

    fn assemble(ctx: &egui::Context, startup: Startup) -> Result<App, String> {
        apply_theme(ctx, &startup.settings.theme);
        // ⌘= and ⌘- zoom the map, not the interface.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let pid = startup.pid;
        let editor = Editor::builder()
            .settings(startup.settings.clone())
            .recovery_folder(startup.recovery_folder.clone())
            .pid(pid)
            .home(home_folder())
            .build()
            .map_err(|error| format!("the key bindings in settings.toml are not valid: {error}"))?;
        let mut images = ImageStore::builder()
            .build()
            .map_err(|error| format!("the image decoders did not start: {error}"))?;
        let waker = ctx.clone();
        images.set_waker(move || waker.request_repaint());
        let writer = BackgroundWriter::start(startup.recovery_folder.clone(), pid)?;
        let checking = startup.screenshot.is_some() || startup.bench;
        let mut app = App {
            editor,
            canvases: HashMap::new(),
            layers_panel: LayersPanel::default(),
            objects_panel: ObjectsPanel::default(),
            left_tab: LeftTab::default(),
            toolbox: ToolBox::default(),
            tiles_panel: TilesPanel::default(),
            tileset_editor: TilesetEditorView::default(),
            tile_images: TileImages::default(),
            modals: Modals::default(),
            workers: Workers::new(ctx.clone()),
            file_dialogs: FileDialogs::System,
            writer,
            watcher: Watcher::new(),
            settings_path: startup.settings_path.clone(),
            watch_listed: Instant::now(),
            watch_list_stale: true,
            gpu: None,
            images: Arc::new(Mutex::new(images)),
            intents: Vec::new(),
            flash: None,
            session: SessionSync {
                path: startup.session_path.clone(),
                written: 0,
                changed_since: None,
                // A check leaves the session as it was.
                enabled: !checking,
            },
            quit_allowed: false,
            quit_guard: QuitGuard::new(ctx.clone()),
            dev: None,
            outcome: startup.outcome(),
            title: String::new(),
            started: Instant::now(),
            frame_started: Instant::now(),
            expects_document: !startup.files.is_empty(),
        };
        app.start(ctx, startup, checking);
        Ok(app)
    }

    /// Restores the session, offers back what a run that ended badly left, and opens the files
    /// given. A check opens only its files and asks nothing.
    fn start(&mut self, ctx: &egui::Context, startup: Startup, checking: bool) {
        if !checking {
            let orphans =
                bt_editor::orphans(&startup.recovery_folder, startup.pid, SystemTime::now());
            if !orphans.is_empty() {
                self.intents.push(Intent::Recoverable(orphans));
            }
            if let Some(mut session) = startup.session {
                if !startup.files.is_empty() || !self.editor.settings().general.restore_session {
                    session.open_files.clear();
                    session.current = None;
                }
                let effects = self.editor.restore_session(session);
                self.carry_out_all(ctx, effects);
            }
        }
        self.session.written = self.editor.session_generation();
        for file in &startup.files {
            let path = std::path::absolute(file).unwrap_or_else(|_| file.clone());
            self.intents.push(Intent::Open(path));
        }
        if let Some(problem) = startup.session_problem {
            self.notify(Notice::warning(problem));
        }
        self.dev = if let Some(path) = startup.screenshot {
            Some(DevRun::Screenshot(ScreenshotRun::new(
                path,
                startup.screenshot_actions.clone(),
            )))
        } else if startup.bench {
            Some(DevRun::Bench(BenchRun::new()))
        } else {
            None
        };
        self.apply_intents(ctx);
    }

    /// Applies the intents collected, in order, and carries out what they return.
    pub(super) fn apply_intents(&mut self, ctx: &egui::Context) {
        while !self.intents.is_empty() {
            self.watch_list_stale = true;
            let intents = std::mem::take(&mut self.intents);
            for intent in intents {
                let effects = self.editor.apply(intent);
                self.carry_out_all(ctx, effects);
            }
        }
    }

    /// Tells the watcher which files to look at, at most once per poll: what is open changes
    /// only through intents, so a frame comes after any change.
    fn list_watched_files(&mut self, ctx: &egui::Context) {
        if !self.watch_list_stale {
            return;
        }
        let since = self.watch_listed.elapsed();
        if since < WATCH_INTERVAL {
            ctx.request_repaint_after(WATCH_INTERVAL - since);
            return;
        }
        self.watcher.set_watched(self.editor.watched());
        self.watch_listed = Instant::now();
        self.watch_list_stale = false;
    }

    /// Shows a notice in the status bar, or as a dialog when it has details.
    pub(super) fn notify(&mut self, notice: Notice) {
        match notice.severity {
            Severity::Info => log::info!("{}", notice.text),
            Severity::Warning => log::warn!("{}", notice.text),
            Severity::Error => log::error!("{}", notice.text),
        }
        if self.dev.is_some() && notice.severity == Severity::Error {
            self.outcome.record_failure(notice.text.clone());
        }
        if notice.severity == Severity::Error && notice.detail.is_some() {
            self.modals.push(crate::dialogs::Modal::Error(notice));
            return;
        }
        let lasts = match notice.severity {
            Severity::Info => FLASH_FOR,
            Severity::Warning | Severity::Error => FLASH_PROBLEM_FOR,
        };
        self.flash = Some(Flash {
            notice,
            until: Instant::now() + lasts,
        });
    }

    /// A quit the system asked for and the guard held back runs the Quit command, with the
    /// window brought forward so its question is seen.
    fn handle_system_quit(&mut self, ctx: &egui::Context) {
        if !self.quit_guard.take_asked() || self.quit_allowed || self.modals.asks_about_quitting() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.intents.push(Intent::Run(ActionId::Quit));
    }

    /// Tells the guard whether a quit now would lose unsaved work.
    fn sync_quit_guard(&self) {
        self.quit_guard
            .set_unsaved(!self.quit_allowed && self.editor.has_unsaved());
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.viewport().close_requested()) || self.quit_allowed {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if !self.modals.asks_about_quitting() {
            self.intents.push(Intent::Run(ActionId::Quit));
        }
    }

    fn show_center(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            tab_strip(ui, &self.editor, &mut self.intents);
            match self.editor.current() {
                Some(doc) if self.shows_tileset_editor(doc) => self.show_tileset_editor(ui, doc),
                Some(doc) => self.show_canvas(ui, doc),
                None => self.show_welcome(ui),
            }
        });
    }

    fn show_canvas(&mut self, ui: &mut egui::Ui, doc: DocumentId) {
        let Some(map) = self.editor.map_of(doc).cloned() else {
            return;
        };
        let Some(document) = self.editor.document(doc) else {
            return;
        };
        let frame = CanvasFrame {
            doc,
            map: &map,
            saved_camera: self.editor.camera_of(doc),
            toggles: document.view(),
            selected_layers: &document.selection().layers,
            settings: self.editor.settings(),
            gpu: self.gpu.as_ref(),
            typing: is_typing(ui.ctx()),
            dialog_open: self.modals.is_open(),
            editor: &self.editor,
            role: CanvasRole::Map,
        };
        let canvas = self.canvases.entry(doc).or_default();
        canvas.show(ui, &frame, &mut self.toolbox, &mut self.intents);
    }

    /// Whether `doc` shows its tileset editor in place of a map.
    fn shows_tileset_editor(&self, doc: DocumentId) -> bool {
        self.editor
            .document(doc)
            .is_some_and(|d| d.tileset_editing().is_some())
    }

    fn show_tileset_editor(&mut self, ui: &mut egui::Ui, doc: DocumentId) {
        let frame = TilesetFrame {
            editor: &self.editor,
            doc,
            images: &mut self.tile_images,
            store: &self.images,
            gpu: self.gpu.as_ref(),
            typing: is_typing(ui.ctx()),
            dialog_open: self.modals.is_open(),
        };
        self.tileset_editor
            .show(ui, frame, &mut self.toolbox, &mut self.intents);
    }

    /// The canvas the current document shows: its map's, or its collision view's.
    fn shown_canvas(&self) -> Option<&Canvas> {
        let doc = self.editor.current()?;
        if self.shows_tileset_editor(doc) {
            self.tileset_editor.collision_canvas(doc)
        } else {
            self.canvases.get(&doc)
        }
    }

    fn show_welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.heading("better-tiled");
            ui.add_space(8.0);
            ui.label(format!(
                "{} a map, or drop one here.",
                self.editor.keymap().with_key(ActionId::Open)
            ));
            let recent = self.editor.session().recent_files;
            if !recent.is_empty() {
                ui.add_space(16.0);
                ui.weak("Recent files");
                for path in recent.into_iter().take(10) {
                    let name = bt_utils::paths::display_name(&path);
                    let button = ui.button(name).on_hover_text(path.display().to_string());
                    if button.clicked() {
                        self.intents.push(Intent::Open(path));
                    }
                }
            }
        });
    }

    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        let current = self.editor.current();
        let canvas = self.shown_canvas();
        let input = StatusInput {
            flash: self.flash.as_ref().map(|flash| &flash.notice),
            hover: canvas.and_then(Canvas::hover),
            zoom: canvas.map(Canvas::zoom),
            map: current
                .and_then(|doc| self.editor.map_of(doc))
                .map(|map| &**map),
            hint: canvas.and_then(Canvas::hint),
        };
        egui::Panel::bottom("status_bar").show(ui, |ui| status_bar(ui, &input));
    }

    fn show_panels(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.editor.current() else {
            return;
        };
        if self.editor.map_of(doc).is_none() {
            // A tileset's tab: the tileset editor is all there is.
            return;
        }
        // While a map's tileset is edited in its place, the map's layers and objects wait.
        if !self.shows_tileset_editor(doc) {
            egui::Panel::left("left_panels")
                .resizable(true)
                .default_size(240.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in [LeftTab::Layers, LeftTab::Objects] {
                            let title = egui::RichText::new(tab.title()).heading();
                            if ui.selectable_label(self.left_tab == tab, title).clicked() {
                                self.left_tab = tab;
                            }
                        }
                    });
                    ui.separator();
                    match self.left_tab {
                        LeftTab::Layers => {
                            self.layers_panel
                                .show(ui, &self.editor, doc, &mut self.intents);
                        }
                        LeftTab::Objects => {
                            self.objects_panel
                                .show(ui, &self.editor, doc, &mut self.intents);
                        }
                    }
                });
        }
        egui::Panel::right("tiles")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                self.tiles_panel.show(
                    ui,
                    &self.editor,
                    doc,
                    &mut self.tile_images,
                    &self.images,
                    &mut self.intents,
                );
            });
    }

    /// What the frame ends with: the session written when it changed (after a pause), the
    /// title, the flash's end, closed tabs' canvases forgotten, and a check's next step.
    fn end_frame(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.sync_session(ctx, now);
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if let Some(flash) = &self.flash {
            match flash.until.checked_duration_since(now) {
                Some(left) => ctx.request_repaint_after(left),
                None => self.flash = None,
            }
        }
        let editor = &self.editor;
        self.canvases
            .retain(|doc, _| editor.document(*doc).is_some());
        let open: Vec<DocumentId> = editor.documents().in_order().map(|d| d.id()).collect();
        self.tileset_editor.retain_documents(&open);
        self.step_dev_run(ctx);
    }

    fn sync_session(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.session.enabled {
            return;
        }
        let generation = self.editor.session_generation();
        if generation == self.session.written {
            self.session.changed_since = None;
            return;
        }
        let since = *self.session.changed_since.get_or_insert(now);
        let waited = now - since;
        if waited >= SESSION_DELAY {
            self.write_session();
        } else {
            ctx.request_repaint_after(SESSION_DELAY - waited);
        }
    }

    /// Writes the session now (on the writer thread).
    pub(super) fn write_session(&mut self) {
        if !self.session.enabled {
            return;
        }
        match self.editor.session().to_json() {
            Ok(text) => self.writer.write_session(self.session.path.clone(), text),
            Err(error) => log::warn!("the session could not be written: {error}"),
        }
        self.session.written = self.editor.session_generation();
        self.session.changed_since = None;
    }

    fn window_title(&self) -> String {
        let Some(doc) = self.editor.current() else {
            return "better-tiled".to_owned();
        };
        let asset = self
            .editor
            .document(doc)
            .and_then(|document| self.editor.store().get(document.asset()));
        match asset {
            Some(asset) => {
                let dirty = if asset.is_dirty() { " •" } else { "" };
                format!("{}{dirty} — better-tiled", asset.display_name())
            }
            None => "better-tiled".to_owned(),
        }
    }

    fn step_dev_run(&mut self, ctx: &egui::Context) {
        let Some(dev) = &mut self.dev else {
            return;
        };
        let stats = self
            .gpu
            .as_ref()
            .map(CanvasGpu::last_stats)
            .unwrap_or_default();
        let readiness = Readiness {
            busy: self.workers.is_busy() || !self.intents.is_empty(),
            pending_images: stats.pending_images > 0
                || stats.pending_impostors > 0
                || self.tile_images.take_pending(),
            has_document: self.editor.current().is_some(),
            expects_document: self.expects_document,
        };
        let step = match dev {
            DevRun::Screenshot(run) => {
                let picture = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                        _ => None,
                    })
                });
                run.step(readiness, picture.as_deref())
            }
            DevRun::Bench(run) => {
                let current = self.editor.current();
                run.step(BenchFrame {
                    readiness,
                    canvas: current.and_then(|doc| self.canvases.get_mut(&doc)),
                    map: current
                        .and_then(|doc| self.editor.map_of(doc))
                        .map(|map| &**map),
                    cpu: self.frame_started.elapsed(),
                    stats,
                })
            }
        };
        match step {
            DevStep::Continue => ctx.request_repaint(),
            DevStep::Run(actions) => {
                self.intents.extend(actions.into_iter().map(Intent::Run));
                ctx.request_repaint();
            }
            DevStep::TakeScreenshot => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                ctx.request_repaint();
            }
            DevStep::Quit(failure) => {
                if let Some(failure) = failure {
                    self.outcome.record_failure(failure);
                }
                self.dev = None;
                self.quit_allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_started = Instant::now();
        let finished = self.workers.take_results();
        for intent in finished {
            match intent {
                // The New Tileset dialog's own question.
                Intent::FilePicked {
                    request: FileRequest::TilesetImage,
                    path,
                } => {
                    if let Some(path) = path {
                        self.modals.deliver_tileset_image(path);
                    }
                }
                other => self.intents.push(other),
            }
        }
        if let Some(observed) = self.watcher.take_observations()
            && self.editor.settings().general.watch_files
        {
            self.intents.push(Intent::Observed(observed));
        }
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.intents.push(Intent::Tick { now_ms });
        self.handle_system_quit(ctx);
        self.apply_intents(ctx);
        for problem in self.writer.take_problems() {
            self.notify(Notice::warning(problem));
        }
        if self.editor.has_unsaved() {
            ctx.request_repaint_after(RECOVERY_TICK);
        }
        self.list_watched_files(ctx);
        self.sync_quit_guard();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_close_request(&ctx);
        for path in dropped_files(&ctx) {
            self.intents.push(Intent::Open(path));
        }
        if !self.modals.is_open() {
            enforce_focus(&ctx);
            for id in shortcuts(&ctx, &self.editor) {
                self.intents.push(Intent::Run(id));
            }
        }
        egui::Panel::top("menu_bar").show(ui, |ui| menu_bar(ui, &self.editor, &mut self.intents));
        if self.editor.current().is_some() {
            egui::Panel::top("toolbar").show(ui, |ui| toolbar(ui, &self.editor, &mut self.intents));
        }
        self.show_status_bar(ui);
        self.show_panels(ui);
        self.show_center(ui);
        let mut requests = Vec::new();
        self.modals
            .show(&ctx, &self.editor, &mut self.intents, &mut requests);
        for request in requests {
            match request {
                DialogRequest::PickTilesetImage => {
                    let asked = self
                        .file_dialogs
                        .ask(&mut self.workers, FileRequest::TilesetImage);
                    if let Err(problem) = asked {
                        self.notify(Notice::error(problem));
                    }
                }
            }
        }
        self.apply_intents(&ctx);
        self.end_frame(&ctx);
        self.sync_quit_guard();
    }

    fn on_exit(&mut self) {
        self.watcher.stop();
        if !self.quit_allowed {
            // The run ends without a quit anyone answered (the system's, when the quit guard
            // could not be set up): the last snapshots of unsaved work are written and kept for
            // the next start.
            for snapshot in self.editor.snapshots_for_unanswered_exit() {
                self.writer.write_snapshot(snapshot);
            }
        }
        self.write_session();
        self.writer.finish();
    }
}

/// The user's home folder: projects are not looked for at or above it.
fn home_folder() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
