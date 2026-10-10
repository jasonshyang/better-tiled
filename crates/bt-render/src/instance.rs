//! The sprite instance the GPU draws, and Tiled's placement of cells and tile objects (its
//! `CellRenderer`): anchoring, tile offset, render size and fill mode, flips and hexagonal turns.

use bt_core::{
    Cell, Color, FillMode, Flip, MapGeometry, MapObject, Orientation, TileRenderSize, Tileset,
};
use bt_utils::DRect;
use glam::{DVec2, IVec2};

/// Mirrored left-right.
pub(crate) const FLIP_H: u32 = 1;
/// Mirrored top-bottom.
pub(crate) const FLIP_V: u32 = 2;
/// Transposed (mirrored along the anti-diagonal).
pub(crate) const TRANSPOSE: u32 = 4;
/// A solid quad in the instance's colour.
pub(crate) const SOLID: u32 = 8;
/// Tiled's marker for something that cannot be drawn.
pub(crate) const MARKER: u32 = 16;

/// One quad: a destination rectangle in layer-local world pixels, how it turns, what it shows.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Instance {
    /// x, y, width, height in world pixels.
    pub(crate) dst: [f32; 4],
    /// The pivot (x, y) of a clockwise rotation in radians, then a clockwise spin in radians
    /// about the image's own centre that is applied first (hexagonal turns).
    pub(crate) pivot_rot: [f32; 4],
    /// The sprite's index (before the animation remap).
    pub(crate) sprite: u32,
    /// [`FLIP_H`], [`FLIP_V`], [`TRANSPOSE`], [`SOLID`], [`MARKER`].
    pub(crate) flags: u32,
    /// A straight-alpha colour the texels are multiplied by.
    pub(crate) color: [u8; 4],
    /// Unused; keeps the size a multiple of 16 bytes.
    pub(crate) pad: u32,
}

/// The size of an instance in a vertex buffer.
pub(crate) const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;

const OPAQUE: [u8; 4] = [255, 255, 255, 255];

/// How a cell's flips are drawn: whether the quad's sides swap, the shader's flags, and a turn.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct CellTransform {
    pub(crate) swap: bool,
    pub(crate) flags: u32,
    pub(crate) spin: f32,
}

/// Tiled's meaning of a cell's flips. On square and staggered grids `D` transposes the image,
/// drawn with the sides swapped so it stays anchored at its bottom-left. On hexagonal grids the
/// image is mirrored, then turned 60° for `D` and 120° for `HEX120`; a half turn is drawn as a
/// mirror on both axes, which keeps pixel art exact.
pub(crate) fn cell_transform(flip: Flip, orientation: Orientation) -> CellTransform {
    let mirror = u32::from((flip & (Flip::H | Flip::V)).bits());
    if orientation != Orientation::Hexagonal {
        let transpose = flip.contains(Flip::D);
        return CellTransform {
            swap: transpose,
            flags: mirror | if transpose { TRANSPOSE } else { 0 },
            spin: 0.0,
        };
    }
    let degrees =
        60 * u32::from(flip.contains(Flip::D)) + 120 * u32::from(flip.contains(Flip::HEX120));
    match degrees {
        0 => CellTransform {
            swap: false,
            flags: mirror,
            spin: 0.0,
        },
        180 => CellTransform {
            swap: false,
            flags: mirror ^ (FLIP_H | FLIP_V),
            spin: 0.0,
        },
        _ => CellTransform {
            swap: false,
            flags: mirror,
            spin: (degrees as f32).to_radians(),
        },
    }
}

/// Where Tiled draws a cell's image from: its anchor, with isometric maps' half pixels landing
/// as Tiled's loop puts them (x truncated, y a half rounded up).
pub(crate) fn drawn_anchor(geometry: &MapGeometry, at: IVec2) -> DVec2 {
    let anchor = geometry.cell_anchor(at);
    match geometry.orientation {
        Orientation::Isometric => DVec2::new(anchor.x.floor(), (anchor.y + 0.5).floor()),
        _ => anchor,
    }
}

