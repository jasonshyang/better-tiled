//! The grid a map's tiles sit on, as Godot's TileSet has it (docs/design/12-godot-export.md): the TileSet's
//! shape lines, the box a tile's image is centred in, how far Godot's cell centres are from
//! Tiled's (the shift), and the alternative tiles that keep flipped tall tiles in place.
//!
//! | Tiled | Godot `tile_shape`, `tile_layout`, `tile_offset_axis` | `tile_size` |
//! |---|---|---|
//! | orthogonal | square | the map's tile size |
//! | isometric | isometric, diamond down | the map's tile size |
//! | staggered | isometric, stacked (odd) or stacked offset (even), horizontal (axis y) or vertical (x) | Tiled's rounded-down size |
//! | hexagonal, side = 2 × side offset | hexagon, as staggered | Tiled's rounded-down size |
//! | hexagonal, other sides | half-offset square, as staggered | one row (axis y) or column (x) of it |
//!
//! Tiled cell (x, y) is Godot cell (x, y) on every grid. Oblique maps have no Godot grid.

use bt_core::{
    Flip, HexParams, LayerKind, Map, MapGeometry, Orientation, StaggerAxis, StaggerIndex,
};
use glam::{DVec2, IVec2};

use super::report::{Element, Note, Severity};
use super::tileset::TileDef;
use super::{collision, text, tiledata};
use crate::build::Build;

/// Godot's flags for flipped and transposed cells (`TileSetAtlasSource.TRANSFORM_*`), added to
/// a cell's alternative id.
pub(super) const FLIP_H: u16 = 4096;
pub(super) const FLIP_V: u16 = 8192;
pub(super) const TRANSPOSE: u16 = 16384;

/// Godot's `TileSet.TileShape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Square = 0,
    Isometric = 1,
    HalfOffsetSquare = 2,
    Hexagon = 3,
}

/// Godot's `TileSet.TileLayout`, the ones Tiled's grids need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    Stacked = 0,
    StackedOffset = 1,
    DiamondDown = 5,
}

/// Godot's `TileSet.TileOffsetAxis`: whether rows (horizontal) or columns (vertical) shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Horizontal = 0,
    Vertical = 1,
}

pub(super) struct Grid {
    geometry: MapGeometry,
    shape: Shape,
    layout: Layout,
    axis: Axis,
    tile_size: IVec2,
    cell_box: IVec2,
    shift: DVec2,
}

/// A tile's alternative for cells flipped by Godot's transform `flags`: the texture origin that
/// puts the image where Tiled draws it (§4.3). Alternative 0 is the tile itself.
pub(super) struct Flipped {
    pub flags: u16,
    pub origin: IVec2,
    /// Its alternative id: 1, 2, … in the order the map uses them, or for a shared TileSet the
    /// flags' own number (shared.rs).
    pub id: u16,
}

