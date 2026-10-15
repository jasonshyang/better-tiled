//! Carrying out what the editor asks of the window: loads and file dialogs on workers,
//! questions as modals, notices in the status bar, camera moves, recovery snapshots on the
//! writer thread, and the window closing.

use bt_editor::{Dialog, Effect, Intent, Notice};

use super::frame::App;
use crate::dialogs::{
    ChooseLayerDialog, CustomTypesDialog, Modal, NewMapDialog, NewTilesetDialog, OffsetMapDialog,
    PaletteDialog, ResizeMapDialog,
};
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
            Effect::RunCommand(job) => {
                if job.shows_output() {
                    self.console_open = true;
                }
                if let Err(error) = self.console.start(ctx, &mut self.workers, job) {
                    self.notify(Notice::error(error));
                }
            }
            Effect::Show(Dialog::Console) => self.console_open = true,
            Effect::Show(Dialog::WorldProperties(doc)) => {
                if let Some(dialog) = crate::dialogs::WorldDialog::new(doc, &self.editor) {
                    self.modals.push(Modal::World(Box::new(dialog)));
                }
            }
            Effect::Show(Dialog::EditCommands(doc)) => {
                self.modals
                    .push(Modal::Commands(crate::dialogs::CommandsDialog::new(
                        doc,
                        &self.editor,
                    )))
            }
            Effect::Stamps(job) => {
                let job = std::sync::Arc::new(job);
                let worker_job = std::sync::Arc::clone(&job);
                let spawned = self.workers.spawn("stamps", move || {
                    vec![Intent::StampsLoaded(worker_job.run())]
                });
                if let Err(problem) = spawned {
                    self.intents.push(Intent::StampsLoaded(job.failed(problem)));
                }
            }
            Effect::Godot { doc, operation } => self.start_godot(ctx, doc, operation),
            Effect::Load(job) => {
                let spawned = self
                    .workers
                    .spawn("load", move || vec![Intent::Loaded(Box::new(job.run()))]);
                if let Err(problem) = spawned {
                    self.notify(Notice::error(problem));
                }
            }
            Effect::LoadRules(job) => {
                let spawned = self.workers.spawn("rules", move || {
                    vec![Intent::RulesLoaded(Box::new(job.run()))]
                });
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
            Effect::Show(Dialog::ExportImage(doc)) => {
                self.modals
                    .push(Modal::ExportImage(crate::dialogs::ExportImageDialog::new(
                        doc,
                    )))
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
            Effect::Show(Dialog::MoveObjectsToLayer(doc)) => self
                .modals
                .push(Modal::ChooseLayer(ChooseLayerDialog::new(doc))),
            Effect::Show(Dialog::ResizeMap(doc)) => {
                if let Some(map) = self.editor.map_of(doc) {
                    let size = glam::UVec2::new(map.width, map.height);
                    self.modals
                        .push(Modal::ResizeMap(ResizeMapDialog::new(doc, size)));
                }
            }
            Effect::Show(Dialog::OffsetMap(doc)) => self
                .modals
                .push(Modal::OffsetMap(OffsetMapDialog::new(doc, &self.editor))),
            Effect::Show(Dialog::CustomTypes) => self
                .modals
                .push(Modal::CustomTypes(Box::<CustomTypesDialog>::default())),
            Effect::PasteAtPointer(doc) => {
                let in_tileset = self
                    .editor
                    .document(doc)
                    .is_some_and(|d| d.tileset_editing().is_some());
                let at = if in_tileset {
                    self.tileset_editor.collision_pointer(doc)
                } else {
                    self.canvases
                        .get(&doc)
                        .and_then(crate::canvas::Canvas::hover)
                        .map(|hover| hover.pixel)
                };
                self.intents.push(Intent::PasteObjects { doc, at });
            }
            Effect::Show(Dialog::Preferences) => self.modals.open_preferences(&self.editor),
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
            Effect::ReadTilesetImage(job) => {
                let spawned = self.workers.spawn("tileset image", move || {
                    vec![Intent::TilesetImageRead(Box::new(job.run()))]
                });
                if let Err(problem) = spawned {
                    self.notify(Notice::error(problem));
                }
            }
            Effect::ExpandAutotiles(job) => {
                let spawned = self.workers.spawn("autotiles", move || {
                    vec![Intent::AutotilesExpanded(Box::new(job.run()))]
                });
                if let Err(problem) = spawned {
                    self.notify(Notice::error(problem));
                }
            }
        }
    }
}
