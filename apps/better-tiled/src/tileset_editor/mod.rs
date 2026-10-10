//! The tileset editor (`06-application.md`, §8), shown in place of a map while Edit Tileset
//! shows one of its tilesets, or in a tileset's own tab. A header names the tileset, goes back
//! to the map, and switches between the editor's modes; the tiles are drawn as the Tiles panel
//! draws them, to choose the ones edited (Shift picks a range, Command adds and removes);
//! Collision shows the current tile's shapes on a canvas of their own, edited with the object
//! tools; Animation edits the current tile's frames.

mod animation;
mod collision;
mod terrain;

use std::collections::HashMap;
use std::sync::Mutex;

use bt_core::{TileId, Tileset};
use bt_editor::{DocumentId, Editor, Intent, TilesetMode};
use bt_image::ImageStore;
use egui::{Sense, Stroke, StrokeKind, vec2};
use glam::DVec2;

use crate::canvas::CanvasGpu;
use crate::panels::{ShownTiles, TileGrid, TileImages, TileSource, cell_size, paint_tile};
use crate::tools::ToolBox;

use animation::AnimationEditor;
use collision::CollisionView;
use terrain::TerrainLabeller;

/// The zoom range of the tiles.
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=8.0;

/// The tileset editor's own state, per document.
#[derive(Debug, Default)]
pub(crate) struct TilesetEditorView {
    zoom: HashMap<DocumentId, f32>,
    collision: CollisionView,
    animation: AnimationEditor,
    terrain: TerrainLabeller,
    /// Where each document's tiles were drawn last.
    shown: HashMap<DocumentId, ShownTiles>,
}

/// What a frame of the tileset editor works with.
pub(crate) struct TilesetFrame<'a> {
    /// The editor.
    pub(crate) editor: &'a Editor,
    /// The document.
    pub(crate) doc: DocumentId,
    /// The tiles' textures.
    pub(crate) images: &'a mut TileImages,
    /// The decoded images.
    pub(crate) store: &'a Mutex<ImageStore>,
    /// The renderer of the collision view; `None` in the window's tests.
    pub(crate) gpu: Option<&'a CanvasGpu>,
    /// A text field has the keyboard.
    pub(crate) typing: bool,
    /// A dialog is open.
    pub(crate) dialog_open: bool,
}

