//! The tileset editor's animation editor (Tiled's TileAnimationEditor,
//! `reference/tiled/src/tiled/tileanimationeditor.cpp`): the current tile's frames in order,
//! each with its duration; a click on a tile of the palette adds it as a frame (after the
//! selected one, else at the end), and the selected frame moves left or right or goes; its
//! duration can be given to every frame; a preview plays the animation. Every change is one
//! step.

use std::collections::HashMap;

use bt_core::{Frame, TileId, Tileset};
use bt_editor::{DocumentId, Intent};
use egui::{Sense, Stroke, StrokeKind, vec2};

use crate::panels::{TileGrid, TileSource, paint_tile};

/// The duration of a frame added from the palette (Tiled's default).
const DEFAULT_DURATION_MS: u32 = 100;

/// The side of a frame's picture, in points.
const FRAME_SIDE: f32 = 48.0;

/// The side of a palette tile, in points.
const PALETTE_SIDE: f32 = 32.0;

/// The animation editor's own state.
#[derive(Debug, Default)]
pub(crate) struct AnimationEditor {
    /// The frame selected in each document.
    selected: HashMap<DocumentId, usize>,
    /// A duration being dragged or typed, applied when the edit ends.
    pending: Option<PendingDuration>,
    /// Where the palette was drawn last.
    palette: Option<crate::panels::ShownTiles>,
}

/// A frame's duration being edited.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PendingDuration {
    doc: DocumentId,
    tile: TileId,
    index: usize,
    duration_ms: u32,
}