/// Where Tiled's `CellRenderer` draws a tile image in a target box: scaled to the box (uniformly
/// for `PreserveAspectFit`) about the box's centre, moved by the tile offset scaled the same way,
/// and on square grids turned a quarter for `D` about a centre moved by half the difference of
/// the box's sides, so the image stays anchored at the box's bottom-left. The result is the
/// quad's rectangle as the shader draws it (its sides swapped for `D`), with the flags and the
/// hexagonal turn.
struct Placed {
    dst: DRect,
    transform: CellTransform,
}

fn placed(target: DRect, tileset: &Tileset, cell: Cell, orientation: Orientation) -> Placed {
    let image = tileset
        .tile_pixel_size(cell.tile())
        .as_dvec2()
        .max(DVec2::ONE);
    let size = target.size();
    let mut scale = size / image;
    if tileset.fill_mode == FillMode::PreserveAspectFit {
        scale = DVec2::splat(scale.x.min(scale.y));
    }
    let drawn = image * scale;
    let mut center = (target.min + target.max) * 0.5 + tileset.tile_offset.as_dvec2() * scale;
    let transform = cell_transform(cell.flip(), orientation);
    let quad = if transform.swap {
        let half_difference = (size.y - size.x) * 0.5;
        center += DVec2::splat(half_difference);
        DVec2::new(drawn.y, drawn.x)
    } else {
        drawn
    };
    Placed {
        dst: DRect::from_min_size(center - quad * 0.5, quad),
        transform,
    }
}

/// A cell on a tile layer (or a preview): Tiled's `CellRenderer` with the target box at the
/// cell's anchor, the map's cell for the `Grid` render size or the tile's own size otherwise.
/// `sprite` is `None` when the tile cannot be drawn: Tiled's marker, the size of a map cell,
/// takes its place.
pub(crate) fn cell_instance(
    geometry: &MapGeometry,
    tileset: Option<&Tileset>,
    cell: Cell,
    at: IVec2,
    sprite: Option<u32>,
) -> Instance {
    let anchor = drawn_anchor(geometry, at);
    let grid = DVec2::new(
        f64::from(geometry.tile_width),
        f64::from(geometry.tile_height),
    );
    let missing = || {
        marker(
            DRect::from_min_size(DVec2::new(anchor.x, anchor.y - grid.y), grid),
            OPAQUE,
        )
    };
    let Some(tileset) = tileset else {
        return missing();
    };
    let Some(sprite) = sprite else {
        return missing();
    };
    let size = match tileset.tile_render_size {
        TileRenderSize::Tile => tileset
            .tile_pixel_size(cell.tile())
            .as_dvec2()
            .max(DVec2::ONE),
        TileRenderSize::Grid => grid,
    };
    let target = DRect::from_min_size(DVec2::new(anchor.x, anchor.y - size.y), size);
    let placed = placed(target, tileset, cell, geometry.orientation);
    Instance {
        dst: rect_array(placed.dst),
        pivot_rot: [0.0, 0.0, 0.0, placed.transform.spin],
        sprite,
        flags: placed.transform.flags,
        color: OPAQUE,
        pad: 0,
    }
}

