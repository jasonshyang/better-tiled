//! The grid geometry of every Tiled orientation: conversions between tile, pixel and screen
//! coordinates, bounding rectangles, draw order, hit testing, outlines, grid lines and snapping.
//!
//! *Screen* coordinates are where Tiled's renderers draw: pixels from the map's origin, the
//! top-left of [`MapGeometry::map_bounding_rect`] for finite maps. *Pixel* coordinates are where
//! object positions are stored; they are screen coordinates except on isometric maps (an
//! unprojected grid of tile-height squares) and oblique maps (unsheared).
//!
//! Every formula, its integer rounding included, follows libtiled's renderers
//! (`reference/tiled/src/libtiled/`: `orthogonalrenderer.cpp`, `isometricrenderer.cpp`,
//! `hexagonalrenderer.cpp`, `staggeredrenderer.cpp`, `obliquerenderer.cpp`, `maprenderer.cpp`,
//! `hex.cpp`; BSD-2-Clause, see `THIRD_PARTY_NOTICES.md`), so a cell is where Tiled draws it.

mod hex;
#[cfg(test)]
mod tests;

pub use hex::Hex;

use std::collections::{BTreeMap, HashMap, HashSet};

use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2, Vec2};

use crate::map::{Map, Orientation, RenderOrder, StaggerAxis, StaggerIndex};
use crate::tileset::{Alignment, Tileset};

/// The geometry of a map's grid; cheap to copy. Built with [`Map::geometry`] or
/// [`MapGeometry::new`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapGeometry {
    /// The grid's shape.
    pub orientation: Orientation,
    /// The draw order (honoured on orthogonal and oblique maps only, as in Tiled).
    pub render_order: RenderOrder,
    /// A cell's width in pixels.
    pub tile_width: i32,
    /// A cell's height in pixels.
    pub tile_height: i32,
    /// The map's width in tiles.
    pub width: i32,
    /// The map's height in tiles; isometric maps place their origin by it, infinite ones too.
    pub height: i32,
    /// Whether the map is infinite.
    pub infinite: bool,
    /// A hexagon's straight side, on hexagonal maps.
    pub hex_side_length: i32,
    /// The staggered axis.
    pub stagger_axis: StaggerAxis,
    /// The shifted rows or columns.
    pub stagger_index: StaggerIndex,
    /// An oblique map's shift per row (`x`) and per column (`y`).
    pub skew: IVec2,
}

/// The measures of hexagonal and staggered grids (Tiled's `HexagonalRenderer::RenderParams`).
/// Halves are rounded down, so the effective tile can be a pixel smaller than the map's.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HexParams {
    /// The straight side along x (hexagonal maps staggered along x).
    pub side_length_x: i32,
    /// The straight side along y (hexagonal maps staggered along y).
    pub side_length_y: i32,
    /// Half of what is left of the width beside the side.
    pub side_offset_x: i32,
    /// Half of what is left of the height beside the side.
    pub side_offset_y: i32,
    /// The distance between columns.
    pub column_width: i32,
    /// The distance between rows.
    pub row_height: i32,
    /// The effective tile width.
    pub tile_width: i32,
    /// The effective tile height.
    pub tile_height: i32,
    /// Whether columns are staggered.
    pub stagger_x: bool,
    /// Whether the even rows or columns are the shifted ones.
    pub stagger_even: bool,
}

impl HexParams {
    /// Whether column `x` is shifted down (stagger axis x).
    pub fn do_stagger_x(&self, x: i32) -> bool {
        self.stagger_x && ((x & 1) != 0) != self.stagger_even
    }

    /// Whether row `y` is shifted right (stagger axis y).
    pub fn do_stagger_y(&self, y: i32) -> bool {
        !self.stagger_x && ((y & 1) != 0) != self.stagger_even
    }
}

/// A segment of the grid, in screen coordinates.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridLine {
    /// One end.
    pub from: DVec2,
    /// The other end.
    pub to: DVec2,
    /// Drawn with the major grid's pen.
    pub major: bool,
}

/// The sort key of Tiled's tile draw order: cells are drawn in increasing key order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawOrderKey {
    /// The row, diagonal or half-row.
    pub major: i64,
    /// The place within it.
    pub minor: i64,
}

/// A cell's centre and the step from the reference cell to it, for hexagonal hit testing.
#[derive(Copy, Clone)]
struct Candidate {
    center: IVec2,
    offset: IVec2,
}

/// The first and last index of a run of cells that reach into an area.
#[derive(Copy, Clone)]
struct Span {
    first: i64,
    last: i64,
}

/// A vertex of an outline, in the integer keys outlines are built from.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Vertex {
    x: i32,
    y: i32,
}

impl Vertex {
    fn of(v: IVec2) -> Vertex {
        Vertex { x: v.x, y: v.y }
    }

    fn ivec2(self) -> IVec2 {
        IVec2::new(self.x, self.y)
    }
}

/// A directed edge of a cell outline.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct Edge {
    from: Vertex,
    to: Vertex,
}

/// `v` rounded down to cells. Coordinates beyond `i32` saturate: no map Tiled can hold
/// reaches them.
fn floor_i(v: DVec2) -> IVec2 {
    IVec2::new(v.x.floor() as i32, v.y.floor() as i32)
}

/// A value of the 64-bit arithmetic below as an `i32`, saturating beyond what any map reaches.
fn fit_i32(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX })
}

/// An integer rectangle from 64-bit parts.
fn irect(x: i64, y: i64, w: i64, h: i64) -> IRect {
    IRect {
        x: fit_i32(x),
        y: fit_i32(y),
        w: fit_i32(w),
        h: fit_i32(h),
    }
}

/// Qt's `QPolygonF::boundingRect().toAlignedRect()`.
fn aligned_bounds(points: impl IntoIterator<Item = DVec2>) -> Option<IRect> {
    DRect::from_points(points)?.to_aligned()
}

impl MapGeometry {
    /// A finite map's geometry, with Tiled's defaults for the orientation-specific settings.
    pub fn new(orientation: Orientation, tile_size: IVec2, map_size: IVec2) -> MapGeometry {
        MapGeometry {
            orientation,
            render_order: RenderOrder::RightDown,
            tile_width: tile_size.x,
            tile_height: tile_size.y,
            width: map_size.x,
            height: map_size.y,
            infinite: false,
            hex_side_length: 0,
            stagger_axis: StaggerAxis::Y,
            stagger_index: StaggerIndex::Odd,
            skew: IVec2::ZERO,
        }
    }

    /// The geometry of `map`.
    pub fn from_map(map: &Map) -> MapGeometry {
        let to_i32 = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        MapGeometry {
            orientation: map.orientation,
            render_order: map.render_order,
            tile_width: to_i32(map.tile_width),
            tile_height: to_i32(map.tile_height),
            width: to_i32(map.width),
            height: to_i32(map.height),
            infinite: map.infinite,
            hex_side_length: map.hex_side_length,
            stagger_axis: map.stagger_axis,
            stagger_index: map.stagger_index,
            skew: map.skew,
        }
    }