impl Grid {
    /// The map's grid, or why Godot can't have it.
    pub fn of(map: &Map) -> Result<Grid, String> {
        let geometry = MapGeometry::from_map(map);
        let p = geometry.hex_params();
        let staggered = |shape: Shape, size: IVec2| {
            let layout = if map.stagger_index == StaggerIndex::Odd {
                Layout::Stacked
            } else {
                Layout::StackedOffset
            };
            let axis = if map.stagger_axis == StaggerAxis::Y {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            (shape, layout, axis, size)
        };
        let map_size = IVec2::new(map.tile_width as i32, map.tile_height as i32);
        let hex_size = IVec2::new(p.tile_width, p.tile_height);
        let (shape, layout, axis, tile_size) = match map.orientation {
            Orientation::Orthogonal => (Shape::Square, Layout::Stacked, Axis::Horizontal, map_size),
            Orientation::Isometric => (
                Shape::Isometric,
                Layout::DiamondDown,
                Axis::Horizontal,
                map_size,
            ),
            Orientation::Staggered => staggered(Shape::Isometric, hex_size),
            Orientation::Hexagonal if exact_hexagon(&p) => staggered(Shape::Hexagon, hex_size),
            // Rows (or columns) as Godot's half-offset squares, each as tall as Tiled's row.
            Orientation::Hexagonal if p.stagger_x => staggered(
                Shape::HalfOffsetSquare,
                IVec2::new(p.column_width, p.tile_height),
            ),
            Orientation::Hexagonal => staggered(
                Shape::HalfOffsetSquare,
                IVec2::new(p.tile_width, p.row_height),
            ),
            Orientation::Oblique => {
                return Err(
                    "oblique maps can't be exported to Godot: it has no sheared grid".to_owned(),
                );
            }
        };
        if tile_size.min_element() < 1 {
            return Err(format!(
                "Godot's cells are at least 1×1 px, and this map's are {}×{} px",
                tile_size.x, tile_size.y
            ));
        }
        // Tiled anchors a tile's image at the bottom-left of this box around the cell's centre.
        let cell_box = match map.orientation {
            Orientation::Orthogonal => map_size,
            // Tiled's isometric anchor is half the tile width (rounded down) left of the centre.
            Orientation::Isometric => IVec2::new(map_size.x / 2 * 2, map_size.y),
            _ => hex_size,
        };
        let mut grid = Grid {
            geometry,
            shape,
            layout,
            axis,
            tile_size,
            cell_box,
            shift: DVec2::ZERO,
        };
        grid.shift = tiled_centre(&geometry, IVec2::ZERO) - grid.godot_centre(IVec2::ZERO);
        Ok(grid)
    }

    /// The TileSet's `tile_size`.
    pub fn tile_size(&self) -> IVec2 {
        self.tile_size
    }

    /// The TileSet's `tile_shape`, `tile_layout` and `tile_offset_axis`, written before its
    /// `tile_size`; Godot's defaults (a square grid) are left out, as Godot leaves them out.
    pub fn tileset_lines(&self) -> Vec<(String, String)> {
        let lines = [
            ("tile_shape", self.shape as i32),
            ("tile_layout", self.layout as i32),
            ("tile_offset_axis", self.axis as i32),
        ];
        lines
            .into_iter()
            .filter(|(_, v)| *v != 0)
            .map(|(k, v)| (k.to_owned(), v.to_string()))
            .collect()
    }

    /// The box Tiled anchors a tile's image in, which `texture_origin` centres it on.
    pub fn cell_box(&self) -> IVec2 {
        self.cell_box
    }

    /// Tiled's centre of a cell minus Godot's, the same for every cell: zero but on isometric
    /// maps and the hex fallback. TileMapLayers stay at their Tiled offsets (Decision 1), so
    /// whatever else is placed in Tiled's pixels is moved by minus this.
    pub fn shift(&self) -> DVec2 {
        self.shift
    }

    /// Where the node of a layer of `kind` with Tiled's `offset` goes. Objects are moved one by
    /// one (objects.rs), so their layer, like a group, stays at its offset.
    pub fn layer_position(&self, kind: &LayerKind, offset: DVec2) -> DVec2 {
        match kind {
            LayerKind::Image(_) => offset - self.shift(),
            _ => offset,
        }
    }

    /// What Godot does differently on this grid: the hex fallback's warning, and the shift.
    pub fn notes(&self) -> Vec<Note> {
        let g = &self.geometry;
        let mut notes = Vec::new();
        if self.shape == Shape::HalfOffsetSquare {
            notes.push(Note::warning(
                crate::Code::HexFallback,
                format!(
                    "hexagons {}×{} px with {} px sides aren't a hexagon Godot can draw: the TileSet uses half-offset squares, which keep every cell's centre and six neighbours, but Godot picks cells by rectangle and its default collision and navigation shapes are squares",
                    g.tile_width, g.tile_height, g.hex_side_length
                ),
                Vec::new(),
            ));
        }
        if self.shift() != DVec2::ZERO {
            let vector = |v: DVec2| text::vector2(v).trim_start_matches("Vector2").to_owned();
            let message = format!(
                "on this {} map Tiled's cell centres are Godot's plus {} px: the TileMapLayers keep their Tiled offsets, and objects and image layers are moved by {} px to stay over the same cells",
                orientation_name(g.orientation),
                vector(self.shift()),
                vector(-self.shift())
            );
            notes.push(Note {
                code: crate::Code::GridShift,
                severity: Severity::Info,
                message,
                elements: Vec::new(),
            });
        }
        notes
    }

    /// The `texture_origin` that puts a tile image of `size`, moved by its tileset's `offset`,
    /// where Tiled draws it in a cell that Godot transforms by `flags` (0 for none), and whether
    /// it is exact (otherwise Godot draws it half a pixel off).
    ///
    /// Tiled flips the image in place, anchored at the cell box's bottom-left. Godot draws it
    /// centred on the cell, `texture_origin` back from the centre, and then transposes and
    /// mirrors that placement about the centre with the cell (its draw rule, ported by docs/design/12-godot-export.md's
    /// independent check), so the origin is worked out at the drawn size and mirrored ahead.
    pub fn texture_origin(&self, size: IVec2, offset: IVec2, flags: u16) -> (IVec2, bool) {
        let transposed = flags & TRANSPOSE != 0;
        let drawn = if transposed {
            IVec2::new(size.y, size.x)
        } else {
            size
        };
        let b = self.cell_box;
        let mut twice = IVec2::new(b.x - drawn.x, drawn.y - b.y) - 2 * offset;
        if flags & FLIP_H != 0 {
            twice.x = -twice.x;
        }
        if flags & FLIP_V != 0 {
            twice.y = -twice.y;
        }
        if transposed {
            twice = IVec2::new(twice.y, twice.x);
        }
        let exact = twice.x % 2 == 0 && twice.y % 2 == 0;
        (
            IVec2::new(twice.x.div_euclid(2), twice.y.div_euclid(2)),
            exact,
        )
    }

    /// Godot's `TileSet::map_to_local` for the grids written: the centre of `cell`.
    pub(super) fn godot_centre(&self, cell: IVec2) -> DVec2 {
        let mut r = cell.as_dvec2();
        if self.shape != Shape::Square {
            let half = |odd: bool| if odd { 0.5 } else { 0.0 };
            let (odd_x, odd_y) = (cell.x.rem_euclid(2) == 1, cell.y.rem_euclid(2) == 1);
            r = match (self.axis, self.layout) {
                (Axis::Horizontal, Layout::Stacked) => DVec2::new(r.x + half(odd_y), r.y),
                (Axis::Horizontal, Layout::StackedOffset) => DVec2::new(r.x + half(!odd_y), r.y),
                (Axis::Horizontal, Layout::DiamondDown) => DVec2::new((r.x - r.y) / 2.0, r.y + r.x),
                (Axis::Vertical, Layout::Stacked) => DVec2::new(r.x, r.y + half(odd_x)),
                (Axis::Vertical, Layout::StackedOffset) => DVec2::new(r.x, r.y + half(!odd_x)),
                (Axis::Vertical, Layout::DiamondDown) => DVec2::new(r.x - r.y, (r.y + r.x) / 2.0),
            };
        }
        // How much neighbouring rows (or columns) overlap.
        let overlap = match self.shape {
            Shape::Isometric => 0.5,
            Shape::Hexagon => 0.75,
            Shape::Square | Shape::HalfOffsetSquare => 1.0,
        };
        match self.axis {
            Axis::Horizontal => r.y *= overlap,
            Axis::Vertical => r.x *= overlap,
        }
        (r + 0.5) * self.tile_size.as_dvec2()
    }
}

/// Whether Godot's hexagon, whose straight side is half its height (or width, for stagger axis
/// x), is Tiled's.
fn exact_hexagon(p: &HexParams) -> bool {
    if p.stagger_x {
        p.side_length_x == 2 * p.side_offset_x
    } else {
        p.side_length_y == 2 * p.side_offset_y
    }
}

/// The centre of a cell's outline on Tiled's screen.
pub(super) fn tiled_centre(geometry: &MapGeometry, cell: IVec2) -> DVec2 {
    let outline = geometry.tile_polygon(cell);
    let (min, max) = outline
        .iter()
        .fold((DVec2::INFINITY, DVec2::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
    (min + max) / 2.0
}

fn orientation_name(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Orthogonal => "orthogonal",
        Orientation::Isometric => "isometric",
        Orientation::Staggered => "staggered",
        Orientation::Hexagonal => "hexagonal",
        Orientation::Oblique => "oblique",
    }
}

impl Build<'_> {
    /// The alternative id of a cell flipped by Tiled's `flip`, whose tile is `atlas` in atlas
    /// `source` (tile `id` of the tileset in `slot`, an image of `size`). That is Godot's
    /// transform flags on alternative 0, unless Godot would then draw the tile elsewhere than
    /// Tiled does: then the flags on an alternative of the tile's own, with the texture origin
    /// that draws it in place (one per combination of flags the map uses).
    pub(super) fn alternative(
        &mut self,
        source: usize,
        atlas: IVec2,
        slot: usize,
        id: u32,
        size: IVec2,
        flip: Flip,
    ) -> u16 {
        let flags = self.transform_flags(slot, id, flip);
        if flags == 0 {
            return 0;
        }
        let Some(ts) = self.map.tilesets.get(slot).map(|t| &t.tileset) else {
            self.failures
                .push(format!("tileset slot {slot} is missing"));
            return flags;
        };
        let (origin, exact) = self.grid.texture_origin(size, ts.tile_offset, flags);
        // Tiles that are off when not flipped have their warning already.
        if !exact && self.grid.texture_origin(size, ts.tile_offset, 0).1 {
            let b = self.grid.cell_box();
            self.warn_once(
                crate::Code::HalfPixelOrigin,
                format!("tileset {}: tiles {}×{} px turned sideways don't center on {}×{} px cells; Godot draws them half a pixel off", ts.name, size.x, size.y, b.x, b.y),
                vec![Element::Tileset(slot)],
            );
        }
        let Some(def) = self
            .tileset
            .sources
            .get_mut(source)
            .and_then(|s| s.tiles.get_mut(&(atlas.x, atlas.y)))
        else {
            self.failures
                .push(format!("tile at {atlas} is missing from source {source}"));
            return flags;
        };
        if origin == def.origin {
            return flags;
        }
        let id = match def.flipped.iter().find(|f| f.flags == flags) {
            Some(f) => f.id,
            None => {
                let id = def.flipped.len() as u16 + 1;
                def.flipped.push(Flipped { flags, origin, id });
                id
            }
        };
        id | flags
    }

    /// Godot's transform flags for Tiled's flip bits. On hexagonal maps Tiled turns tiles by 60°
    /// and 120° instead of transposing them: Godot can't, so only their half turn (a mirror both
    /// ways) is kept.
    fn transform_flags(&mut self, slot: usize, id: u32, flip: Flip) -> u16 {
        let mirrors = if flip.contains(bt_core::Flip::H) {
            FLIP_H
        } else {
            0
        } | if flip.contains(bt_core::Flip::V) {
            FLIP_V
        } else {
            0
        };
        if self.map.orientation != Orientation::Hexagonal {
            return mirrors
                | if flip.contains(bt_core::Flip::D) {
                    TRANSPOSE
                } else {
                    0
                };
        }
        match (
            flip.contains(bt_core::Flip::D),
            flip.contains(bt_core::Flip::HEX120),
        ) {
            (false, false) => mirrors,
            (true, true) => mirrors ^ (FLIP_H | FLIP_V),
            _ => {
                self.warn_once(
                    crate::Code::HexRotation,
                    "tiles turned by 60° or 120° on a hexagonal map can't be turned in Godot: they are drawn unturned".to_owned(),
                    vec![Element::Tile { tileset: slot, id }],
                );
                mirrors
            }
        }
    }

    /// The lines of a tile's alternatives for flipped cells, each to go after `x:y/`: what
    /// alternative 0 has, with the image and its collision moved by the difference in origin.
    /// Terrain stays on alternative 0 alone: its peering sides would need flipping too.
    pub(super) fn flipped_lines(&self, def: &TileDef) -> Vec<(String, String)> {
        let mut lines = Vec::new();
        for flipped in &def.flipped {
            let n = flipped.id;
            lines.push((n.to_string(), n.to_string()));
            if flipped.origin != IVec2::ZERO {
                lines.push((
                    format!("{n}/texture_origin"),
                    text::vector2i(flipped.origin),
                ));
            }
            let moved = (def.origin - flipped.origin).as_dvec2();
            for (key, value) in collision::tile_lines(&def.polygons, moved)
                .into_iter()
                .chain(tiledata::tile_lines(
                    self.map,
                    &self.tileset,
                    def.slot,
                    def.id,
                ))
            {
                lines.push((format!("{n}/{key}"), value));
            }
        }
        lines
    }
}
