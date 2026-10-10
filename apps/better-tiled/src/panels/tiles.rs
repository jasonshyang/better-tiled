//! The Tiles panel: the map's tilesets as tabs, and the current one's tiles at the panel's zoom
//! (an atlas in its own columns, a collection wrapped to the panel). Only the rows in view are
//! drawn. Clicking a tile, or dragging over a rectangle of them, makes them the brush, laid out
//! as they are in the tileset. Edit Tileset opens the tileset editor in place of the map.

use std::collections::HashMap;
use std::sync::Mutex;

use bt_core::{Cell, Map, PlacedCell, Tileset, TilesetImage, TilesetRef, TilesetSlot};
use bt_editor::{DocumentId, Editor, Intent};
use bt_image::ImageStore;
use egui::{Pos2, Sense, Stroke, StrokeKind, vec2};

use super::tile_grid::{TileGrid, TileSource, cell_size, paint_tile};
use super::tile_images::{TileImages, TileTexture};

/// The zoom range of the panel.
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The panel's own state.
#[derive(Debug, Default)]
pub(crate) struct TilesPanel {
    current: HashMap<DocumentId, usize>,
    zoom: HashMap<String, f32>,
    /// The tiles picked last in each tileset, as places in the panel's grid.
    picked: HashMap<String, TilePick>,
    /// A drag over tiles under way: the place it started at.
    dragging: Option<usize>,
}

/// A rectangle of tiles in the panel's grid, from one place to another (any corners).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TilePick {
    from: usize,
    to: usize,
}

impl TilePick {
    /// The places inside, row by row, with their column and row from the rectangle's corner.
    fn places(self, columns: usize) -> Vec<PickedPlace> {
        let columns = columns.max(1);
        let (a, b) = (
            [self.from % columns, self.from / columns],
            [self.to % columns, self.to / columns],
        );
        let (left, right) = (a[0].min(b[0]), a[0].max(b[0]));
        let (top, bottom) = (a[1].min(b[1]), a[1].max(b[1]));
        (top..=bottom)
            .flat_map(|row| {
                (left..=right).map(move |column| PickedPlace {
                    index: row * columns + column,
                    column: column - left,
                    row: row - top,
                })
            })
            .collect()
    }
}

/// A place of a picked rectangle.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PickedPlace {
    index: usize,
    column: usize,
    row: usize,
}