    fn tile_size_f(&self) -> DVec2 {
        DVec2::new(f64::from(self.tile_width), f64::from(self.tile_height))
    }

    fn is_hex_grid(&self) -> bool {
        matches!(
            self.orientation,
            Orientation::Hexagonal | Orientation::Staggered
        )
    }

    /// Isometric maps: the screen x of the top corner of tile (0, 0).
    fn iso_origin_x(&self) -> f64 {
        (i64::from(self.height) * i64::from(self.tile_width) / 2) as f64
    }

    /// Oblique shear factors (Tiled's `ObliqueRenderer::transform`).
    fn shear(&self) -> DVec2 {
        if self.tile_width == 0 || self.tile_height == 0 {
            return DVec2::ZERO;
        }
        DVec2::new(
            f64::from(self.skew.x) / f64::from(self.tile_height),
            f64::from(self.skew.y) / f64::from(self.tile_width),
        )
    }

    /// The measures of a hexagonal or staggered grid.
    pub fn hex_params(&self) -> HexParams {
        let stagger_x = self.stagger_axis == StaggerAxis::X;
        let hexagonal = self.orientation == Orientation::Hexagonal;
        let side_length_x = if hexagonal && stagger_x {
            self.hex_side_length
        } else {
            0
        };
        let side_length_y = if hexagonal && !stagger_x {
            self.hex_side_length
        } else {
            0
        };
        let side_offset_x = (self.tile_width - side_length_x) / 2;
        let side_offset_y = (self.tile_height - side_length_y) / 2;
        let column_width = side_offset_x + side_length_x;
        let row_height = side_offset_y + side_length_y;
        HexParams {
            side_length_x,
            side_length_y,
            side_offset_x,
            side_offset_y,
            column_width,
            row_height,
            tile_width: column_width + side_offset_x,
            tile_height: row_height + side_offset_y,
            stagger_x,
            stagger_even: self.stagger_index == StaggerIndex::Even,
        }
    }

    /// The top-left of a hexagonal or staggered cell's bounding box (integer, as in Tiled).
    fn hex_origin(&self, p: &HexParams, t: IVec2) -> IVec2 {
        if p.stagger_x {
            let shift = if p.do_stagger_x(t.x) { p.row_height } else { 0 };
            IVec2::new(
                t.x.wrapping_mul(p.column_width),
                t.y.wrapping_mul(p.tile_height + p.side_length_y)
                    .wrapping_add(shift),
            )
        } else {
            let shift = if p.do_stagger_y(t.y) {
                p.column_width
            } else {
                0
            };
            IVec2::new(
                t.x.wrapping_mul(p.tile_width + p.side_length_x)
                    .wrapping_add(shift),
                t.y.wrapping_mul(p.row_height),
            )
        }
    }

    // Conversions.

    /// Tiled's `tileToScreenCoords`. Orthogonal: the cell's top-left corner; isometric: the top
    /// corner of its diamond; hexagonal and staggered: the top-left of its bounding box (rounded
    /// down to a cell); oblique: its sheared top-left corner.
    pub fn tile_to_screen(&self, tile: DVec2) -> DVec2 {
        let t = self.tile_size_f();
        match self.orientation {
            Orientation::Orthogonal => tile * t,
            Orientation::Isometric => DVec2::new(
                (tile.x - tile.y) * t.x / 2.0 + self.iso_origin_x(),
                (tile.x + tile.y) * t.y / 2.0,
            ),
            Orientation::Hexagonal | Orientation::Staggered => self
                .hex_origin(&self.hex_params(), floor_i(tile))
                .as_dvec2(),
            Orientation::Oblique => self.pixel_to_screen(tile * t),
        }
    }

    /// Tiled's `screenToTileCoords`: fractional, except on hexagonal maps (the cell with the
    /// nearest centre) and staggered maps (the cell, plus the position inside its diamond along
    /// the diamond's own axes).
    pub fn screen_to_tile(&self, screen: DVec2) -> DVec2 {
        let t = self.tile_size_f();
        match self.orientation {
            Orientation::Orthogonal => screen / t,
            Orientation::Isometric => {
                let x = screen.x - self.iso_origin_x();
                let tile_y = screen.y / t.y;
                let tile_x = x / t.x;
                DVec2::new(tile_y + tile_x, tile_y - tile_x)
            }
            Orientation::Hexagonal => self.hex_screen_to_tile(screen).as_dvec2(),
            Orientation::Staggered => self.staggered_screen_to_tile(screen),
            Orientation::Oblique => self.screen_to_pixel(screen) / t,
        }
    }

    /// The cell under a screen position (Tiled's hovered tile).
    pub fn tile_at(&self, screen: DVec2) -> IVec2 {
        match self.orientation {
            Orientation::Hexagonal => self.hex_screen_to_tile(screen),
            _ => floor_i(self.screen_to_tile(screen)),
        }
    }

    /// Where an object position (pixels) is drawn: the same point except on isometric maps
    /// (projected) and oblique maps (sheared).
    pub fn pixel_to_screen(&self, pixel: DVec2) -> DVec2 {
        match self.orientation {
            Orientation::Isometric => {
                let (tw, th) = (f64::from(self.tile_width), f64::from(self.tile_height));
                let tile_y = pixel.y / th;
                let tile_x = pixel.x / th;
                DVec2::new(
                    (tile_x - tile_y) * tw / 2.0 + self.iso_origin_x(),
                    (tile_x + tile_y) * th / 2.0,
                )
            }
            Orientation::Oblique => {
                let s = self.shear();
                DVec2::new(pixel.x + s.x * pixel.y, s.y * pixel.x + pixel.y)
            }
            _ => pixel,
        }
    }

    /// The object position drawn at a screen position (the inverse of
    /// [`MapGeometry::pixel_to_screen`]). A singular shear maps a point to itself, as Qt does.
    pub fn screen_to_pixel(&self, screen: DVec2) -> DVec2 {
        match self.orientation {
            Orientation::Isometric => {
                let (tw, th) = (f64::from(self.tile_width), f64::from(self.tile_height));
                let x = screen.x - self.iso_origin_x();
                let tile_y = screen.y / th;
                let tile_x = x / tw;
                DVec2::new((tile_y + tile_x) * th, (tile_y - tile_x) * th)
            }
            Orientation::Oblique => {
                let s = self.shear();
                let det = 1.0 - s.x * s.y;
                if det.abs() <= 1e-12 {
                    return screen;
                }
                let inv = 1.0 / det;
                DVec2::new(
                    inv * screen.x - s.x * inv * screen.y,
                    -s.y * inv * screen.x + inv * screen.y,
                )
            }
            _ => screen,
        }
    }

