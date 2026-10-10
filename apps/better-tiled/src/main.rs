//! better-tiled, the editor's window. `main` reads the command line, sets up logging, finds the
//! data folder, reads the settings and opens the window; the window is the app's
//! (`docs/design/06-application.md`).

mod app;
mod canvas;
mod cli;
mod dev;
mod dialogs;
mod files;
mod focus;
mod panels;
mod platform;
mod style;
#[cfg(test)]
mod test_window;
mod tools;
mod workers;

use std::process::ExitCode;

use crate::app::{App, Startup};
use crate::cli::{Cli, Mode, USAGE};

/// The window's size when it opens, in points.
const WINDOW_SIZE: [f32; 2] = [1440.0, 900.0];
/// The smallest the window may be, in points.
const MIN_WINDOW_SIZE: [f32; 2] = [760.0, 480.0];

#[allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the command line's own output"
)]
fn main() -> ExitCode {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,better_tiled=info,bt_editor=info"),
    )
    .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match Cli::parse(&args) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("better-tiled: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match cli.mode {
        Mode::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Mode::Version => {
            println!("better-tiled {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Mode::Edit => {}
    }
    let startup = match Startup::from_cli(cli) {
        Ok(startup) => startup,
        Err(message) => {
            eprintln!("better-tiled: {message}");
            return ExitCode::FAILURE;
        }
    };
    let outcome = startup.outcome();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("better-tiled")
            .with_app_id("better-tiled")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        persist_window: false,
        ..eframe::NativeOptions::default()
    };
    let run = eframe::run_native(
        "better-tiled",
        options,
        Box::new(|cc| {
            let app = App::new(cc, startup)?;
            Ok(Box::new(app))
        }),
    );
    if let Err(error) = run {
        eprintln!("better-tiled: {error}");
        return ExitCode::FAILURE;
    }
    match outcome.failure() {
        Some(message) => {
            eprintln!("better-tiled: {message}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}
