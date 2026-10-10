//! Carrying out what the editor asks of the window: loads and file dialogs on workers,
//! questions as modals, notices in the status bar, camera moves, recovery snapshots on the
//! writer thread, and the window closing.

use bt_editor::{Dialog, Effect, Intent, Notice};

use super::frame::App;
use crate::dialogs::{Modal, NewMapDialog, NewTilesetDialog, PaletteDialog, PreferencesDialog};
use crate::style::apply_theme;

impl App {
    /// Carries out effects in order.
    pub(super) fn carry_out_all(&mut self, ctx: &egui::Context, effects: Vec<Effect>) {
        for effect in effects {
            self.carry_out(ctx, effect);
        }
    }

    fn carry_out(&mut self, ctx: &egui::Context, effect: Effect) {
        match effect {
            Effect::Load(job) => {
                let spawned = self
                    .workers
                    .spawn("load", move || vec![Intent::Loaded(Box::new(job.run()))]);
                if let Err(problem) = spawned {
                    self.notify(Notice::error(problem));
                }
            }
            Effect::PickFile(request) => {
                if let Err(problem) = self.file_dialogs.ask(&mut self.workers, request) {
                    self.notify(Notice::error(problem));
                }
            }
            Effect::Ask(decision) => self.modals.push(Modal::Decision(decision)),
            Effect::Notify(notice) => self.notify(notice),
            Effect::View(request) => {
                if let Some(doc) = self.editor.current()
                    && let Some(map) = self.editor.map_of(doc)
                    && let Some(canvas) = self.canvases.get_mut(&doc)
                {
                    canvas.apply_view_request(request, map);
                }
            }
            Effect::Show(Dialog::SearchActions) => {
                self.modals.push(Modal::Palette(PaletteDialog::default()));
            }
            Effect::Show(Dialog::NewMap) => {
                self.modals.push(Modal::NewMap(NewMapDialog::default()))
            }
            Effect::Show(Dialog::NewTileset(doc)) => {
                let tile_size = self
                    .editor
                    .map_of(doc)
                    .map_or(glam::UVec2::splat(32), |map| map.tile_size());
                self.modals
                    .push(Modal::NewTileset(NewTilesetDialog::new(doc, tile_size)));
            }
            Effect::Show(Dialog::Preferences) => {
                self.modals
                    .push(Modal::Preferences(Box::new(PreferencesDialog::new(
                        &self.editor,
                    ))))
            }
            Effect::WriteSettings(settings) => {
                apply_theme(ctx, &settings.theme);
                self.writer
                    .write_settings(self.settings_path.clone(), *settings);
            }
            Effect::ReloadImages(paths) => {
                if let Some(gpu) = &self.gpu {
                    gpu.forget_image_files(&paths);
                }
            }
            Effect::WriteSnapshot(snapshot) => self.writer.write_snapshot(snapshot),
            Effect::DeleteSnapshot(path) => self.writer.delete_snapshot(path),
            Effect::Quit => {
                self.quit_allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