impl TilesPanel {
    /// Shows the tilesets of `doc`'s map; tiles picked become the brush through `out`.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        images: &mut TileImages,
        store: &Mutex<ImageStore>,
        out: &mut Vec<Intent>,
    ) {
        ui.heading("Tiles");
        ui.separator();
        let Some(map) = editor.map_of(doc) else {
            return;
        };
        if map.tilesets.is_empty() {
            ui.weak("The map uses no tilesets.");
            return;
        }
        let current = self.show_tabs(ui, map, doc);
        let Some(reference) = map.tilesets.get(current) else {
            return;
        };
        let key = tileset_key(doc, reference);
        let mut zoom = self.zoom.get(&key).copied().unwrap_or(1.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut zoom, ZOOM_RANGE)
                    .logarithmic(true)
                    .text("Zoom"),
            );
            if let Some(slot) = TilesetSlot::new(current)
                && ui
                    .button("Edit Tileset")
                    .on_hover_text(
                        "Its tiles' collision shapes and animations, in place of the map",
                    )
                    .clicked()
            {
                out.push(Intent::EditTileset { doc, slot });
            }
        });
        self.zoom.insert(key.clone(), zoom);
        ui.separator();
        let mut source = TileSource { images, store };
        let picked = self.show_tiles(ui, &reference.tileset, &key, zoom, &mut source);
        if let (Some(cells), Some(slot)) = (picked, TilesetSlot::new(current)) {
            let cells = cells
                .into_iter()
                .map(|placed| PlacedCell {
                    at: placed.at,
                    cell: Cell::new(slot, placed.cell.tile()),
                })
                .collect();
            out.push(Intent::PickTiles { doc, cells });
        }
    }

    fn show_tabs(&mut self, ui: &mut egui::Ui, map: &Map, doc: DocumentId) -> usize {
        let mut current = self
            .current
            .get(&doc)
            .copied()
            .unwrap_or(0)
            .min(map.tilesets.len().saturating_sub(1));
        ui.horizontal_wrapped(|ui| {
            for (index, reference) in map.tilesets.iter().enumerate() {
                let name = tileset_name(reference);
                let response = ui.selectable_label(index == current, name);
                let response = match &reference.file {
                    Some(file) => response.on_hover_text(file.path.display().to_string()),
                    None => response.on_hover_text("embedded in the map"),
                };
                if response.clicked() {
                    current = index;
                }
            }
        });
        self.current.insert(doc, current);
        current
    }

    /// Draws the tiles; the tiles picked this frame (their ids, as cells of slot 0, at their
    /// places), if any.
    fn show_tiles(
        &mut self,
        ui: &mut egui::Ui,
        tileset: &Tileset,
        key: &str,
        zoom: f32,
        source: &mut TileSource<'_>,
    ) -> Option<Vec<PlacedCell>> {
        let ids = tileset.tile_ids();
        if ids.is_empty() {
            ui.weak("The tileset has no tiles.");
            return None;
        }
        if let TilesetImage::Atlas { image, .. } = &tileset.image
            && let TileTexture::Failed(error) = source.images.texture(ui.ctx(), source.store, image)
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("The tileset's image could not be read: {error}"),
            );
        }
        let cell = cell_size(tileset, &ids) * zoom;
        let columns = match &tileset.image {
            TilesetImage::Atlas { columns, .. } => (*columns).max(1) as usize,
            TilesetImage::Collection => ((ui.available_width() / cell.x).floor() as usize).max(1),
        };
        let selection = ui.visuals().selection.stroke.color;
        let mut picked = None;
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
                let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
                grid.origin = area.min;
                let under = |at: Option<Pos2>| at.and_then(|at| grid.index_at(at));
                let pressed = under(response.interact_pointer_pos());
                if response.drag_started() {
                    self.dragging = pressed;
                }
                if response.dragged()
                    && let (Some(from), Some(to)) = (self.dragging, pressed)
                {
                    self.picked.insert(key.to_owned(), TilePick { from, to });
                }
                let finished = if response.clicked() {
                    pressed.map(|index| TilePick {
                        from: index,
                        to: index,
                    })
                } else if response.drag_stopped() {
                    self.dragging.take().and(self.picked.get(key).copied())
                } else {
                    None
                };
                if let Some(pick) = finished {
                    self.picked.insert(key.to_owned(), pick);
                    let cells: Vec<PlacedCell> = pick
                        .places(columns)
                        .into_iter()
                        .filter_map(|place| {
                            let id = ids.get(place.index).copied()?;
                            Some(PlacedCell {
                                at: glam::IVec2::new(place.column as i32, place.row as i32),
                                cell: Cell::new(TilesetSlot::new(0)?, id),
                            })
                        })
                        .collect();
                    if !cells.is_empty() {
                        picked = Some(cells);
                    }
                }
                let chosen: Vec<usize> = self
                    .picked
                    .get(key)
                    .map(|pick| pick.places(columns).iter().map(|p| p.index).collect())
                    .unwrap_or_default();
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
                        if chosen.contains(&index) {
                            ui.painter().rect_stroke(
                                rect.shrink(1.0),
                                0.0,
                                Stroke::new(2.0, selection),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
                let pointed = under(response.hover_pos()).and_then(|index| ids.get(index).copied());
                if let Some(id) = pointed {
                    response.on_hover_text(format!("Tile {}", id.0));
                }
            });
        picked
    }
}

/// What a tileset tab shows.
fn tileset_name(reference: &TilesetRef) -> String {
    if !reference.tileset.name.is_empty() {
        return reference.tileset.name.clone();
    }
    reference.file.as_ref().map_or_else(
        || "(unnamed)".to_owned(),
        |file| bt_utils::paths::display_name(&file.path),
    )
}

/// What a tileset's panel state is kept under: its file, or the document and its place for an
/// embedded one.
fn tileset_key(doc: DocumentId, reference: &TilesetRef) -> String {
    match &reference.file {
        Some(file) => file.path.display().to_string(),
        None => format!("{doc}/{}", reference.tileset.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_pick_is_a_rectangle_whatever_its_corners() {
        // Places 7 and 2 of a 3-column grid: columns 1 and 2, rows 0 to 2.
        let pick = TilePick { from: 7, to: 2 };
        let places = pick.places(3);
        let indexes: Vec<usize> = places.iter().map(|p| p.index).collect();
        assert_eq!(indexes, [1, 2, 4, 5, 7, 8]);
        assert_eq!((places[5].column, places[5].row), (1, 2));
    }
}