/// A tile object: drawn upright at its projected position, its box (its size, or the tile's
/// when it has none) placed by its alignment, the image drawn in the box as on tile layers, and
/// the whole turned clockwise about the alignment origin. `sprite` is `None` when the tile cannot
/// be drawn: the marker covers the object's box.
pub(crate) fn object_instance(
    geometry: &MapGeometry,
    tileset: Option<&Tileset>,
    object: &MapObject,
    cell: Cell,
    sprite: Option<u32>,
) -> Instance {
    let origin = geometry.pixel_to_screen(object.pos);
    let alpha = (object.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let color = [255, 255, 255, alpha];
    let rotation = object.rotation.to_radians() as f32;
    let fallback = Tileset::default();
    let alignment = geometry.object_alignment(Some(tileset.unwrap_or(&fallback)));
    let sized = object.size.x > 0.0 && object.size.y > 0.0;
    let missing = || {
        let size = if sized {
            object.size
        } else {
            DVec2::new(
                f64::from(geometry.tile_width),
                f64::from(geometry.tile_height),
            )
        };
        let min = origin - alignment.origin_fraction() * size;
        let mut instance = marker(DRect::from_min_size(min, size), color);
        instance.pivot_rot = [origin.x as f32, origin.y as f32, rotation, 0.0];
        instance
    };
    let Some(tileset) = tileset else {
        return missing();
    };
    let Some(sprite) = sprite else {
        return missing();
    };
    let size = if sized {
        object.size
    } else {
        tileset
            .tile_pixel_size(cell.tile())
            .as_dvec2()
            .max(DVec2::ONE)
    };
    let target = DRect::from_min_size(origin - alignment.origin_fraction() * size, size);
    let placed = placed(target, tileset, cell, geometry.orientation);
    Instance {
        dst: rect_array(placed.dst),
        pivot_rot: [
            origin.x as f32,
            origin.y as f32,
            rotation,
            placed.transform.spin,
        ],
        sprite,
        flags: placed.transform.flags,
        color,
        pad: 0,
    }
}

/// A solid quad.
pub(crate) fn solid(rect: DRect, color: Color) -> Instance {
    Instance {
        dst: rect_array(rect),
        pivot_rot: [0.0; 4],
        sprite: 0,
        flags: SOLID,
        color: color.to_array(),
        pad: 0,
    }
}

/// Tiled's marker over `rect`.
pub(crate) fn marker(rect: DRect, color: [u8; 4]) -> Instance {
    Instance {
        dst: rect_array(rect),
        pivot_rot: [0.0; 4],
        sprite: 0,
        flags: MARKER,
        color,
        pad: 0,
    }
}

/// A sprite drawn plainly over `rect`.
pub(crate) fn plain(rect: DRect, sprite: u32, color: [u8; 4]) -> Instance {
    Instance {
        dst: rect_array(rect),
        pivot_rot: [0.0; 4],
        sprite,
        flags: 0,
        color,
        pad: 0,
    }
}

fn rect_array(rect: DRect) -> [f32; 4] {
    let size = rect.size();
    [
        rect.min.x as f32,
        rect.min.y as f32,
        size.x as f32,
        size.y as f32,
    ]
}

#[cfg(test)]
mod tests {
    use bt_core::{TileId, TilesetSlot};
    use glam::UVec2;

    use super::*;

    fn tileset(size: UVec2) -> Tileset {
        Tileset {
            tile_width: size.x,
            tile_height: size.y,
            ..Tileset::default()
        }
    }

    fn geometry(orientation: Orientation, tile: IVec2) -> MapGeometry {
        MapGeometry::new(orientation, tile, IVec2::new(10, 10))
    }

    fn cell(flip: Flip) -> Cell {
        Cell::new(TilesetSlot::new(0).unwrap(), TileId(0)).with_flip(flip)
    }

    #[test]
    fn cells_hang_from_their_bottom_left_with_offset_and_size() {
        let g = geometry(Orientation::Orthogonal, IVec2::new(16, 16));
        let tall = tileset(UVec2::new(16, 32));
        let i = cell_instance(
            &g,
            Some(&tall),
            cell(Flip::empty()),
            IVec2::new(2, 3),
            Some(5),
        );
        assert_eq!(i.dst, [32.0, 64.0 - 32.0, 16.0, 32.0]);
        assert_eq!(i.sprite, 5);
        // A tile offset moves it; the grid render size stretches it to the cell.
        let mut moved = tileset(UVec2::new(8, 8));
        moved.tile_offset = IVec2::new(2, -1);
        moved.tile_render_size = TileRenderSize::Grid;
        let i = cell_instance(&g, Some(&moved), cell(Flip::empty()), IVec2::ZERO, Some(1));
        assert_eq!(i.dst, [4.0, -2.0, 16.0, 16.0]);
        // Fitting keeps proportions and centres the image.
        let mut wide = tileset(UVec2::new(32, 16));
        wide.tile_render_size = TileRenderSize::Grid;
        wide.fill_mode = FillMode::PreserveAspectFit;
        let i = cell_instance(&g, Some(&wide), cell(Flip::empty()), IVec2::ZERO, Some(1));
        assert_eq!(i.dst, [0.0, 16.0 - 8.0 - 4.0, 16.0, 8.0]);
        // Fitted and transposed: Tiled turns the fitted image about the cell's centre.
        let i = cell_instance(&g, Some(&wide), cell(Flip::D), IVec2::ZERO, Some(1));
        assert_eq!(i.dst, [4.0, 0.0, 8.0, 16.0]);
        // No sprite: the marker over the cell.
        let i = cell_instance(&g, Some(&tall), cell(Flip::empty()), IVec2::new(1, 0), None);
        assert_eq!((i.dst, i.flags), ([16.0, 0.0, 16.0, 16.0], MARKER));
    }

    #[test]
    fn flips_follow_tileds_meaning_per_orientation() {
        let square = cell_transform(Flip::D | Flip::H, Orientation::Orthogonal);
        assert_eq!(
            square,
            CellTransform {
                swap: true,
                flags: FLIP_H | TRANSPOSE,
                spin: 0.0
            }
        );
        let g = geometry(Orientation::Orthogonal, IVec2::new(16, 16));
        let wide = tileset(UVec2::new(32, 16));
        let i = cell_instance(&g, Some(&wide), cell(Flip::D), IVec2::ZERO, Some(1));
        assert_eq!(
            i.dst,
            [0.0, 16.0 - 32.0, 16.0, 32.0],
            "sides swap, bottom-left stays"
        );
        let hex = |flip| cell_transform(flip, Orientation::Hexagonal);
        assert_eq!(hex(Flip::D).spin, 60f32.to_radians());
        assert_eq!(hex(Flip::HEX120).spin, 120f32.to_radians());
        assert_eq!(
            hex(Flip::D | Flip::HEX120),
            CellTransform {
                swap: false,
                flags: FLIP_H | FLIP_V,
                spin: 0.0
            }
        );
        assert_eq!(hex(Flip::D | Flip::HEX120 | Flip::H).flags, FLIP_V);
    }

    #[test]
    fn isometric_anchors_land_on_tileds_pixels() {
        let g = MapGeometry::new(Orientation::Isometric, IVec2::new(15, 7), IVec2::new(3, 3));
        let exact = g.cell_anchor(IVec2::new(1, 0));
        let drawn = drawn_anchor(&g, IVec2::new(1, 0));
        assert_eq!(drawn, DVec2::new(exact.x.floor(), (exact.y + 0.5).floor()));
        assert_eq!(drawn.fract(), DVec2::ZERO);
    }

    #[test]
    fn tile_objects_are_aligned_and_turn_about_their_origin() {
        let g = geometry(Orientation::Orthogonal, IVec2::new(16, 16));
        let t = tileset(UVec2::new(16, 16));
        let object = MapObject {
            pos: DVec2::new(40.0, 50.0),
            size: DVec2::new(32.0, 32.0),
            rotation: 90.0,
            opacity: 0.5,
            ..MapObject::default()
        };
        let i = object_instance(&g, Some(&t), &object, cell(Flip::empty()), Some(3));
        // Bottom-left aligned: the box hangs up from the position.
        assert_eq!(i.dst, [40.0, 18.0, 32.0, 32.0]);
        assert_eq!(i.pivot_rot[..3], [40.0, 50.0, 90f32.to_radians()]);
        assert_eq!(i.color[3], 128);
        let iso = geometry(Orientation::Isometric, IVec2::new(32, 16));
        let i = object_instance(
            &iso,
            Some(&t),
            &MapObject {
                size: DVec2::new(32.0, 32.0),
                ..MapObject::default()
            },
            cell(Flip::empty()),
            Some(3),
        );
        let origin = iso.pixel_to_screen(DVec2::ZERO);
        assert_eq!(
            i.dst[0],
            (origin.x - 16.0) as f32,
            "bottom-centre on isometric maps"
        );
    }
}
