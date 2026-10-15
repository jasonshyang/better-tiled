//! Project processes run on workers; their output wakes the console while they are running.

use crate::workers::Workers;

impl super::App {
    pub(super) fn stop_commands(&mut self) {
        self.console.cancel_all();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while self.console.runs.iter().any(|run| run.status.is_none()) {
            for intent in self.workers.take_results() {
                if let Intent::CommandFinished { id, result } = intent {
                    self.console.finish(id, result);
                }
            }
            if std::time::Instant::now() >= deadline {
                log::error!("Timed out waiting for project commands to stop during exit");
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
use bt_editor::{CommandJob, Intent, Notice};
use bt_utils::process::{ProcessExit, ProcessText};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[derive(Default)]
pub(super) struct CommandConsole {
    next: u64,
    runs: Vec<CommandRun>,
}

struct CommandRun {
    id: u64,
    name: String,
    cancel: Arc<AtomicBool>,
    output: mpsc::Receiver<ProcessText>,
    text: String,
    status: Option<Result<ProcessExit, String>>,
    truncated: bool,
}

impl Drop for CommandConsole {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

impl CommandConsole {
    pub(super) fn start(
        &mut self,
        ctx: &egui::Context,
        workers: &mut Workers,
        job: CommandJob,
    ) -> Result<(), String> {
        if self.runs.len() >= 20 {
            let Some(index) = self.runs.iter().position(|run| run.status.is_some()) else {
                return Err(
                    "Twenty commands are already running; stop one before starting another".into(),
                );
            };
            self.runs.remove(index);
        }
        self.next += 1;
        let id = self.next;
        let name = job.name().to_owned();
        let cancel = Arc::new(AtomicBool::new(false));
        let request = Arc::clone(&cancel);
        let (sender, output) = mpsc::channel();
        let waker = ctx.clone();
        let shown = job.shows_output();
        workers.spawn("project command", move || {
            let result = job.run(request, move |text| {
                if shown {
                    let _ = sender.send(text);
                    waker.request_repaint();
                }
            });
            vec![Intent::CommandFinished { id, result }]
        })?;
        self.runs.push(CommandRun {
            id,
            name,
            cancel,
            output,
            text: String::new(),
            status: None,
            truncated: false,
        });
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        id: u64,
        result: Result<ProcessExit, String>,
    ) -> Option<Notice> {
        let run = self.runs.iter_mut().find(|run| run.id == id)?;
        let notice = match &result {
            Ok(exit) if exit.cancelled => None,
            Ok(exit) if exit.code == Some(0) => None,
            Ok(exit) => Some(Notice::error(format!(
                "{} failed ({})",
                run.name,
                exit.code
                    .map_or_else(|| "terminated".into(), |code| format!("exit {code}"))
            ))),
            Err(error) => Some(Notice::error(format!("{}: {error}", run.name))),
        };
        run.status = Some(result);
        notice
    }

    pub(super) fn cancel_all(&self) {
        for run in &self.runs {
            if run.status.is_none() {
                run.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        if ui.button("Clear Finished Commands").clicked() {
            self.runs.retain(|run| run.status.is_none());
        }
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for run in &mut self.runs {
                    for output in run.output.try_iter() {
                        if output.stderr {
                            run.text.push_str("[stderr] ");
                        }
                        run.text.push_str(&output.text);
                        const LIMIT: usize = 128 * 1024;
                        if run.text.len() > LIMIT {
                            let mut removed = run.text.len() - LIMIT;
                            while !run.text.is_char_boundary(removed) {
                                removed += 1;
                            }
                            run.text.drain(..removed);
                            run.truncated = true;
                        }
                    }
                    ui.push_id(run.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(&run.name);
                            match &run.status {
                                None => {
                                    ui.label("Running…");
                                    if ui.button("Stop Command").clicked() {
                                        run.cancel.store(true, Ordering::Relaxed);
                                    }
                                }
                                Some(Ok(exit)) if exit.cancelled => {
                                    ui.label("Cancelled");
                                }
                                Some(Ok(exit)) if exit.code == Some(0) => {
                                    ui.label("Exited 0");
                                }
                                Some(Ok(exit)) => {
                                    ui.colored_label(
                                        ui.visuals().error_fg_color,
                                        exit.code.map_or_else(
                                            || "Terminated".into(),
                                            |code| format!("Exited {code}"),
                                        ),
                                    );
                                }
                                Some(Err(error)) => {
                                    ui.colored_label(ui.visuals().error_fg_color, error);
                                }
                            }
                            if ui.button("Copy Output").clicked() {
                                ui.ctx().copy_text(run.text.clone());
                            }
                        });
                        if run.truncated {
                            ui.weak("Earlier output truncated; showing the last 128 KiB.");
                        }
                        if !run.text.is_empty() {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&run.text).monospace())
                                    .selectable(true),
                            );
                        }
                        ui.separator();
                    });
                }
            });
    }
}
