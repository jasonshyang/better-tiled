//! Drawing a tileset's tiles in a grid, as the Tiles panel and the tileset editor show them:
//! where each tile goes, which tile is under the pointer, and a tile's image in its cell.

use std::sync::Mutex;

use bt_core::{TileId, Tileset, TilesetImage};
use bt_image::ImageStore;
use egui::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};

use super::tile_images::{TileImages, TileTexture, Uploaded};

/// Where a tileset's tiles go.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct TileGrid {
    /// Where the first tile goes.
    pub(crate) origin: Pos2,
    /// A tile's cell.
    pub(crate) cell: Vec2,
    /// The tiles in a row.
    pub(crate) columns: usize,
    /// The tiles.
    pub(crate) count: usize,
}

impl TileGrid {
    /// The cell of the tile at `index`.
    pub(crate) fn rect(&self, index: usize) -> Rect {
        let column = index % self.columns;
        let row = index / self.columns;
        Rect::from_min_size(
            self.origin + vec2(column as f32 * self.cell.x, row as f32 * self.cell.y),
            self.cell,
        )
    }

    /// The tile under `at`.
    pub(crate) fn index_at(&self, at: Pos2) -> Option<usize> {
        let offset = at - self.origin;
        if offset.x < 0.0 || offset.y < 0.0 {
            return None;
        }
        let column = (offset.x / self.cell.x) as usize;
        let row = (offset.y / self.cell.y) as usize;
        let index = row * self.columns + column;
        (column < self.columns && index < self.count).then_some(index)
    }

    /// The rows of tiles.
    pub(crate) fn rows(&self) -> usize {
        self.count.div_ceil(self.columns)
    }
}

/// Where tiles were drawn in the last frame.
#[derive(Clone, Debug)]
pub(crate) struct ShownTiles {
    /// The grid.
    pub(crate) grid: TileGrid,
    /// The tiles, in the grid's order.
    pub(crate) ids: Vec<TileId>,
}

impl ShownTiles {
    /// Where the middle of a tile is on screen.
    #[cfg(test)]
    pub(crate) fn centre_of(&self, tile: TileId) -> Option<egui::Pos2> {
        let index = self.ids.iter().position(|t| *t == tile)?;
        Some(self.grid.rect(index).center())
    }
}

/// What tiles are drawn with.
pub(crate) struct TileSource<'a> {
    /// The textures uploaded so far.
    pub(crate) images: &'a mut TileImages,
    /// The decoded images.
    pub(crate) store: &'a Mutex<ImageStore>,
}

/// The size of a tile's cell at zoom 1: the tile size of an atlas, the largest
/// image of a collection.
pub(crate) fn cell_size(tileset: &Tileset, ids: &[TileId]) -> Vec2 {
    let size = match tileset.image {
        TilesetImage::Atlas { .. } => tileset.tile_size(),
        TilesetImage::Collection => ids
            .iter()
            .map(|id| tileset.tile_pixel_size(*id))
            .fold(glam::UVec2::ZERO, glam::UVec2::max),
    };
    vec2(size.x.max(1) as f32, size.y.max(1) as f32)
}

/// Draws a tile's image in `cell`, scaled to fit; a faint cell while it loads, a cross when it
/// failed.
pub(crate) fn paint_tile(
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
