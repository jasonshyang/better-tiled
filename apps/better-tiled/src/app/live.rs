//! The window's side of the live link's jobs (`11-live-link.md`, §5, §9–§11): each frame, in
//! `logic`, the jobs `bt`'s connections asked for are done against the editor and the canvas.
//! An edit or an undo waits while a stroke or drag is under way, an opened map is answered once
//! its document is there, and a screenshot once its picture arrives; any of them is dropped at
//! its deadline, never done unseen.

use std::path::{Path, PathBuf};

use bt_core::Map;
use bt_editor::{
    AssetKind, Document, DocumentId, Effect, ExternalUndo, Intent, MarkCells, Severity, ToolKind,
    ViewRequest, tool_action,
};
use bt_live::{
    DocStatus, Done, Job, KeyboardFocus, MapText, MarkSpec, Reply, Response, Status, TilesetText,
    runs_from_cells,
};
use bt_script::Names;
use bt_tmx::WriteRequest;
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2};

use super::frame::App;
use crate::focus::is_typing;
use crate::live::{Answered, LiveAnswer, LiveJob};

/// How many of a document's last undo steps `bt editor` lists.
const HISTORY_SHOWN: usize = 6;

/// A job the window holds over frames until it can be answered.
pub(crate) enum HeldJob {
    /// An edit or an undo waiting for a stroke or drag to end.
    Waiting(Job<LiveJob, LiveAnswer>),
    /// A map being opened: answered when its document is there.
    Opening {
        /// The map.
        path: PathBuf,
        /// Where the answer goes.
        reply: Reply<LiveAnswer>,
    },
    /// The window's picture asked for: answered when it arrives.
    Screenshot {
        /// The PNG to write.
        out: PathBuf,
        /// Where the answer goes.
        reply: Reply<LiveAnswer>,
    },
}

impl App {
    /// Answers `bt`: listens or not as the preference says, and does the jobs its connections
    /// asked for. Runs in `logic`, so it answers while the window is hidden.
    pub(super) fn serve_live_link(&mut self, ctx: &egui::Context) {
        let enabled = self.editor.settings().general.live_link;
        if let Some(problem) = self.live.keep_listening(enabled) {
            self.notify(bt_editor::Notice::warning(problem));
        }
        let held = std::mem::take(&mut self.live_held);
        for job in held {
            if let Some(still) = self.continue_held(ctx, job) {
                self.live_held.push(still);
            }
        }
        for job in self.live.take_jobs() {
            if job.reply.is_overdue() {
                continue;
            }
            if let Some(held) = self.do_live_job(ctx, job) {
                self.live_held.push(held);
            }
        }
        if !self.live_held.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    /// Does a job, or holds it when it cannot be answered yet.
    fn do_live_job(
        &mut self,
        ctx: &egui::Context,
        job: Job<LiveJob, LiveAnswer>,
    ) -> Option<HeldJob> {
        let Job { kind, reply } = job;
        let answer = match kind {
            LiveJob::Status => Ok(Answered::Response(Response::Status(self.live_status(ctx)))),
            LiveJob::Snapshot { path } => self
                .snapshot(&path)
                .map(|text| Answered::Response(Response::MapText(text))),
            LiveJob::Prepare { path } => self
                .open_document(&path)
                .and_then(|doc| {
                    self.editor
                        .prepare_external_edit(doc)
                        .ok_or_else(|| not_a_map(&path))
                })
                .map(|base| Answered::Base(Box::new(base))),
            kind @ (LiveJob::Apply { .. } | LiveJob::Undo { .. }) => {
                return self.edit_or_wait(ctx, Job { kind, reply });
            }
            LiveJob::Mark {
                path,
                marks,
                add,
                focus,
            } => self
                .mark(&path, marks, add, focus)
                .map(|done| Answered::Response(Response::Done(done))),
            LiveJob::Open { path } => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                if let Some(doc) = self.editor.document_at(&path) {
                    self.intents.push(Intent::SelectDocument(doc));
                    answered(done(format!("{} is open in the editor", display(&path))))
                } else {
                    self.intents.push(Intent::Open(path.clone()));
                    return Some(HeldJob::Opening { path, reply });
                }
            }
            LiveJob::Save { path } => self.save_for_bt(ctx, &path),
            LiveJob::Screenshot { out } => {
                let hidden = ctx.input(|input| input.viewport().minimized == Some(true));
                if hidden {
                    Err(
                        "the editor's window is hidden, so it has no picture: bt render draws the \
                         map instead"
                            .to_owned(),
                    )
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                    return Some(HeldJob::Screenshot { out, reply });
                }
            }
        };
        reply.send(answer);
        None
    }