    /// The tile position of an object position. Hexagonal and staggered maps give the cell with
    /// the nearest centre.
    pub fn pixel_to_tile(&self, pixel: DVec2) -> DVec2 {
        let t = self.tile_size_f();
        match self.orientation {
            Orientation::Isometric => pixel / t.y,
            Orientation::Hexagonal | Orientation::Staggered => {
                self.hex_screen_to_tile(pixel).as_dvec2()
            }
            _ => pixel / t,
        }
    }

    /// The object position of a tile position.
    pub fn tile_to_pixel(&self, tile: DVec2) -> DVec2 {
        let t = self.tile_size_f();
        match self.orientation {
            Orientation::Isometric => tile * t.y,
            Orientation::Hexagonal | Orientation::Staggered => self.tile_to_screen(tile),
            _ => tile * t,
        }
    }

    /// The bottom-left corner of a cell's image before the tileset's tile offset. Tiled draws
    /// tile images from there, so a larger image extends up and to the right.
    pub fn cell_anchor(&self, tile: IVec2) -> DVec2 {
        let t = self.tile_size_f();
        match self.orientation {
            Orientation::Orthogonal => {
                DVec2::new(f64::from(tile.x) * t.x, (f64::from(tile.y) + 1.0) * t.y)
            }
            Orientation::Isometric => {
                self.tile_to_screen(tile.as_dvec2())
                    + DVec2::new(-f64::from(self.tile_width / 2), t.y)
            }
            Orientation::Hexagonal | Orientation::Staggered => {
                let p = self.hex_params();
                (self.hex_origin(&p, tile) + IVec2::new(0, p.tile_height)).as_dvec2()
            }
            Orientation::Oblique => {
                self.tile_to_screen(DVec2::new(f64::from(tile.x), f64::from(tile.y) + 1.0))
            }
        }
    }

    /// The screen rectangle an image of `size` covers at a cell, before the tile offset.
    pub fn cell_image_rect(&self, tile: IVec2, size: DVec2) -> DRect {
        let anchor = self.cell_anchor(tile);
        DRect::from_min_size(DVec2::new(anchor.x, anchor.y - size.y), size)
    }

    // Bounds.

    /// The screen rectangle of a rectangle of cells (Tiled's `boundingRect(QRect)`).
    pub fn bounding_rect(&self, tiles: IRect) -> IRect {
        let (tw, th) = (i64::from(self.tile_width), i64::from(self.tile_height));
        let (x, y, w, h) = (
            i64::from(tiles.x),
            i64::from(tiles.y),
            i64::from(tiles.w),
            i64::from(tiles.h),
        );
        match self.orientation {
            Orientation::Orthogonal => irect(x * tw, y * th, w * tw, h * th),
            Orientation::Isometric => {
                let origin_x = i64::from(self.height) * tw / 2;
                let side = h + w;
                irect(
                    (x - (y + h)) * tw / 2 + origin_x,
                    (x + y) * th / 2,
                    side * tw / 2,
                    side * th / 2,
                )
            }
            Orientation::Hexagonal | Orientation::Staggered => {
                let p = self.hex_params();
                let top_left = self.hex_origin(&p, tiles.min());
                let (mut left, mut top) = (i64::from(top_left.x), i64::from(top_left.y));
                let (width, height);
                if p.stagger_x {
                    width = w * i64::from(p.column_width) + i64::from(p.side_offset_x);
                    let mut full = h * i64::from(p.tile_height + p.side_length_y);
                    if w > 1 {
                        full += i64::from(p.row_height);
                        if p.do_stagger_x(tiles.x) {
                            top -= i64::from(p.row_height);
                        }
                    }
                    height = full;
                } else {
                    let mut full = w * i64::from(p.tile_width + p.side_length_x);
                    height = h * i64::from(p.row_height) + i64::from(p.side_offset_y);
                    if h > 1 {
                        full += i64::from(p.column_width);
                        if p.do_stagger_y(tiles.y) {
                            left -= i64::from(p.column_width);
                        }
                    }
                    width = full;
                }
                irect(left, top, width, height)
            }
            Orientation::Oblique => {
                aligned_bounds(self.tile_rect_corners(tiles)).unwrap_or_default()
            }
        }
    }

