//! The window under test (`06-application.md`, §15): the app without a GPU in a kittest
//! harness, with a data folder of its own, driven as a person drives it (dropping files,
//! pressing keys, clicking labelled widgets, closing the window) and read back through the
//! editor's queries.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_editor::Editor;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::{App, Startup};
use crate::cli::Cli;
use crate::platform::SystemQuit;

/// How long the window may take to finish its work after an input.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// A file dropped on the window by a test.
#[derive(Debug)]
struct DroppedPath(PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// The window, driven by a test.
pub(crate) struct TestWindow {
    harness: Harness<'static, App>,
}

impl TestWindow {
    /// A window whose settings, session and recovery folder are in `data`.
    pub(crate) fn with_data(data: &Path) -> TestWindow {
        TestWindow::run_as(data, std::process::id())
    }

    /// A window in `data` for run `pid` (a later run in the same process is another run).
    pub(crate) fn run_as(data: &Path, pid: u32) -> TestWindow {
        let mut startup = Startup::from_folder(Cli::default(), data).unwrap();
        startup.pid = pid;
        // Frames a 60th of a second apart, as a person's are: two clicks a few frames apart are
        // a double click.
        let harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_step_dt(1.0 / 60.0)
            .build_eframe(|cc| App::without_gpu(&cc.egui_ctx, startup).unwrap());
        let mut window = TestWindow { harness };
        window.settle();
        window
    }

    /// Drops a file on the window and waits until it is open.
    pub(crate) fn drop_file(&mut self, path: &Path) {
        self.harness
            .input_mut()
            .dropped_files
            .push(Arc::new(DroppedPath(path.to_path_buf())));
        self.settle();
    }

    /// Lets the watcher look at the files on disk now, and waits for what that brings.
    pub(crate) fn poll_files(&mut self) {
        self.harness.state_mut().poll_files_now();
        self.harness.run_steps(2);
        self.settle();
    }

    /// Answers the file dialog the window asked for with `path`, as its worker would; the
    /// window must have asked exactly `request`.
    pub(crate) fn answer_file_dialog(&mut self, request: bt_editor::FileRequest, path: &Path) {
        let asked = self.harness.state_mut().take_file_requests();
        assert_eq!(
            asked,
            std::slice::from_ref(&request),
            "the file dialogs the window asked for"
        );
        self.harness
            .state_mut()
            .inject_worker_result(vec![bt_editor::Intent::FilePicked {
                request,
                path: Some(path.to_path_buf()),
            }]);
        self.harness.run_steps(2);
        self.settle();
    }

    /// Drags on the map from the middle of cell `from` to the middle of cell `to` with the
    /// primary button, holding `modifiers`, passing every cell between.
    pub(crate) fn drag_cells(
        &mut self,
        from: glam::IVec2,
        to: glam::IVec2,
        modifiers: egui::Modifiers,
    ) {
        let start = self.screen_of(from);
        let end = self.screen_of(to);
        self.pointer(egui::Event::PointerMoved(start));
        self.pointer(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        });
        let steps = 8;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            self.pointer(egui::Event::PointerMoved(start.lerp(end, t)));
        }
        self.pointer(egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        });
        self.settle();
    }

    /// Clicks cell `cell` of the map with `button`, holding `modifiers`.
    pub(crate) fn click_cell(
        &mut self,
        cell: glam::IVec2,
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
    ) {
        let at = self.screen_of(cell);
        self.pointer(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            self.pointer(egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers,
            });
        }
        self.settle();
    }

    /// Where a cell's middle is on screen.
    fn screen_of(&mut self, cell: glam::IVec2) -> egui::Pos2 {
        self.harness.run_steps(1);
        let Some(at) = self.harness.state().cell_on_screen(cell) else {
            panic!("no map is shown, so cell {cell} is nowhere");
        };
        at
    }

    /// Sends one pointer event, in a frame of its own.
    fn pointer(&mut self, event: egui::Event) {
        self.harness.event(event);
        self.harness.run_steps(1);
    }

    /// Presses a key with modifiers.
    pub(crate) fn press(&mut self, modifiers: egui::Modifiers, key: egui::Key) {
        self.harness.key_press_modifiers(modifiers, key);
        self.settle();
    }

    /// Clicks the first widget labelled `label`.
    pub(crate) fn click(&mut self, label: &str) {
        let node = self.harness.get_all_by_label(label).next();
        let Some(node) = node else {
            panic!("no widget is labelled “{label}”");
        };
        node.click();
        self.settle();
    }

    /// Double-clicks the first widget labelled `label`.
    pub(crate) fn double_click(&mut self, label: &str) {
        let node = self.harness.get_all_by_label(label).next();
        let Some(node) = node else {
            panic!("no widget is labelled “{label}”");
        };
        node.click();
        node.click();
        self.settle();
    }

    /// Types text into the focused text field.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.harness.event(egui::Event::Text(text.to_owned()));
        self.settle();
    }

    /// Clicks the window's close button.
    pub(crate) fn request_close(&mut self) {
        self.harness
            .input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        self.settle();
    }

    /// Asks to quit as the app menu, the Dock or a logout does on macOS; the answer the system
    /// gets.
    pub(crate) fn system_quit(&mut self) -> SystemQuit {
        let answer = self.harness.state().quit_guard().answer_system_quit();
        self.settle();
        answer
    }

    /// Ends the run as eframe does when the window closes.
    pub(crate) fn exit(mut self) {
        eframe::App::on_exit(self.harness.state_mut());
    }

    /// The editor behind the window.
    pub(crate) fn editor(&self) -> &Editor {
        self.harness.state().editor()
    }

    /// Whether a modal is open.
    pub(crate) fn has_modal(&self) -> bool {
        self.harness.state().has_modal()
    }

    /// Whether the window is closing.
    pub(crate) fn is_closing(&self) -> bool {
        self.harness.state().is_closing()
    }

    /// Runs frames until the workers are done, nothing is left to apply and the layout has
    /// stopped moving (a new dialog takes a frame to measure itself before it is placed).
    pub(crate) fn settle(&mut self) {
        let started = Instant::now();
        loop {
            self.harness.step();
            if !self.harness.state().is_busy()
                && self.harness.try_run_realtime().is_ok()
                && !self.harness.state().is_busy()
            {
                return;
            }
            assert!(
                started.elapsed() < SETTLE_TIMEOUT,
                "the window did not settle"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