    /// Goes on with a held job: answers it when it can, holds it again otherwise.
    fn continue_held(&mut self, ctx: &egui::Context, held: HeldJob) -> Option<HeldJob> {
        match held {
            HeldJob::Waiting(job) => {
                if job.reply.is_overdue() {
                    return None;
                }
                self.edit_or_wait(ctx, job)
            }
            HeldJob::Opening { path, reply } => {
                if reply.is_overdue() {
                    return None;
                }
                if self.editor.document_at(&path).is_some() {
                    reply.send(answered(done(format!(
                        "opened {} in the editor",
                        display(&path)
                    ))));
                    return None;
                }
                Some(HeldJob::Opening { path, reply })
            }
            HeldJob::Screenshot { out, reply } => {
                let picture = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot { image, .. } => Some(std::sync::Arc::clone(image)),
                        _ => None,
                    })
                });
                let Some(picture) = picture else {
                    return (!reply.is_overdue()).then_some(HeldJob::Screenshot { out, reply });
                };
                let written = crate::dev::save_png(&picture, &out).and_then(|()| {
                    answered(done(format!(
                        "wrote the editor's window to {}",
                        out.display()
                    )))
                });
                reply.send(written);
                None
            }
        }
    }

    /// Lands an edit or takes one back, or holds the job while a stroke or drag is under way.
    fn edit_or_wait(
        &mut self,
        ctx: &egui::Context,
        job: Job<LiveJob, LiveAnswer>,
    ) -> Option<HeldJob> {
        if self.editor.is_stroking() {
            return Some(HeldJob::Waiting(job));
        }
        let Job { kind, reply } = job;
        let answer = match kind {
            LiveJob::Apply { edit, area } => {
                let doc = edit.doc;
                let label = edit.label.clone();
                let result = self.editor.apply_external_edit(*edit);
                let applied = match result {
                    Ok(effects) => {
                        self.carry_out_all(ctx, effects);
                        if let (Some(area), Some(canvas)) = (area, self.canvases.get_mut(&doc)) {
                            canvas.outline_bt_edit(area);
                        }
                        self.notify(bt_editor::Notice::info(format!("bt: {label}")));
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                Ok(Answered::Applied(applied))
            }
            LiveJob::Undo { path } => self.undo_for_bt(ctx, &path),
            _ => Err("the editor was asked something it does not hold".to_owned()),
        };
        reply.send(answer);
        None
    }

    /// The document showing the map at `path`, or why there is none.
    fn open_document(&self, path: &Path) -> Result<DocumentId, String> {
        self.editor
            .document_at(path)
            .ok_or_else(|| format!("{} is not open in the editor", display(path)))
    }

    /// `bt undo` of a map open in the editor.
    fn undo_for_bt(&mut self, ctx: &egui::Context, path: &Path) -> LiveAnswer {
        let doc = self.open_document(path)?;
        let undone = self
            .editor
            .external_undo(doc)
            .map_err(|error| error.to_string())?;
        self.carry_out_all(ctx, undone.effects);
        let (message, nothing) = match undone.undo {
            ExternalUndo::Undone { label, earlier } => (
                format!(
                    "took back “{label}” in the editor; {}",
                    earlier_edits(earlier)
                ),
                false,
            ),
            ExternalUndo::AlreadyUndone { label } => {
                (format!("“{label}” was undone in the editor already"), false)
            }
            ExternalUndo::Nothing => ("no bt edit to take back in the editor".to_owned(), true),
        };
        Ok(Answered::Response(Response::Done(Done {
            message,
            nothing,
        })))
    }

    /// `bt save`: the editor's own save of the map; an external tileset of the map that could
    /// not be written is an error naming it, with the map saved.
    fn save_for_bt(&mut self, ctx: &egui::Context, path: &Path) -> LiveAnswer {
        let doc = self.open_document(path)?;
        let effects = self.editor.apply(Intent::SaveDocument(doc));
        let problems: Vec<String> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify(notice) if notice.severity != Severity::Info => {
                    Some(notice.text.clone())
                }
                _ => None,
            })
            .collect();
        self.carry_out_all(ctx, effects);
        let store = self.editor.store();
        let asset = self.editor.document(doc).map(Document::asset);
        if asset
            .and_then(|a| store.get(a))
            .is_some_and(|a| a.is_dirty())
        {
            return Err(if problems.is_empty() {
                format!("{} was not saved", display(path))
            } else {
                problems.join("; ")
            });
        }
        let unwritten: Vec<String> = self
            .editor
            .map_of(doc)
            .map(|map| {
                map.tilesets
                    .iter()
                    .filter_map(|reference| reference.file.as_ref())
                    .filter(|file| {
                        store
                            .by_path(&file.path)
                            .and_then(|id| store.get(id))
                            .is_some_and(|tileset| tileset.is_dirty())
                    })
                    .map(|file| display(&file.path))
                    .collect()
            })
            .unwrap_or_default();
        if !unwritten.is_empty() {
            return Err(format!(
                "{} was saved, but its tileset {} could not be written{}",
                display(path),
                unwritten.join(", "),
                if problems.is_empty() {
                    String::new()
                } else {
                    format!(": {}", problems.join("; "))
                }
            ));
        }
        answered(done(format!("saved {}", display(path))))
    }

    /// `bt mark`: outlines on a map's canvas; with `focus` the view moves to show them when
    /// the map is the one shown (the editor never switches the map the person looks at).
    fn mark(
        &mut self,
        path: &Path,
        marks: Vec<MarkCells>,
        add: bool,
        focus: bool,
    ) -> Result<Done, String> {
        let doc = self.open_document(path)?;
        let cleared = marks.is_empty() && !add;
        let bounds = marks
            .iter()
            .flat_map(|mark| mark.cells.iter().copied())
            .fold(None, |bounds: Option<IRect>, cell| {
                let one = IRect::from_min_size(cell, IVec2::ONE);
                Some(bounds.map_or(one, |b| b.union(one)))
            });
        let count = self.editor.set_marks(doc, marks, add).len();
        if cleared {
            return Ok(done(format!("cleared the marks on {}", display(path))));
        }
        let shown = self.editor.current() == Some(doc);
        let marked = format!(
            "{} {} on {}",
            count,
            if count == 1 { "mark" } else { "marks" },
            display(path)
        );
        if !focus {
            return Ok(done(marked));
        }
        if !shown {
            return Ok(done(format!(
                "{marked}; the editor shows another map, so they wait there"
            )));
        }
        let (Some(bounds), Some(map)) = (bounds, self.editor.shown_map(doc)) else {
            return Ok(done(marked));
        };
        let world = world_rect(map, bounds);
        let canvas = self.canvases.get_mut(&doc);
        let in_view = canvas
            .as_ref()
            .and_then(|canvas| canvas.last_view())
            .is_some_and(|view| visible_world(&view).intersection(world) == world);
        if in_view {
            return Ok(done(format!("{marked}; in view already")));
        }
        let map = std::sync::Arc::clone(map);
        if let Some(canvas) = canvas {
            canvas.apply_view_request(ViewRequest::CenterOn((world.min + world.max) / 2.0), &map);
        }
        Ok(done(format!("{marked}; the view moved to show them")))
    }

    /// A map as the editor shows it, as TMX text, with its unsaved external tilesets as TSX.
    fn snapshot(&self, path: &Path) -> Result<MapText, String> {
        let doc = self.open_document(path)?;
        let map = self.editor.shown_map(doc).ok_or_else(|| not_a_map(path))?;
        let rendered = WriteRequest::for_map(map, path)
            .render()
            .map_err(|error| format!("{} could not be written as text: {error}", display(path)))?;
        let store = self.editor.store();
        let mut tilesets = Vec::new();
        for file in map.tilesets.iter().filter_map(|t| t.file.as_ref()) {
            let Some(asset) = store.by_path(&file.path).and_then(|id| store.get(id)) else {
                continue;
            };
            let (true, Some(tileset)) = (asset.is_dirty(), store.tileset(asset.id())) else {
                continue;
            };
            let tsx = WriteRequest::for_tileset(tileset, &file.path)
                .render()
                .map_err(|error| {
                    format!(
                        "{} could not be written as text: {error}",
                        display(&file.path)
                    )
                })?;
            tilesets.push(TilesetText {
                path: file.path.clone(),
                tsx: tsx.text,
            });
        }
        let dirty = self
            .editor
            .document(doc)
            .and_then(|d| store.get(d.asset()))
            .is_some_and(|asset| asset.is_dirty());
        Ok(MapText {
            dirty,
            map: rendered.text,
            tilesets,
        })
    }

    /// What `bt editor` reports: the open documents, the current one first among equals.
    fn live_status(&self, ctx: &egui::Context) -> Status {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
        let current = self.editor.current();
        let documents = self
            .editor
            .documents()
            .in_order()
            .map(|document| self.doc_status(ctx, document, current == Some(document.id())))
            .collect();
        Status::of_this_process(focused, documents)
    }

    /// One document as `bt editor` reports it.
    fn doc_status(&self, ctx: &egui::Context, document: &Document, active: bool) -> DocStatus {
        let doc = document.id();
        let store = self.editor.store();
        let asset = store.get(document.asset());
        let map = self.editor.shown_map(doc);
        let names = map.map(|map| Names::of(map));
        let selection = document.selection();
        let layer = selection
            .current_layer
            .and_then(|id| map.and_then(|map| map.layer(id)))
            .map(|layer| layer.name.clone());
        let tool = tool_action(self.editor.tools().tool())
            .spec()
            .label
            .to_owned();
        let canvas = self.canvases.get(&doc);
        let view = canvas
            .and_then(|canvas| canvas.last_view())
            .zip(map)
            .and_then(|(view, map)| cells_in_view(map, &visible_world(&view)));
        let history = document.history();
        let done_steps: Vec<String> = history
            .entries()
            .into_iter()
            .filter(|entry| entry.done)
            .map(|entry| entry.label)
            .collect();
        let shown_history = done_steps
            .iter()
            .skip(done_steps.len().saturating_sub(HISTORY_SHOWN))
            .cloned()
            .collect();
        DocStatus {
            path: asset.and_then(|a| a.path().map(Path::to_path_buf)),
            title: asset.map_or_else(String::new, |a| a.display_name()),
            active,
            dirty: asset.is_some_and(|a| a.is_dirty()),
            tileset: asset.is_some_and(|a| a.kind() == AssetKind::Tileset),
            layer,
            tool,
            brush: names.as_ref().and_then(|names| self.brush_text(doc, names)),
            selection: runs_from_cells(selection.tiles.iter()),
            objects: selection.objects.iter().map(|id| id.0).collect(),
            view,
            mouse: canvas
                .and_then(|canvas| canvas.hover())
                .map(|hover| hover.cell),
            history: shown_history,
            undo_steps: history.position(),
            marks: self
                .editor
                .marks(doc)
                .iter()
                .map(|mark| MarkSpec {
                    runs: runs_from_cells(mark.cells.iter().copied()),
                    label: mark.label.clone(),
                })
                .collect(),
            world_tool: false,
            keyboard: active.then(|| self.keyboard_focus(ctx, document)),
        }
    }

    /// What the tool paints with, in `bt`'s names: a terrain, a tile, a stamp, an eraser.
    fn brush_text(&self, doc: DocumentId, names: &Names) -> Option<String> {
        let tools = self.editor.tools();
        match tools.tool() {
            ToolKind::Terrain => {
                let in_use = self.editor.terrain_in_use(doc).ok()?;
                let terrain = &in_use.terrain;
                Some(
                    names
                        .terrain_of(terrain.slot(), terrain.set_index(), in_use.color)
                        .map_or_else(|| in_use.name(in_use.color), |t| t.name.clone()),
                )
            }
            ToolKind::Eraser => {
                let size = tools.brush_size();
                Some(format!("eraser {size}x{size}"))
            }
            ToolKind::Brush | ToolKind::BucketFill | ToolKind::ShapeFill => {
                let brush = tools.brush()?;
                let size = brush.variations().first()?.clip.size;
                if size == IVec2::ONE
                    && let Some(cell) = self.editor.brush_tile(doc)
                {
                    return Some(names.tile_text(cell));
                }
                Some(format!("{}x{} stamp", size.x, size.y))
            }
            _ => None,
        }
    }

    /// Where the keyboard's keys go.
    fn keyboard_focus(&self, ctx: &egui::Context, document: &Document) -> KeyboardFocus {
        if self.modals.is_open() {
            KeyboardFocus::Dialog
        } else if is_typing(ctx) {
            KeyboardFocus::TextField
        } else if document.tileset_editing().is_some() {
            KeyboardFocus::TilesetCanvas
        } else {
            KeyboardFocus::Map
        }
    }
}