impl AnimationEditor {
    /// Shows the animation of `tile` in the rest of `ui`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        tileset: &Tileset,
        tile: TileId,
        doc: DocumentId,
        source: &mut TileSource<'_>,
        out: &mut Vec<Intent>,
    ) {
        let frames = tileset
            .tile(tile)
            .map(|data| data.animation.clone())
            .unwrap_or_default();
        let set = |frames: Vec<Frame>| Intent::SetTileAnimation { doc, tile, frames };
        let selected = self
            .selected
            .get(&doc)
            .copied()
            .filter(|index| *index < frames.len());
        ui.horizontal(|ui| {
            ui.strong(format!("Animation of tile {}", tile.0));
            let (preview, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            let time_ms = (ui.input(|input| input.time) * 1000.0) as u64;
            paint_tile(
                ui,
                tileset,
                tileset.animated_tile(tile, time_ms),
                preview,
                source,
            );
            if let Some(left) = until_next_frame(&frames, time_ms) {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(left));
            }
        });
        ui.separator();
        if frames.is_empty() {
            ui.weak("No frames: click tiles below to add them.");
        }
        ui.horizontal_wrapped(|ui| {
            for (index, frame) in frames.iter().enumerate() {
                ui.vertical(|ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(FRAME_SIDE, FRAME_SIDE), Sense::click());
                    paint_tile(ui, tileset, frame.tile, rect, source);
                    if selected == Some(index) {
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(2.0, ui.visuals().selection.stroke.color),
                            StrokeKind::Inside,
                        );
                    }
                    if response
                        .on_hover_text(format!("Frame {}: tile {}", index + 1, frame.tile.0))
                        .clicked()
                    {
                        self.selected.insert(doc, index);
                    }
                    let shown = match self.pending {
                        Some(p) if p.doc == doc && p.tile == tile && p.index == index => {
                            p.duration_ms
                        }
                        _ => frame.duration_ms,
                    };
                    let mut duration = shown;
                    let field = ui.add(
                        egui::DragValue::new(&mut duration)
                            .range(1..=60_000)
                            .suffix(" ms"),
                    );
                    if duration != shown {
                        self.pending = Some(PendingDuration {
                            doc,
                            tile,
                            index,
                            duration_ms: duration,
                        });
                    }
                    let finished = field.drag_stopped() || field.lost_focus();
                    if let Some(pending) = self
                        .pending
                        .filter(|p| finished && p.doc == doc && p.tile == tile && p.index == index)
                    {
                        self.pending = None;
                        if pending.duration_ms != frame.duration_ms {
                            let mut changed = frames.clone();
                            if let Some(edited) = changed.get_mut(index) {
                                edited.duration_ms = pending.duration_ms;
                            }
                            out.push(set(changed));
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            let has = selected.is_some();
            let mut moved = |from: usize, to: usize| {
                let mut changed = frames.clone();
                changed.swap(from, to);
                self.selected.insert(doc, to);
                out.push(set(changed));
            };
            if ui
                .add_enabled(has, egui::Button::new("◀"))
                .on_hover_text("Earlier")
                .clicked()
                && let Some(index) = selected.filter(|index| *index > 0)
            {
                moved(index, index - 1);
            }
            if ui
                .add_enabled(has, egui::Button::new("▶"))
                .on_hover_text("Later")
                .clicked()
                && let Some(index) = selected.filter(|index| index + 1 < frames.len())
            {
                moved(index, index + 1);
            }
            if ui.add_enabled(has, egui::Button::new("Remove")).clicked()
                && let Some(index) = selected
            {
                let mut changed = frames.clone();
                changed.remove(index);
                out.push(set(changed));
            }
            let duration = selected
                .and_then(|index| frames.get(index))
                .map(|frame| frame.duration_ms);
            if ui
                .add_enabled(
                    duration.is_some(),
                    egui::Button::new("Same Duration for All"),
                )
                .clicked()
                && let Some(duration_ms) = duration
            {
                let changed = frames
                    .iter()
                    .map(|frame| Frame {
                        duration_ms,
                        ..*frame
                    })
                    .collect();
                out.push(set(changed));
            }
            if ui
                .add_enabled(!frames.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                out.push(set(Vec::new()));
            }
        });
        ui.separator();
        ui.weak("Click a tile to add it as a frame:");
        let (added, shown) = palette(ui, tileset, source);
        self.palette = shown;
        if let Some(added) = added {
            let duration_ms = selected
                .and_then(|index| frames.get(index))
                .map_or(DEFAULT_DURATION_MS, |frame| frame.duration_ms);
            let at = selected.map_or(frames.len(), |index| index + 1);
            let mut changed = frames.clone();
            changed.insert(
                at,
                Frame {
                    tile: added,
                    duration_ms,
                },
            );
            self.selected.insert(doc, at);
            out.push(set(changed));
        }
    }
}

impl AnimationEditor {
    /// Where the middle of a palette tile is on screen.
    #[cfg(test)]
    pub(crate) fn palette_tile_on_screen(&self, tile: TileId) -> Option<egui::Pos2> {
        self.palette.as_ref()?.centre_of(tile)
    }
}

/// How long the frame shown `time_ms` into an animation stays, in milliseconds; `None` for a
/// tile that does not change.
fn until_next_frame(frames: &[Frame], time_ms: u64) -> Option<u64> {
    let total: u64 = frames.iter().map(|f| u64::from(f.duration_ms)).sum();
    if frames.len() < 2 || total == 0 {
        return None;
    }
    let into = time_ms % total;
    let mut end = 0;
    for frame in frames {
        end += u64::from(frame.duration_ms);
        if into < end {
            return Some(end - into);
        }
    }
    None
}

/// The tileset's tiles, small: the tile clicked, if any, and where they were drawn.
fn palette(
    ui: &mut egui::Ui,
    tileset: &Tileset,
    source: &mut TileSource<'_>,
) -> (Option<TileId>, Option<crate::panels::ShownTiles>) {
    let ids = tileset.tile_ids();
    let cell = vec2(PALETTE_SIDE, PALETTE_SIDE);
    let columns = ((ui.available_width() / cell.x).floor() as usize).max(1);
    let mut clicked = None;
    let mut shown = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut grid = TileGrid {
                origin: ui.min_rect().min,
                cell,
                columns,
                count: ids.len(),
            };
            let size = vec2(columns as f32 * cell.x, grid.rows() as f32 * cell.y);
            let (area, response) = ui.allocate_exact_size(size, Sense::click());
            grid.origin = area.min;
            for (index, id) in ids.iter().enumerate() {
                paint_tile(ui, tileset, *id, grid.rect(index), source);
            }
            if response.clicked() {
                clicked = response
                    .interact_pointer_pos()
                    .and_then(|at| grid.index_at(at))
                    .and_then(|index| ids.get(index).copied());
            }
            shown = Some(crate::panels::ShownTiles {
                grid,
                ids: ids.clone(),
            });
        });
    (clicked, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_repaints_when_its_frame_ends() {
        let frames = [
            Frame {
                tile: TileId(0),
                duration_ms: 100,
            },
            Frame {
                tile: TileId(1),
                duration_ms: 50,
            },
        ];
        assert_eq!(until_next_frame(&frames, 30), Some(70));
        assert_eq!(until_next_frame(&frames, 120), Some(30));
        assert_eq!(until_next_frame(&frames, 150), Some(100), "round again");
        assert_eq!(
            until_next_frame(&frames[..1], 30),
            None,
            "one frame never changes"
        );
    }
}
