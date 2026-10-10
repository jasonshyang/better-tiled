//! The window's state and its frame. `logic` delivers what the workers finished and ticks the
//! editor (recovery snapshots), even while the window is hidden; `ui` handles a request to
//! close, routes the keyboard, draws the menu bar, status bar, panels, tabs and canvas and the
//! modals, then applies the intents the frame collected, in order, and carries out the effects
//! the editor returns.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bt_editor::{ActionId, DocumentId, Editor, Intent, Notice, Severity};
use bt_image::ImageStore;

use super::menu::menu_bar;
use super::startup::{RunOutcome, Startup};
use super::status_bar::{StatusInput, status_bar};
use super::tabs::tab_strip;
use crate::canvas::{Canvas, CanvasFrame, CanvasGpu};
use crate::dev::{BenchFrame, BenchRun, DevRun, DevStep, Readiness, ScreenshotRun};
use crate::dialogs::Modals;
use crate::files::{BackgroundWriter, dropped_files};
use crate::focus::{enforce_focus, is_typing, shortcuts};
use crate::panels::{LayersPanel, TileImages, TilesPanel};
use crate::platform::route_quit_key_to_window;
use crate::style::apply_theme;
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
    tiles_panel: TilesPanel,
    pub(super) tile_images: TileImages,
    pub(super) modals: Modals,
    pub(super) workers: Workers,
    pub(super) writer: BackgroundWriter,
    pub(super) gpu: Option<CanvasGpu>,
    images: Arc<Mutex<ImageStore>>,
    pub(super) intents: Vec<Intent>,
    pub(super) flash: Option<Flash>,
    session: SessionSync,
    pub(super) quit_allowed: bool,
    pub(super) dev: Option<DevRun>,
    pub(super) outcome: RunOutcome,
    title: String,
    started: Instant,
    frame_started: Instant,
    expects_document: bool,
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
        let mut app = App::assemble(&cc.egui_ctx, startup)?;
        app.gpu = Some(CanvasGpu::install(render_state, Arc::clone(&app.images)));
        route_quit_key_to_window();
        Ok(app)
    }

    /// The window without a GPU: the canvas draws no map (for the window's tests).
    #[cfg(test)]
    pub(crate) fn without_gpu(ctx: &egui::Context, startup: Startup) -> Result<App, String> {
        App::assemble(ctx, startup)
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
            tiles_panel: TilesPanel::default(),
            tile_images: TileImages::default(),
            modals: Modals::default(),
            workers: Workers::new(ctx.clone()),
            writer,
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
            Some(DevRun::Screenshot(ScreenshotRun::new(path)))
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
            let intents = std::mem::take(&mut self.intents);
            for intent in intents {
                let effects = self.editor.apply(intent);
                self.carry_out_all(ctx, effects);
            }
        }
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
        };
        let canvas = self.canvases.entry(doc).or_default();
        canvas.show(ui, &frame, &mut self.intents);
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
        let canvas = current.and_then(|doc| self.canvases.get(&doc));
        let input = StatusInput {
            flash: self.flash.as_ref().map(|flash| &flash.notice),
            hover: canvas.and_then(Canvas::hover),
            zoom: canvas.map(Canvas::zoom),
            map: current
                .and_then(|doc| self.editor.map_of(doc))
                .map(|map| &**map),
        };
        egui::Panel::bottom("status_bar").show(ui, |ui| status_bar(ui, &input));
    }

    fn show_panels(&mut self, ui: &mut egui::Ui) {
        let Some(doc) = self.editor.current() else {
            return;
        };
        egui::Panel::left("layers")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                self.layers_panel
                    .show(ui, &self.editor, doc, &mut self.intents);
            });
        egui::Panel::right("tiles")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                self.tiles_panel
                    .show(ui, &self.editor, doc, &mut self.tile_images, &self.images);
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
            pending_images: stats.pending_images > 0 || self.tile_images.take_pending(),
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
        self.intents.extend(finished);
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.intents.push(Intent::Tick { now_ms });
        self.apply_intents(ctx);
        for problem in self.writer.take_problems() {
            self.notify(Notice::warning(problem));
        }
        if self.editor.has_unsaved() {
            ctx.request_repaint_after(RECOVERY_TICK);
        }
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
        self.show_status_bar(ui);
        self.show_panels(ui);
        self.show_center(ui);
        self.modals.show(&ctx, &mut self.intents);
        self.apply_intents(&ctx);
        self.end_frame(&ctx);
    }

    fn on_exit(&mut self) {
        if !self.quit_allowed {
            // The system ended the run (the app menu, the Dock, a logout): the last snapshots
            // of unsaved work are written and kept for the next start.
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
