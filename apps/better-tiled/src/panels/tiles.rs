//! The Tiles panel: the map's tilesets as tabs, and the current one's tiles at the panel's zoom
//! (an atlas in its own columns, a collection wrapped to the panel). Only the rows in view are
//! drawn. Clicking a tile selects it; it becomes the brush once there are tools to paint with.

use std::collections::HashMap;
use std::sync::Mutex;

use bt_core::{Map, TileId, Tileset, TilesetImage, TilesetRef};
use bt_editor::{DocumentId, Editor};
use bt_image::ImageStore;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

use super::tile_images::{TileImages, TileTexture, Uploaded};

/// The zoom range of the panel.
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;

/// The panel's own state.
#[derive(Debug, Default)]
pub(crate) struct TilesPanel {
    current: HashMap<DocumentId, usize>,
    zoom: HashMap<String, f32>,
    selected: HashMap<String, TileId>,
}

/// Where a tileset's tiles go in the panel.
#[derive(Copy, Clone, Debug, PartialEq)]
struct TileGrid {
    origin: Pos2,
    cell: Vec2,
    columns: usize,
    count: usize,
}

impl TileGrid {
    fn rect(&self, index: usize) -> Rect {
        let column = index % self.columns;
        let row = index / self.columns;
        Rect::from_min_size(
            self.origin + vec2(column as f32 * self.cell.x, row as f32 * self.cell.y),
            self.cell,
        )
    }

    fn index_at(&self, at: Pos2) -> Option<usize> {
        let offset = at - self.origin;
        if offset.x < 0.0 || offset.y < 0.0 {
            return None;
        }
        let column = (offset.x / self.cell.x) as usize;
        let row = (offset.y / self.cell.y) as usize;
        let index = row * self.columns + column;
        (column < self.columns && index < self.count).then_some(index)
    }

    fn rows(&self) -> usize {
        self.count.div_ceil(self.columns)
    }
}

/// What the panel draws a tileset with.
struct TileSource<'a> {
    images: &'a mut TileImages,
    store: &'a Mutex<ImageStore>,
}

impl TilesPanel {
    /// Shows the tilesets of `doc`'s map.
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        editor: &Editor,
        doc: DocumentId,
        images: &mut TileImages,
        store: &Mutex<ImageStore>,
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
        ui.add(
            egui::Slider::new(&mut zoom, ZOOM_RANGE)
                .logarithmic(true)
                .text("Zoom"),
        );
        self.zoom.insert(key.clone(), zoom);
        ui.separator();
        let mut source = TileSource { images, store };
        self.show_tiles(ui, &reference.tileset, &key, zoom, &mut source);
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

    fn show_tiles(
        &mut self,
        ui: &mut egui::Ui,
        tileset: &Tileset,
        key: &str,
        zoom: f32,
        source: &mut TileSource<'_>,
    ) {
        let ids = tileset.tile_ids();
        if ids.is_empty() {
            ui.weak("The tileset has no tiles.");
            return;
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
        let selected = self.selected.get(key).copied();
        let selection = ui.visuals().selection.stroke.color;
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
                let (area, response) = ui.allocate_exact_size(size, Sense::click());
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
                        if selected == Some(id) {
                            ui.painter().rect_stroke(
                                rect.shrink(1.0),
                                0.0,
                                Stroke::new(2.0, selection),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
                let pointed = response
                    .hover_pos()
                    .and_then(|at| grid.index_at(at))
                    .and_then(|index| ids.get(index).copied());
                if let Some(id) = pointed {
                    if response.clicked() {
                        self.selected.insert(key.to_owned(), id);
                    }
                    response.on_hover_text(format!("Tile {}", id.0));
                }
            });
    }
}

/// The size of a tile's cell in the panel at zoom 1: the tile size of an atlas, the largest
/// image of a collection.
fn cell_size(tileset: &Tileset, ids: &[TileId]) -> Vec2 {
    let size = match tileset.image {
        TilesetImage::Atlas { .. } => tileset.tile_size(),
        TilesetImage::Collection => ids
            .iter()
            .map(|id| tileset.tile_pixel_size(*id))
            .fold(glam::UVec2::ZERO, glam::UVec2::max),
    };
    vec2(size.x.max(1) as f32, size.y.max(1) as f32)
}

fn paint_tile(
    ui: &egui::Ui,
    tileset: &Tileset,
    id: TileId,
    cell: Rect,
    source: &mut TileSource<'_>,
) {
    let Some(image) = tileset.tile_image(id) else {
        return;
    };
    match source.images.texture(ui.ctx(), source.store, image.image) {
        TileTexture::Ready(uploaded) => {
            let size = vec2(image.rect.w as f32, image.rect.h as f32);
            let scale = (cell.width() / size.x.max(1.0)).min(cell.height() / size.y.max(1.0));
            let rect = Rect::from_center_size(cell.center(), size * scale);
            ui.painter().image(
                uploaded.texture.id(),
                rect,
                uv_of(uploaded, image.rect),
                Color32::WHITE,
            );
        }
        TileTexture::Pending => {
            ui.painter()
                .rect_filled(cell.shrink(1.0), 0.0, ui.visuals().faint_bg_color);
            ui.ctx().request_repaint();
        }
        TileTexture::Failed(_) => {
            let stroke = Stroke::new(1.0, ui.visuals().error_fg_color);
            let rect = cell.shrink(2.0);
            ui.painter()
                .line_segment([rect.left_top(), rect.right_bottom()], stroke);
            ui.painter()
                .line_segment([rect.right_top(), rect.left_bottom()], stroke);
        }
    }
}

/// A tile's rectangle in its image, as texture coordinates.
fn uv_of(uploaded: &Uploaded, rect: bt_utils::IRect) -> Rect {
    let width = uploaded.image_size.x.max(1) as f32;
    let height = uploaded.image_size.y.max(1) as f32;
    Rect::from_min_max(
        pos2(rect.x as f32 / width, rect.y as f32 / height),
        pos2(
            (rect.x + rect.w) as f32 / width,
            (rect.y + rect.h) as f32 / height,
        ),
    )
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
    fn grids_find_tiles_under_the_pointer() {
        let grid = TileGrid {
            origin: pos2(10.0, 10.0),
            cell: vec2(16.0, 8.0),
            columns: 3,
            count: 7,
        };
        assert_eq!(grid.rows(), 3);
        assert_eq!(grid.index_at(pos2(10.0, 10.0)), Some(0));
        assert_eq!(grid.index_at(pos2(43.0, 19.0)), Some(5));
        assert_eq!(grid.index_at(pos2(30.0, 27.0)), None, "past the last tile");
        assert_eq!(
            grid.index_at(pos2(60.0, 10.0)),
            None,
            "past the last column"
        );
        assert_eq!(grid.index_at(pos2(5.0, 12.0)), None);
        assert_eq!(grid.rect(4).min, pos2(26.0, 18.0));
    }
}