    /// The screen rectangle of a finite map (Tiled's `mapBoundingRect`): what a picture of it
    /// covers. For infinite maps see [`Map::bounding_rect`].
    pub fn map_bounding_rect(&self) -> IRect {
        self.bounding_rect(IRect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        })
    }

    /// A rectangle of cells holding every cell whose outline meets `screen` (conservative, for
    /// culling).
    pub fn tiles_in_rect(&self, screen: DRect) -> IRect {
        let corners = [
            screen.min,
            DVec2::new(screen.max.x, screen.min.y),
            screen.max,
            DVec2::new(screen.min.x, screen.max.y),
        ];
        let (mut min, mut max) = (IVec2::MAX, IVec2::MIN);
        for corner in corners {
            let t = self.tile_at(corner);
            min = min.min(t);
            max = max.max(t);
        }
        IRect::from_corners(
            min.saturating_sub(IVec2::ONE),
            max.saturating_add(IVec2::ONE),
        )
    }

    /// The corners of a rectangle of cells on screen, clockwise from the top-left.
    fn tile_rect_corners(&self, r: IRect) -> [DVec2; 4] {
        let (min, max) = (r.min().as_dvec2(), r.max().as_dvec2());
        [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
            .map(|c| self.tile_to_screen(c))
    }

    // Draw order.

    /// Visits the cells Tiled draws for the screen rectangle `exposed`, in Tiled's draw order,
    /// with their [`MapGeometry::cell_anchor`]s. Cells near `exposed` are visited too; grow it by
    /// a layer's draw margins to include images that overhang into it from further away.
    pub fn for_each_tile(&self, exposed: DRect, mut visit: impl FnMut(IVec2, DVec2)) {
        match self.orientation {
            Orientation::Orthogonal => self.orthogonal_tiles(exposed, &mut visit),
            Orientation::Isometric => self.isometric_tiles(exposed, &mut visit),
            Orientation::Hexagonal | Orientation::Staggered => {
                self.hexagonal_tiles(exposed, &mut visit)
            }
            Orientation::Oblique => self.oblique_tiles(exposed, &mut visit),
        }
    }

    /// The key that orders cells as [`MapGeometry::for_each_tile`] visits them: by row in the
    /// render order on orthogonal and oblique maps, by diagonal on isometric maps, by row (by
    /// half-row for stagger axis x) on hexagonal and staggered maps.
    pub fn draw_order_key(&self, tile: IVec2) -> DrawOrderKey {
        let (x, y) = (i64::from(tile.x), i64::from(tile.y));
        let key = |major, minor| DrawOrderKey { major, minor };
        match self.orientation {
            Orientation::Orthogonal | Orientation::Oblique => match self.render_order {
                RenderOrder::RightDown => key(y, x),
                RenderOrder::RightUp => key(-y, x),
                RenderOrder::LeftDown => key(y, -x),
                RenderOrder::LeftUp => key(-y, -x),
            },
            Orientation::Isometric => key(x + y, x),
            Orientation::Hexagonal | Orientation::Staggered => {
                let p = self.hex_params();
                if p.stagger_x {
                    key(2 * y + i64::from(p.do_stagger_x(tile.x)), x)
                } else {
                    key(y, x)
                }
            }
        }
    }

    /// Visits rows and columns `min..=max` in the render order.
    fn rows_in_render_order(&self, min: IVec2, max: IVec2, visit: &mut dyn FnMut(IVec2, DVec2)) {
        let (reverse_x, reverse_y) = match self.render_order {
            RenderOrder::RightDown => (false, false),
            RenderOrder::RightUp => (false, true),
            RenderOrder::LeftDown => (true, false),
            RenderOrder::LeftUp => (true, true),
        };
        let rows = i64::from(max.y) - i64::from(min.y);
        let columns = i64::from(max.x) - i64::from(min.x);
        for i in 0..=rows {
            let y = fit_i32(if reverse_y {
                i64::from(max.y) - i
            } else {
                i64::from(min.y) + i
            });
            for j in 0..=columns {
                let x = fit_i32(if reverse_x {
                    i64::from(max.x) - j
                } else {
                    i64::from(min.x) + j
                });
                let tile = IVec2::new(x, y);
                visit(tile, self.cell_anchor(tile));
            }
        }
    }

    fn orthogonal_tiles(&self, e: DRect, visit: &mut dyn FnMut(IVec2, DVec2)) {
        let (tw, th) = (self.tile_width, self.tile_height);
        if tw <= 0 || th <= 0 {
            return;
        }
        let start = IVec2::new(
            (e.min.x / f64::from(tw)).floor() as i32,
            (e.min.y / f64::from(th)).floor() as i32,
        );
        let end = IVec2::new(e.max.x.ceil() as i32 / tw, e.max.y.ceil() as i32 / th);
        if start.x > end.x || start.y > end.y {
            return;
        }
        self.rows_in_render_order(start, end, visit);
    }

    fn oblique_tiles(&self, e: DRect, visit: &mut dyn FnMut(IVec2, DVec2)) {
        let (tw, th) = (f64::from(self.tile_width), f64::from(self.tile_height));
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        let Some(r) = self.screen_to_pixel_rect(e) else {
            return;
        };
        let mut start = IVec2::new((r.min.x / tw).floor() as i32, (r.min.y / th).floor() as i32);
        let mut end = IVec2::new((r.max.x / tw).ceil() as i32, (r.max.y / th).ceil() as i32);
        if !self.infinite {
            start = start.max(IVec2::ZERO);
            end = end.min(IVec2::new(self.width, self.height));
        }
        if start.x > end.x || start.y > end.y {
            return;
        }
        self.rows_in_render_order(start, end, visit);
    }

    /// The bounding rectangle of a screen rectangle taken to pixel space (oblique maps).
    fn screen_to_pixel_rect(&self, r: DRect) -> Option<DRect> {
        let corners = [
            r.min,
            DVec2::new(r.max.x, r.min.y),
            r.max,
            DVec2::new(r.min.x, r.max.y),
        ];
        DRect::from_points(corners.map(|c| self.screen_to_pixel(c)))
    }

    fn isometric_tiles(&self, e: DRect, visit: &mut dyn FnMut(IVec2, DVec2)) {
        let (tw, th) = (self.tile_width, self.tile_height);
        if tw < 1 || th < 1 {
            return;
        }
        let (half_w, half_h) = (f64::from(tw / 2), f64::from(th / 2));
        let mut row = floor_i(self.screen_to_tile(e.min));
        let mut start = self.tile_to_screen(row.as_dvec2()) + DVec2::new(-half_w, f64::from(th));
        // Start a row higher when the exposed corner is in the upper half of the tile.
        let in_upper_half = start.y - e.min.y > half_h;
        let in_left_half = e.min.x - start.x < half_w;
        if in_upper_half {
            if in_left_half {
                row.x -= 1;
                start.x -= half_w;
            } else {
                row.y -= 1;
                start.x += half_w;
            }
            start.y -= half_h;
        }
        let mut shifted = in_upper_half ^ in_left_half;
        // Doubled y coordinates keep odd tile heights exact.
        let mut y = (start.y * 2.0) as i64;
        while ((y - 2 * i64::from(th)) as f64) < e.max.y * 2.0 {
            let mut column = row;
            let mut x = start.x as i64;
            while (x as f64) < e.max.x {
                visit(column, self.cell_anchor(column));
                column += IVec2::new(1, -1);
                x += i64::from(tw);
            }
            if shifted {
                row.y += 1;
                start.x -= half_w;
            } else {
                row.x += 1;
                start.x += half_w;
            }
            shifted = !shifted;
            y += i64::from(th);
        }
    }

    /// The cell Tiled's hexagonal grid drawing starts at (`screenToTileCoords(…).toPoint()`,
    /// which rounds a staggered map's fractional coordinates).
    fn hex_start_tile(&self, screen: DVec2) -> IVec2 {
        match self.orientation {
            Orientation::Staggered => {
                let t = self.staggered_screen_to_tile(screen);
                IVec2::new(t.x.round() as i32, t.y.round() as i32)
            }
            _ => self.hex_screen_to_tile(screen),
        }
    }

    /// Hexagonal and staggered maps, in Tiled's order: rows for stagger axis y, half-rows (every
    /// other column) for x. Tiled's own loop starts from the cell at the corner and can skip a
    /// half-row; this visits exactly the cells whose image box (at the anchor, at least
    /// tile-sized) meets the area.
    fn hexagonal_tiles(&self, e: DRect, visit: &mut dyn FnMut(IVec2, DVec2)) {
        let p = self.hex_params();
        let step = p.tile_width + p.side_length_x;
        if p.row_height <= 0 || p.column_width <= 0 || step <= 0 {
            return;
        }
        let width = f64::from(p.tile_width.max(self.tile_width));
        let height = f64::from(p.tile_height.max(self.tile_height));
        let bottom = f64::from(p.tile_height);
        let row_height = f64::from(p.row_height);
        // The rows (or half-rows), `row_height` apart, whose boxes reach into the area.
        let first_line = ((e.min.y - bottom) / row_height).floor() as i64 + 1;
        let last_line = ((e.max.y - bottom + height) / row_height).ceil() as i64 - 1;
        // The first and last index whose box, `stride` apart and shifted by `shift`, reaches in.
        let span = |shift: f64, stride: f64| Span {
            first: ((e.min.x - shift - width) / stride).floor() as i64 + 1,
            last: ((e.max.x - shift) / stride).ceil() as i64 - 1,
        };
        for line in first_line..=last_line {
            if p.stagger_x {
                // Half-row `line`: row `line / 2`, the shifted columns when odd.
                let (y, shifted) = (fit_i32(line.div_euclid(2)), line.rem_euclid(2) == 1);
                let Span { first, last } = span(0.0, f64::from(p.column_width));
                let mut x = first;
                if p.do_stagger_x(fit_i32(x)) != shifted {
                    x += 1;
                }
                while x <= last {
                    let tile = IVec2::new(fit_i32(x), y);
                    visit(tile, self.cell_anchor(tile));
                    x += 2;
                }
            } else {
                let y = fit_i32(line);
                let shift = if p.do_stagger_y(y) {
                    f64::from(p.column_width)
                } else {
                    0.0
                };
                let Span { first, last } = span(shift, f64::from(step));
                for x in first..=last {
                    let tile = IVec2::new(fit_i32(x), y);
                    visit(tile, self.cell_anchor(tile));
                }
            }
        }
    }

    // Hexagonal and staggered helpers.

    /// Tiled's `HexagonalRenderer::screenToTileCoords`: the cell whose centre is nearest, among
    /// the four candidates around the position (distances in `f32`, as in Tiled).
    fn hex_screen_to_tile(&self, screen: DVec2) -> IVec2 {
        let p = self.hex_params();
        let (mut x, mut y) = (screen.x, screen.y);
        if p.stagger_x {
            x -= f64::from(if p.stagger_even {
                p.tile_width
            } else {
                p.side_offset_x
            });
        } else {
            y -= f64::from(if p.stagger_even {
                p.tile_height
            } else {
                p.side_offset_y
            });
        }
        let (cell_w, cell_h) = (f64::from(p.column_width * 2), f64::from(p.row_height * 2));
        let mut reference = IVec2::new((x / cell_w).floor() as i32, (y / cell_h).floor() as i32);
        let rel = Vec2::new(
            (x - f64::from(reference.x) * cell_w) as f32,
            (y - f64::from(reference.y) * cell_h) as f32,
        );
        let even = i32::from(p.stagger_even);
        let candidate = |cx: i32, cy: i32, dx: i32, dy: i32| Candidate {
            center: IVec2::new(cx, cy),
            offset: IVec2::new(dx, dy),
        };
        let candidates = if p.stagger_x {
            reference.x = reference.x.wrapping_mul(2).wrapping_add(even);
            let left = p.side_length_x / 2;
            let center_x = left + p.column_width;
            let center_y = p.tile_height / 2;
            [
                candidate(left, center_y, 0, 0),
                candidate(center_x, center_y - p.row_height, 1, -1),
                candidate(center_x, center_y + p.row_height, 1, 0),
                candidate(center_x + p.column_width, center_y, 2, 0),
            ]
        } else {
            reference.y = reference.y.wrapping_mul(2).wrapping_add(even);
            let top = p.side_length_y / 2;
            let center_x = p.tile_width / 2;
            let center_y = top + p.row_height;
            [
                candidate(center_x, top, 0, 0),
                candidate(center_x - p.column_width, center_y, -1, 1),
                candidate(center_x + p.column_width, center_y, 0, 1),
                candidate(center_x, center_y + p.row_height, 0, 2),
            ]
        };
        let mut nearest = IVec2::ZERO;
        let mut min_dist = f32::MAX;
        for c in candidates {
            let d = c.center.as_vec2() - rel;
            let dist = d.x * d.x + d.y * d.y;
            if dist < min_dist {
                min_dist = dist;
                nearest = c.offset;
            }
        }
        reference.wrapping_add(nearest)
    }

    /// Tiled's `StaggeredRenderer::screenToTileCoords`: the diamond holding the position, plus
    /// the position inside it measured along the diamond's edges.
    fn staggered_screen_to_tile(&self, screen: DVec2) -> DVec2 {
        let p = self.hex_params();
        let (mut ax, mut ay) = (screen.x, screen.y);
        if p.stagger_x {
            ax -= if p.stagger_even {
                f64::from(p.side_offset_x)
            } else {
                0.0
            };
        } else {
            ay -= if p.stagger_even {
                f64::from(p.side_offset_y)
            } else {
                0.0
            };
        }
        let (tw, th) = (f64::from(p.tile_width), f64::from(p.tile_height));
        let mut reference = IVec2::new((ax / tw).floor() as i32, (ay / th).floor() as i32);
        let rel = DVec2::new(
            ax - f64::from(reference.x) * tw,
            ay - f64::from(reference.y) * th,
        );
        let even = i32::from(p.stagger_even);
        if p.stagger_x {
            reference.x = reference.x.wrapping_mul(2).wrapping_add(even);
        } else {
            reference.y = reference.y.wrapping_mul(2).wrapping_add(even);
        }
        // The corners of the grid-aligned rectangle belong to the neighbouring diamonds.
        let side_offset_y = f64::from(p.side_offset_y);
        let y_pos = rel.x * (th / tw);
        if side_offset_y - y_pos > rel.y {
            reference = self.top_left(reference);
        }
        if -side_offset_y + y_pos > rel.y {
            reference = self.top_right(reference);
        }
        if side_offset_y + y_pos < rel.y {
            reference = self.bottom_left(reference);
        }
        if side_offset_y * 3.0 - y_pos < rel.y {
            reference = self.bottom_right(reference);
        }
        // The position from the diamond's top corner, turned into the diamond's axes.
        let origin = self.hex_origin(&p, reference).as_dvec2();
        let mut local = screen - origin - DVec2::new(f64::from(p.tile_width / 2), 0.0);
        local.y *= tw / th;
        let (sin, cos) = (-45f64).to_radians().sin_cos();
        let rotated = DVec2::new(cos * local.x - sin * local.y, sin * local.x + cos * local.y);
        rotated / (tw / 2f64.sqrt()) + reference.as_dvec2()
    }

    fn stagger_index_bit(&self) -> i32 {
        i32::from(self.stagger_index == StaggerIndex::Even)
    }

    /// The upper-left diagonal neighbour on hexagonal and staggered maps (Tiled's
    /// `HexagonalRenderer::topLeft`).
    pub fn top_left(&self, t: IVec2) -> IVec2 {
        let i = self.stagger_index_bit();
        match self.stagger_axis {
            StaggerAxis::Y if (t.y & 1) ^ i != 0 => IVec2::new(t.x, t.y - 1),
            StaggerAxis::Y => IVec2::new(t.x - 1, t.y - 1),
            StaggerAxis::X if (t.x & 1) ^ i != 0 => IVec2::new(t.x - 1, t.y),
            StaggerAxis::X => IVec2::new(t.x - 1, t.y - 1),
        }
    }

    /// The upper-right diagonal neighbour on hexagonal and staggered maps.
    pub fn top_right(&self, t: IVec2) -> IVec2 {
        let i = self.stagger_index_bit();
        match self.stagger_axis {
            StaggerAxis::Y if (t.y & 1) ^ i != 0 => IVec2::new(t.x + 1, t.y - 1),
            StaggerAxis::Y => IVec2::new(t.x, t.y - 1),
            StaggerAxis::X if (t.x & 1) ^ i != 0 => IVec2::new(t.x + 1, t.y),
            StaggerAxis::X => IVec2::new(t.x + 1, t.y - 1),
        }
    }

    /// The lower-left diagonal neighbour on hexagonal and staggered maps.
    pub fn bottom_left(&self, t: IVec2) -> IVec2 {
        let i = self.stagger_index_bit();
        match self.stagger_axis {
            StaggerAxis::Y if (t.y & 1) ^ i != 0 => IVec2::new(t.x, t.y + 1),
            StaggerAxis::Y => IVec2::new(t.x - 1, t.y + 1),
            StaggerAxis::X if (t.x & 1) ^ i != 0 => IVec2::new(t.x - 1, t.y + 1),
            StaggerAxis::X => IVec2::new(t.x - 1, t.y),
        }
    }

    /// The lower-right diagonal neighbour on hexagonal and staggered maps.
    pub fn bottom_right(&self, t: IVec2) -> IVec2 {
        let i = self.stagger_index_bit();
        match self.stagger_axis {
            StaggerAxis::Y if (t.y & 1) ^ i != 0 => IVec2::new(t.x + 1, t.y + 1),
            StaggerAxis::Y => IVec2::new(t.x, t.y + 1),
            StaggerAxis::X if (t.x & 1) ^ i != 0 => IVec2::new(t.x + 1, t.y + 1),
            StaggerAxis::X => IVec2::new(t.x + 1, t.y),
        }
    }

    /// The cells next to `tile`, connected as Tiled's fill tools connect them: four on square
    /// grids, six on hexagonal and staggered grids, clockwise.
    pub fn neighbors(&self, tile: IVec2) -> impl Iterator<Item = IVec2> + use<> {
        let (list, count) = if self.is_hex_grid() {
            let (tl, tr, bl, br) = (
                self.top_left(tile),
                self.top_right(tile),
                self.bottom_left(tile),
                self.bottom_right(tile),
            );
            match self.stagger_axis {
                StaggerAxis::Y => ([tr, tile + IVec2::X, br, bl, tile - IVec2::X, tl], 6),
                StaggerAxis::X => ([tile - IVec2::Y, tr, br, tile + IVec2::Y, bl, tl], 6),
            }
        } else {
            (
                [
                    tile - IVec2::Y,
                    tile + IVec2::X,
                    tile + IVec2::Y,
                    tile - IVec2::X,
                    tile,
                    tile,
                ],
                4,
            )
        };
        list.into_iter().take(count)
    }

    /// The eight points of a hexagonal or staggered cell's outline (coincident points included).
    fn hex_polygon(&self, p: &HexParams, tile: IVec2) -> [IVec2; 8] {
        let o = self.hex_origin(p, tile);
        [
            IVec2::new(0, p.row_height),
            IVec2::new(0, p.side_offset_y),
            IVec2::new(p.side_offset_x, 0),
            IVec2::new(p.column_width, 0),
            IVec2::new(p.tile_width, p.side_offset_y),
            IVec2::new(p.tile_width, p.row_height),
            IVec2::new(p.column_width, p.tile_height),
            IVec2::new(p.side_offset_x, p.tile_height),
        ]
        .map(|v| o.wrapping_add(v))
    }

    // Outlines.

    /// A cell's outline vertices as integer keys, clockwise: tile corners on square, isometric
    /// and oblique grids, screen pixels on hexagonal and staggered grids.
    fn outline_keys(&self, tile: IVec2) -> Vec<IVec2> {
        if self.is_hex_grid() {
            let mut keys: Vec<IVec2> = self.hex_polygon(&self.hex_params(), tile).to_vec();
            keys.dedup();
            while keys.len() > 1 && keys.first() == keys.last() {
                keys.pop();
            }
            keys
        } else {
            vec![tile, tile + IVec2::X, tile + IVec2::ONE, tile + IVec2::Y]
        }
    }

    fn key_to_screen(&self, key: IVec2) -> DVec2 {
        if self.is_hex_grid() {
            key.as_dvec2()
        } else {
            self.tile_to_screen(key.as_dvec2())
        }
    }

    /// A cell's outline on screen, clockwise: a rectangle, diamond, parallelogram or hexagon.
    pub fn tile_polygon(&self, tile: IVec2) -> Vec<DVec2> {
        self.outline_keys(tile)
            .into_iter()
            .map(|k| self.key_to_screen(k))
            .collect()
    }

    /// The outline of a rectangle of cells (see [`MapGeometry::region_outline`]).
    pub fn rect_outline(&self, rect: IRect) -> Vec<Vec<DVec2>> {
        if rect.is_empty() {
            return Vec::new();
        }
        if self.is_hex_grid() {
            self.region_outline(rect.points())
        } else {
            vec![self.tile_rect_corners(rect).to_vec()]
        }
    }

    /// The outline of the union of `tiles` as closed loops, clockwise on screen, for selections
    /// and brush footprints. Cells that touch only at a corner (on staggered maps) may give
    /// separate loops.
    pub fn region_outline(&self, tiles: impl IntoIterator<Item = IVec2>) -> Vec<Vec<DVec2>> {
        // The directed edges of every cell's outline; an edge two cells share runs both ways and
        // cancels.
        let mut edges: HashMap<Edge, u32> = HashMap::new();
        let mut seen = HashSet::new();
        for tile in tiles {
            if !seen.insert(tile) {
                continue;
            }
            let keys = self.outline_keys(tile);
            for (&a, &b) in keys.iter().zip(keys.iter().cycle().skip(1)) {
                let (from, to) = (Vertex::of(a), Vertex::of(b));
                match edges.get_mut(&Edge { from: to, to: from }) {
                    Some(count) if *count > 0 => *count -= 1,
                    _ => *edges.entry(Edge { from, to }).or_default() += 1,
                }
            }
        }
        let mut next: BTreeMap<Vertex, Vec<Vertex>> = BTreeMap::new();
        for (edge, count) in edges {
            for _ in 0..count {
                next.entry(edge.from).or_default().push(edge.to);
            }
        }
        for targets in next.values_mut() {
            targets.sort_unstable_by(|a, b| b.cmp(a));
        }
        let mut loops = Vec::new();
        while let Some(&start) = next.keys().next() {
            let mut keys = vec![start];
            let mut current = start;
            while let Some(targets) = next.get_mut(&current) {
                let to = targets.pop().unwrap_or(start);
                if targets.is_empty() {
                    next.remove(&current);
                }
                if to == start {
                    break;
                }
                keys.push(to);
                current = to;
            }
            let keys = without_collinear(keys);
            if keys.len() >= 3 {
                loops.push(
                    keys.into_iter()
                        .map(|k| self.key_to_screen(k.ivec2()))
                        .collect(),
                );
            }
        }
        loops
    }

    // Grid.

    /// The size below which Tiled fades the grid out when zooming: the smaller side of a cell,
    /// or of a column and row on hexagonal and staggered maps.
    pub fn grid_fade_size(&self) -> i32 {
        if self.is_hex_grid() {
            let p = self.hex_params();
            p.column_width.min(p.row_height)
        } else {
            self.tile_width.min(self.tile_height)
        }
    }

    /// The grid lines to draw for the visible screen rectangle (Tiled's `drawGrid`); every
    /// `major.x`-th column and `major.y`-th row is major (0 for none). A finite map's grid stops
    /// at the map; as in Tiled, the orthogonal grid leaves the right and bottom edges to the
    /// map's border.
    pub fn grid_lines(&self, visible: DRect, major: IVec2) -> Vec<GridLine> {
        let mut lines = Vec::new();
        match self.orientation {
            Orientation::Orthogonal => self.orthogonal_grid(visible, major, &mut lines),
            Orientation::Isometric => self.isometric_grid(visible, major, &mut lines),
            Orientation::Hexagonal | Orientation::Staggered => {
                self.hexagonal_grid(visible, major, &mut lines)
            }
            Orientation::Oblique => self.oblique_grid(visible, major, &mut lines),
        }
        lines
    }

    fn orthogonal_grid(&self, r: DRect, major: IVec2, out: &mut Vec<GridLine>) {
        let (tw, th) = (self.tile_width, self.tile_height);
        if tw <= 0 || th <= 0 {
            return;
        }
        let (tw_f, th_f) = (f64::from(tw), f64::from(th));
        let mut start = IVec2::new(
            (r.min.x / tw_f).floor() as i32,
            (r.min.y / th_f).floor() as i32,
        );
        let mut end = IVec2::new(
            (r.max.x / tw_f).ceil() as i32,
            (r.max.y / th_f).ceil() as i32,
        );
        if !self.infinite {
            start = start.max(IVec2::ZERO);
            end = end.min(IVec2::new(self.width, self.height));
        }
        let point = |x: i32, y: i32| DVec2::new(f64::from(x) * tw_f, f64::from(y) * th_f);
        if start.y < end.y {
            for x in start.x..end.x {
                out.push(GridLine {
                    from: point(x, start.y),
                    to: point(x, end.y),
                    major: is_major(x, major.x),
                });
            }
        }
        if start.x < end.x {
            for y in start.y..end.y {
                out.push(GridLine {
                    from: point(start.x, y),
                    to: point(end.x, y),
                    major: is_major(y, major.y),
                });
            }
        }
    }

    fn isometric_grid(&self, r: DRect, major: IVec2, out: &mut Vec<GridLine>) {
        let (tw, th) = (self.tile_width, self.tile_height);
        if tw <= 0 || th <= 0 {
            return;
        }
        let Some(a) = r.to_aligned() else { return };
        // Qt's QRect corners are inclusive: right is x + w - 1.
        let (left, top) = (a.x - tw / 2, a.y - th / 2);
        let (right, bottom) = (a.x + a.w - 1 + tw / 2, a.y + a.h - 1 + th / 2);
        let to_tile = |x: i32, y: i32| self.screen_to_tile(DVec2::new(f64::from(x), f64::from(y)));
        let mut start = IVec2::new(to_tile(left, top).x as i32, to_tile(right, top).y as i32);
        let mut end = IVec2::new(
            to_tile(right, bottom).x as i32,
            to_tile(left, bottom).y as i32,
        );
        if !self.infinite {
            start = start.max(IVec2::ZERO);
            end = end.min(IVec2::new(self.width, self.height));
        }
        self.straight_grid(start, end, major, out);
    }

    fn oblique_grid(&self, r: DRect, major: IVec2, out: &mut Vec<GridLine>) {
        let (tw, th) = (f64::from(self.tile_width), f64::from(self.tile_height));
        if tw <= 0.0 || th <= 0.0 {
            return;
        }
        let Some(p) = self.screen_to_pixel_rect(r) else {
            return;
        };
        let mut start = IVec2::new((p.min.x / tw).floor() as i32, (p.min.y / th).floor() as i32);
        let mut end = IVec2::new((p.max.x / tw).ceil() as i32, (p.max.y / th).ceil() as i32);
        if start.x > end.x || start.y > end.y {
            return;
        }
        if !self.infinite {
            start = start.max(IVec2::ZERO);
            end = end.min(IVec2::new(self.width, self.height));
        }
        if start.x > end.x || start.y > end.y {
            return;
        }
        self.straight_grid(start, end, major, out);
    }

    /// Lines of constant row and constant column between cell corners (isometric and oblique).
    fn straight_grid(&self, start: IVec2, end: IVec2, major: IVec2, out: &mut Vec<GridLine>) {
        let point = |x: i32, y: i32| self.tile_to_screen(DVec2::new(f64::from(x), f64::from(y)));
        for y in start.y..=end.y {
            out.push(GridLine {
                from: point(start.x, y),
                to: point(end.x, y),
                major: is_major(y, major.y),
            });
        }
        for x in start.x..=end.x {
            out.push(GridLine {
                from: point(x, start.y),
                to: point(x, end.y),
                major: is_major(x, major.x),
            });
        }
    }

    /// Tiled's `HexagonalRenderer::drawGrid`: the upper edges of every cell, with the map's
    /// border completed by special cases.
    fn hexagonal_grid(&self, visible: DRect, major: IVec2, out: &mut Vec<GridLine>) {
        let Some(rect) = visible.to_aligned() else {
            return;
        };
        if rect.w == 0 && rect.h == 0 {
            return;
        }
        let (right, bottom) = (rect.x + rect.w - 1, rect.y + rect.h - 1);
        let p = self.hex_params();
        let infinite = self.infinite;
        let (map_w, map_h) = (self.width, self.height);

        let mut start_tile = self.hex_start_tile(rect.min().as_dvec2());
        let start_pos = self.hex_origin(&p, start_tile);
        if rect.y - start_pos.y < p.side_offset_y {
            start_tile.y -= 1;
        }
        if rect.x - start_pos.x < p.side_offset_x {
            start_tile.x -= 1;
        }
        if !infinite {
            start_tile = start_tile.max(IVec2::ZERO);
        }
        let mut start_pos = self.hex_origin(&p, start_tile);

        // The upper half of a cell's outline: left side, top-left, top, top-right.
        let left_bottom = IVec2::new(0, p.row_height);
        let left_top = IVec2::new(0, p.side_offset_y);
        let top_start = IVec2::new(p.side_offset_x, 0);
        let top_end = IVec2::new(p.column_width, 0);
        let right_top = IVec2::new(p.tile_width, p.side_offset_y);
        // Staggered maps have zero-length sides; Tiled draws those as dots at most.
        let mut push = |from: IVec2, to: IVec2, at: IVec2, major: bool| {
            if from != to {
                out.push(GridLine {
                    from: (from + at).as_dvec2(),
                    to: (to + at).as_dvec2(),
                    major,
                });
            }
        };

        if p.stagger_x {
            if p.column_width <= 0 || p.tile_height + p.side_length_y <= 0 {
                return;
            }
            if p.do_stagger_x(start_tile.x) {
                start_pos.y -= p.row_height;
            }
            start_tile.x -= 1;
            start_pos.x -= p.column_width;
            while start_pos.x <= right && (start_tile.x <= map_w || infinite) {
                let staggered = p.do_stagger_x(start_tile.x);
                let first_column = !infinite && start_tile.x == -1;
                let last_column = !infinite && start_tile.x == map_w;
                let x_major = is_major(start_tile.x, major.x);
                let next_x_major = is_major(start_tile.x + 1, major.x);
                let mut row_tile = start_tile;
                let mut row_pos = start_pos;
                if staggered {
                    row_pos.y += p.row_height;
                }
                while row_pos.y <= bottom && (row_tile.y <= map_h || infinite) {
                    let y_major = is_major(row_tile.y, major.y);
                    let first_row = !infinite && row_tile.y == 0;
                    let last_row = !infinite && row_tile.y == map_h;
                    if !(first_column
                        || (last_row && (staggered || row_tile.x == 0))
                        || (first_row && last_column && !staggered))
                    {
                        push(
                            left_top,
                            top_start,
                            row_pos,
                            (y_major && !staggered) || x_major,
                        );
                    }
                    if !(first_column || last_column) {
                        push(top_start, top_end, row_pos, y_major);
                    }
                    if !(last_column
                        || (last_row && (staggered || start_tile.x == map_w - 1))
                        || (first_column && first_row && !staggered))
                    {
                        push(
                            top_end,
                            right_top,
                            row_pos,
                            (y_major && !staggered) || next_x_major,
                        );
                    }
                    row_pos.y += p.tile_height + p.side_length_y;
                    row_tile.y += 1;
                }
                start_pos.x += p.column_width;
                start_tile.x += 1;
            }
        } else {
            if p.row_height <= 0 || p.tile_width + p.side_length_x <= 0 {
                return;
            }
            if p.do_stagger_y(start_tile.y) {
                start_pos.x -= p.column_width;
            }
            while start_pos.y <= bottom && (start_tile.y <= map_h || infinite) {
                let y_major = is_major(start_tile.y, major.y);
                let staggered = p.do_stagger_y(start_tile.y);
                let first_row = !infinite && start_tile.y == 0;
                let last_row = !infinite && start_tile.y == map_h;
                let mut row_tile = start_tile;
                let mut row_pos = start_pos;
                if staggered {
                    row_tile.x -= 1;
                    row_pos.x -= p.column_width;
                }
                while row_pos.x <= right && (row_tile.x <= map_w || infinite) {
                    let x_major = is_major(row_tile.x, major.x);
                    let next_x_major = is_major(row_tile.x + 1, major.x);
                    let last_column = !infinite && row_tile.x == map_w;
                    let first_column = !infinite && row_tile.x == -1;
                    if !(last_row || (staggered && first_column)) {
                        push(left_bottom, left_top, row_pos, x_major);
                    }
                    if !(first_column
                        || (last_row && !staggered && row_tile.x == 0)
                        || (last_column && staggered)
                        || (first_row && last_column))
                    {
                        push(
                            left_top,
                            top_start,
                            row_pos,
                            y_major || (!staggered && x_major),
                        );
                    }
                    if !(last_column
                        || (first_row && first_column)
                        || (last_row && staggered && row_tile.x == map_w - 1))
                    {
                        push(
                            top_end,
                            right_top,
                            row_pos,
                            y_major || (staggered && next_x_major),
                        );
                    }
                    row_pos.x += p.tile_width + p.side_length_x;
                    row_tile.x += 1;
                }
                start_pos.y += p.row_height;
                start_tile.y += 1;
            }
        }
    }

    // Objects.

    /// An object position (pixels) snapped to the grid, or to a finer grid of `subdivisions`
    /// cells per tile (Tiled's `snapToGrid`). Hexagonal and staggered grids snap to the nearest
    /// corner of the cell, or to its centre when subdivided. The only snapping code: every tool
    /// calls it.
    pub fn snap_to_grid(&self, pixel: DVec2, subdivisions: i32) -> DVec2 {
        if self.is_hex_grid() {
            let tile = self.hex_screen_to_tile(pixel);
            let polygon = self
                .hex_polygon(&self.hex_params(), tile)
                .map(|v| v.as_dvec2());
            let [left, _, _, _, right, _, _, _] = polygon;
            let center = (left + right) / 2.0;
            let candidates = polygon
                .into_iter()
                .chain((subdivisions > 1).then_some(center));
            let mut nearest = DVec2::ZERO;
            let mut min_dist = f64::MAX;
            for candidate in candidates {
                let dist = candidate.distance_squared(pixel);
                if dist < min_dist {
                    min_dist = dist;
                    nearest = candidate;
                }
            }
            return nearest;
        }
        let tile = self.pixel_to_tile(pixel);
        let snapped = if subdivisions > 1 {
            (tile * f64::from(subdivisions)).round() / f64::from(subdivisions)
        } else {
            tile.round()
        };
        self.tile_to_pixel(snapped)
    }

    /// Tiled's effective object alignment: for a tile object, its tileset's alignment when set,
    /// else bottom-centre on isometric maps and bottom-left elsewhere; top-left for other
    /// objects. Pass the tileset of a tile object, `None` for any other object.
    pub fn object_alignment(&self, tileset: Option<&Tileset>) -> Alignment {
        match tileset {
            None => Alignment::TopLeft,
            Some(t) if t.object_alignment != Alignment::Unspecified => t.object_alignment,
            Some(_) if self.orientation == Orientation::Isometric => Alignment::Bottom,
            Some(_) => Alignment::BottomLeft,
        }
    }

    /// The screen rectangle of an object drawn upright (tile and text objects): at its projected
    /// position, with its size in screen pixels. A tile object still takes its tileset's tile
    /// offset, scaled like its image.
    pub fn upright_object_rect(&self, pos: DVec2, size: DVec2, alignment: Alignment) -> DRect {
        let origin = self.pixel_to_screen(pos);
        DRect::from_min_size(origin - alignment.origin_fraction() * size, size)
    }
}

fn is_major(i: i32, every: i32) -> bool {
    every != 0 && i % every == 0
}

/// A closed polygon without the vertices that lie on a straight line between their neighbours.
fn without_collinear(mut points: Vec<Vertex>) -> Vec<Vertex> {
    let mut i = 0;
    while points.len() > 3 && i < points.len() {
        let n = points.len();
        let around = (
            points.get((i + n - 1) % n),
            points.get(i),
            points.get((i + 1) % n),
        );
        let (Some(&a), Some(&b), Some(&c)) = around else {
            break;
        };
        let cross = (i64::from(b.x) - i64::from(a.x)) * (i64::from(c.y) - i64::from(b.y))
            - (i64::from(b.y) - i64::from(a.y)) * (i64::from(c.x) - i64::from(b.x));
        if cross == 0 {
            points.remove(i);
        } else {
            i += 1;
        }
    }
    points
}

impl Map {
    /// The geometry of the map's grid.
    pub fn geometry(&self) -> MapGeometry {
        MapGeometry::from_map(self)
    }
}