impl TilesetEditorView {
    /// Shows the tileset editor of `frame.doc` in the rest of `ui`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: TilesetFrame<'_>,
        tools: &mut ToolBox,
        out: &mut Vec<Intent>,
    ) {
        let (Some(document), Some(tileset)) = (
            frame.editor.document(frame.doc),
            frame.editor.edited_tileset(frame.doc).cloned(),
        ) else {
            ui.weak("The tileset is not open any more.");
            return;
        };
        let Some(editing) = document.tileset_editing().cloned() else {
            return;
        };
        let in_map = frame.editor.map_of(frame.doc).is_some();
        ui.horizontal(|ui| {
            if in_map
                && ui
                    .button("◀ Map")
                    .on_hover_text("Back to the map")
                    .clicked()
            {
                out.push(Intent::StopEditingTileset { doc: frame.doc });
            }
            ui.strong(if tileset.name.is_empty() {
                "(unnamed tileset)"
            } else {
                tileset.name.as_str()
            });
            ui.separator();
            for (mode, label) in [
                (TilesetMode::Tiles, "Tiles"),
                (TilesetMode::Collision, "Collision"),
                (TilesetMode::Animation, "Animation"),
                (TilesetMode::Terrain, "Terrain"),
            ] {
                if ui.selectable_label(editing.mode() == mode, label).clicked() {
                    out.push(Intent::SetTilesetMode {
                        doc: frame.doc,
                        mode,
                    });
                }
            }
        });
        ui.separator();
        let doc = frame.doc;
        let mut zoom = self.zoom.get(&doc).copied().unwrap_or(2.0);
        let TilesetFrame {
            editor,
            images,
            store,
            gpu,
            typing,
            dialog_open,
            ..
        } = frame;
        let mut source = TileSource { images, store };
        let chosen = TilesChosen {
            tiles: editing.selected_tiles(),
            current: editing.current_tile(),
        };
        if editing.mode() == TilesetMode::Tiles {
            let shown = show_tiles(ui, &tileset, &mut zoom, chosen, &mut source, doc, out);
            self.remember_shown(doc, shown);
        } else if editing.mode() == TilesetMode::Terrain {
            let shown = self
                .terrain
                .show(ui, &tileset, &editing, doc, &mut zoom, &mut source, out);
            self.remember_shown(doc, shown);
        } else {
            let mut shown = None;
            egui::Panel::left("tileset-editor-tiles")
                .resizable(true)
                .default_size(ui.available_width() * 0.4)
                .show(ui, |ui| {
                    shown = show_tiles(ui, &tileset, &mut zoom, chosen, &mut source, doc, out);
                });
            self.remember_shown(doc, shown);
            let Some(tile) = editing.current_tile() else {
                ui.centered_and_justified(|ui| {
                    ui.weak("Choose a tile to edit.");
                });
                self.zoom.insert(doc, zoom);
                return;
            };
            match editing.mode() {
                TilesetMode::Collision => self.collision.show(
                    ui,
                    collision::CollisionFrame {
                        editor,
                        doc,
                        tileset: &tileset,
                        tile,
                        gpu,
                        typing,
                        dialog_open,
                    },
                    tools,
                    out,
                ),
                TilesetMode::Animation => {
                    self.animation
                        .show(ui, &tileset, tile, doc, &mut source, out);
                }
                TilesetMode::Tiles | TilesetMode::Terrain => {}
            }
        }
        self.zoom.insert(doc, zoom);
    }

    fn remember_shown(&mut self, doc: DocumentId, shown: Option<ShownTiles>) {
        match shown {
            Some(shown) => {
                self.shown.insert(doc, shown);
            }
            None => {
                self.shown.remove(&doc);
            }
        }
    }

    /// Where the middle of a tile of `doc`'s tileset is on screen (for the window's tests).
    #[cfg(test)]
    pub(crate) fn tile_on_screen(&self, doc: DocumentId, tile: TileId) -> Option<egui::Pos2> {
        self.shown.get(&doc)?.centre_of(tile)
    }

    /// Where a point of `doc`'s collision view is on screen (for the window's tests).
    #[cfg(test)]
    pub(crate) fn collision_world_on_screen(
        &self,
        doc: DocumentId,
        world: DVec2,
    ) -> Option<egui::Pos2> {
        self.collision.world_on_screen(doc, world)
    }

    /// Where the middle of a tile of the animation editor's palette is on screen (for the
    /// window's tests).
    #[cfg(test)]
    pub(crate) fn palette_tile_on_screen(&self, tile: TileId) -> Option<egui::Pos2> {
        self.animation.palette_tile_on_screen(tile)
    }

    /// Where the pointer is over the collision view of `doc`, in the tile's pixels.
    pub(crate) fn collision_pointer(&self, doc: DocumentId) -> Option<DVec2> {
        self.collision.pointer(doc)
    }

    /// The canvas of `doc`'s collision view, when it has one.
    pub(crate) fn collision_canvas(&self, doc: DocumentId) -> Option<&crate::canvas::Canvas> {
        self.collision.canvas(doc)
    }

    /// Forgets the state of documents that are closed.
    pub(crate) fn retain_documents(&mut self, open: &[DocumentId]) {
        self.zoom.retain(|doc, _| open.contains(doc));
        self.shown.retain(|doc, _| open.contains(doc));
        self.collision.retain_documents(open);
    }
}

/// The tiles chosen in the tileset editor.
#[derive(Copy, Clone, Debug)]
struct TilesChosen<'a> {
    tiles: &'a [TileId],
    current: Option<TileId>,
}

/// The tiles drawn: where, and the grid's response to the pointer.
pub(crate) struct DrawnTiles {
    /// The grid's response.
    pub(crate) response: egui::Response,
    /// Where the tiles are.
    pub(crate) shown: ShownTiles,
}

/// Draws the tileset's tiles with a zoom slider, each tile passed to `overlay` to draw over;
/// the grid answers the pointer with `sense`. `None` when the tileset has no tiles.
pub(crate) fn draw_tiles(
    ui: &mut egui::Ui,
    tileset: &Tileset,
    zoom: &mut f32,
    source: &mut TileSource<'_>,
    sense: Sense,
    mut overlay: impl FnMut(&egui::Ui, TileId, egui::Rect),
) -> Option<DrawnTiles> {
    ui.add(
        egui::Slider::new(zoom, ZOOM_RANGE)
            .logarithmic(true)
            .text("Zoom"),
    );
    let ids = tileset.tile_ids();
    if ids.is_empty() {
        ui.weak("The tileset has no tiles.");
        return None;
    }
    let cell = cell_size(tileset, &ids) * *zoom;
    let columns = match tileset.columns() {
        0 => ((ui.available_width() / cell.x).floor() as usize).max(1),
        columns => columns as usize,
    };
    let mut drawn = None;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            let mut grid = TileGrid {
                origin: ui.min_rect().min,
                cell,
                columns,
                count: ids.len(),
            };
            let size = vec2(columns as f32 * cell.x, grid.rows() as f32 * cell.y);
            let (area, response) = ui.allocate_exact_size(size, sense);
            grid.origin = area.min;
            let first_row = (viewport.min.y / cell.y).floor().max(0.0) as usize;
            let end_row = ((viewport.max.y / cell.y).ceil().max(0.0) as usize).min(grid.rows());
            for row in first_row..end_row {
                for column in 0..columns {
                    let index = row * columns + column;
                    let Some(id) = ids.get(index).copied() else {
                        break;
                    };
                    let rect = grid.rect(index);
                    paint_tile(ui, tileset, id, rect, source);
                    overlay(ui, id, rect);
                }
            }
            drawn = Some(DrawnTiles {
                response,
                shown: ShownTiles {
                    grid,
                    ids: ids.clone(),
                },
            });
        });
    drawn
}