/// What was done, in a sentence for the person.
fn done(message: String) -> Done {
    Done {
        message,
        nothing: false,
    }
}

/// `done` as the window's answer.
fn answered(done: Done) -> LiveAnswer {
    Ok(Answered::Response(Response::Done(done)))
}

/// A path's file name, for messages.
fn display(path: &Path) -> String {
    bt_utils::paths::display_name(path)
}

/// Why a document shows no map.
fn not_a_map(path: &Path) -> String {
    format!("{} is not a map open in the editor", display(path))
}

/// "N earlier bt edits can be taken back".
fn earlier_edits(earlier: usize) -> String {
    match earlier {
        0 => "no earlier bt edit is left to take back there".to_owned(),
        1 => "1 earlier bt edit can be taken back there".to_owned(),
        n => format!("{n} earlier bt edits can be taken back there"),
    }
}

/// The world rectangle a view shows.
fn visible_world(view: &crate::canvas::ViewTransform) -> DRect {
    let size = DVec2::new(f64::from(view.rect.width()), f64::from(view.rect.height()))
        * view.pixels_per_point
        / view.zoom_px;
    DRect::from_min_size(view.origin, size)
}

/// The world rectangle holding a rectangle of cells.
fn world_rect(map: &Map, cells: IRect) -> DRect {
    let geometry = map.geometry();
    let corners: Vec<DVec2> = [
        cells.min(),
        IVec2::new(cells.max().x - 1, cells.min().y),
        IVec2::new(cells.min().x, cells.max().y - 1),
        cells.max() - IVec2::ONE,
    ]
    .into_iter()
    .flat_map(|cell| geometry.tile_polygon(cell))
    .collect();
    DRect::from_points(corners).unwrap_or_else(|| DRect::from_min_size(DVec2::ZERO, DVec2::ZERO))
}

