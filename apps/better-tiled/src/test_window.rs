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
        let harness = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
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