/// Draws the tileset's tiles with a zoom slider; a click chooses a tile, Shift a range from the
/// current one, Command adds or removes one. Where the tiles were drawn, if any were.
fn show_tiles(
    ui: &mut egui::Ui,
    tileset: &Tileset,
    zoom: &mut f32,
    chosen: TilesChosen<'_>,
    source: &mut TileSource<'_>,
    doc: DocumentId,
    out: &mut Vec<Intent>,
) -> Option<ShownTiles> {
    let selection = ui.visuals().selection.stroke.color;
    let drawn = draw_tiles(ui, tileset, zoom, source, Sense::click(), |ui, id, rect| {
        let animated = tileset.tile(id).is_some_and(|d| !d.animation.is_empty());
        let shaped = tileset.tile(id).is_some_and(|d| d.collision.is_some());
        if animated || shaped {
            let mark = if animated { "▶" } else { "◇" };
            ui.painter().text(
                rect.right_top() + vec2(-2.0, 1.0),
                egui::Align2::RIGHT_TOP,
                mark,
                egui::FontId::proportional(10.0),
                ui.visuals().strong_text_color(),
            );
        }
        if chosen.tiles.contains(&id) {
            let width = if chosen.current == Some(id) { 3.0 } else { 2.0 };
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                0.0,
                Stroke::new(width, selection),
                StrokeKind::Inside,
            );
        }
    })?;
    let DrawnTiles { response, shown } = drawn;
    let ids = &shown.ids;
    if response.clicked()
        && let Some(id) = response
            .interact_pointer_pos()
            .and_then(|at| shown.grid.index_at(at))
            .and_then(|index| ids.get(index).copied())
    {
        let modifiers = ui.input(|input| input.modifiers);
        out.push(choose_tile(ids, chosen, id, modifiers, doc));
    }
    if let Some(id) = response
        .hover_pos()
        .and_then(|at| shown.grid.index_at(at))
        .and_then(|index| ids.get(index).copied())
    {
        response.on_hover_text(format!("Tile {}", id.0));
    }
    Some(shown)
}

/// The selection a click on `id` makes: that tile; with Shift the tiles from the current one to
/// it; with Command the selection with it added or taken out. The tile clicked is current.
fn choose_tile(
    ids: &[TileId],
    chosen: TilesChosen<'_>,
    id: TileId,
    modifiers: egui::Modifiers,
    doc: DocumentId,
) -> Intent {
    let tiles = if modifiers.shift {
        let at = |tile: TileId| ids.iter().position(|t| *t == tile);
        match (chosen.current.and_then(at), at(id)) {
            (Some(from), Some(to)) => ids
                .get(from.min(to)..=from.max(to))
                .map(<[TileId]>::to_vec)
                .unwrap_or_else(|| vec![id]),
            _ => vec![id],
        }
    } else if modifiers.command {
        let mut tiles = chosen.tiles.to_vec();
        match tiles.iter().position(|t| *t == id) {
            Some(at) => {
                tiles.remove(at);
            }
            None => tiles.push(id),
        }
        tiles
    } else {
        vec![id]
    };
    let current = tiles.contains(&id).then_some(id).or(tiles.first().copied());
    Intent::SelectTilesetTiles {
        doc,
        tiles,
        current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen_after(chosen: TilesChosen<'_>, id: u32, modifiers: egui::Modifiers) -> Intent {
        let ids: Vec<TileId> = (0..6).map(TileId).collect();
        choose_tile(&ids, chosen, TileId(id), modifiers, DocumentId(1))
    }

    fn tiles_of(intent: Intent) -> (Vec<u32>, Option<u32>) {
        match intent {
            Intent::SelectTilesetTiles { tiles, current, .. } => (
                tiles.into_iter().map(|t| t.0).collect(),
                current.map(|t| t.0),
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn clicks_choose_tiles_as_lists_do() {
        let none = TilesChosen {
            tiles: &[],
            current: None,
        };
        assert_eq!(
            tiles_of(chosen_after(none, 2, egui::Modifiers::NONE)),
            (vec![2], Some(2))
        );
        let two = [TileId(2)];
        let from_two = TilesChosen {
            tiles: &two,
            current: Some(TileId(2)),
        };
        assert_eq!(
            tiles_of(chosen_after(from_two, 4, egui::Modifiers::SHIFT)),
            (vec![2, 3, 4], Some(4)),
            "Shift: the range from the current tile"
        );
        assert_eq!(
            tiles_of(chosen_after(from_two, 5, egui::Modifiers::COMMAND)),
            (vec![2, 5], Some(5)),
            "Command adds"
        );
        assert_eq!(
            tiles_of(chosen_after(from_two, 2, egui::Modifiers::COMMAND)),
            (vec![], None),
            "and takes out"
        );
    }
}