/// The cells a world rectangle shows, within a finite map.
fn cells_in_view(map: &Map, world: &DRect) -> Option<IRect> {
    let geometry = map.geometry();
    let corners = [
        world.min,
        DVec2::new(world.max.x, world.min.y),
        DVec2::new(world.min.x, world.max.y),
        world.max,
    ];
    let cells = IRect::bounding(corners.into_iter().map(|corner| geometry.tile_at(corner)))?;
    if map.infinite {
        return Some(cells);
    }
    let shown = cells.intersection(map.tile_bounding_rect());
    (!shown.is_empty()).then_some(shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_shows_the_cells_under_it() {
        let map =
            bt_test_data::fixtures::MapBuilder::new(glam::UVec2::splat(16), glam::UVec2::splat(10))
                .build()
                .unwrap();
        let world = DRect::from_min_size(DVec2::new(8.0, 8.0), DVec2::new(32.0, 16.0));
        assert_eq!(
            cells_in_view(&map, &world),
            Some(IRect::from_min_size(IVec2::ZERO, IVec2::new(3, 2)))
        );
        let outside = DRect::from_min_size(DVec2::splat(-100.0), DVec2::splat(10.0));
        assert_eq!(cells_in_view(&map, &outside), None);
        let around = world_rect(
            &map,
            IRect::from_min_size(IVec2::new(1, 1), IVec2::new(2, 2)),
        );
        assert_eq!(
            around,
            DRect::from_min_size(DVec2::splat(16.0), DVec2::splat(32.0))
        );
    }
}
