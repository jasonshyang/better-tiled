//! Feet (`10-agents.md`, §2, §8.4): where a sprite touches the ground, which is rarely the
//! corner Tiled anchors tile objects by. A foot is the middle of the lowest opaque pixels of the
//! tile's image (a tree's trunk, a house's doorstep), measured once per image and part of it and
//! kept by [`Feet`]; the caller owns the cache.
//!
//! Points are in tile units: `10.5,5.5` is the centre of cell 10,5 and `10.5,6` the middle of
//! its bottom edge. On hexagonal and staggered maps, whose cells overlap in rows or columns, a
//! point is a cell and a place in the cell's bounding box (`10.5,6` is the middle of the bottom
//! of cell 10,5's box), so every point names a cell. A point stands in a cell
//! ([`standing_cell`]); a point on a cell's bottom edge stands in that cell.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{
    Cell, Flip, ImageSource, Map, MapGeometry, MapObject, ObjectFrame, Shape, object_outline,
    polygon_holds_or_bounds, rotated, tile_object_box,
};
use bt_image::{ColorKey, DecodedImage, bottom_center};
use bt_utils::hash::fnv1a_64;
use bt_utils::{DRect, IRect};
use glam::{DVec2, IVec2};

use crate::error::Error;
use crate::grid::touching;

/// Feet measured so far, by image and the part of it a tile is.
#[derive(Debug, Default)]
pub struct Feet {
    images: HashMap<ImageKey, Result<Arc<DecodedImage>, bt_image::Error>>,
    measured: HashMap<FootKey, Option<Measured>>,
}

/// Which decoded image: a file, or bytes embedded in a Tiled file, with its colour key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ImageKey {
    source: ImageId,
    key: Option<ColorKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ImageId {
    File { path: PathBuf },
    Embedded { hash: u64, length: usize },
}

/// A part of an image whose foot is measured.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FootKey {
    image: ImageKey,
    rect: IRect,
}

/// A tile's foot as a part of its image's width and height from its top-left: as drawn, and as
/// drawn upside down.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Measured {
    upright: DVec2,
    flipped: DVec2,
}

impl Feet {
    /// No feet measured yet.
    pub fn new() -> Feet {
        Feet::default()
    }

    /// The foot of a tile's image as a part of its width and height from its top-left, for the
    /// tile mirrored as its flips say (tile objects mirror left-right and top-bottom).
    ///
    /// Pixels count when their alpha is at least three quarters of the most opaque pixel's, so
    /// a semi-transparent shadow drawn under a tree does not, and a sprite drawn
    /// semi-transparent still has a foot.
    pub fn fraction(&mut self, map: &Map, tile: Cell) -> Result<DVec2, Error> {
        let tileset = map.tileset_of(tile).ok_or(Error::NoTileset {
            tile: tile.tile().0,
        })?;
        let image = tileset
            .tile_image(tile.tile())
            .ok_or_else(|| Error::NoImage {
                tile: tile_words(map, tile),
            })?;
        let image_key = ImageKey {
            source: match &image.image.source {
                ImageSource::File(file) => ImageId::File {
                    path: file.path.clone(),
                },
                ImageSource::Embedded(embedded) => ImageId::Embedded {
                    hash: fnv1a_64(&embedded.bytes),
                    length: embedded.bytes.len(),
                },
            },
            key: image.image.transparent.map(|c| ColorKey {
                r: c.r,
                g: c.g,
                b: c.b,
            }),
        };
        let foot_key = FootKey {
            image: image_key.clone(),
            rect: image.rect,
        };
        let measured = match self.measured.get(&foot_key) {
            Some(measured) => *measured,
            None => {
                let decoded = self
                    .images
                    .entry(image_key)
                    .or_insert_with(|| decode(&image.image.source, image.image.transparent))
                    .clone()
                    .map_err(|error| Error::Image {
                        tile: tile_words(map, tile),
                        error,
                    })?;
                let measured = measure(&decoded, image.rect);
                self.measured.insert(foot_key, measured);
                measured
            }
        };
        let measured = measured.ok_or_else(|| Error::Invisible {
            tile: tile_words(map, tile),
        })?;
        let flip = tile.flip();
        let mut foot = if flip.contains(Flip::V) {
            measured.flipped
        } else {
            measured.upright
        };
        if flip.contains(Flip::H) {
            foot.x = 1.0 - foot.x;
        }
        Ok(foot)
    }

    /// Where an object stands, in tile units: a tile object's foot (turned with it), a point's
    /// position, the middle of any other shape.
    pub fn standing_point(&mut self, map: &Map, object: &MapObject) -> Result<DVec2, Error> {
        let geometry = map.geometry();
        let screen = match object.tile {
            Some(_) => {
                let local = self.foot_in_frame(map, object)?;
                ObjectFrame::of(&geometry, object).to_screen(local)
            }
            None if matches!(object.shape, Shape::Point) => {
                return Ok(pixel_to_point(&geometry, object.pos));
            }
            None => {
                let bounds = object_outline(&geometry, object, None).bounds();
                (bounds.min + bounds.max) * 0.5
            }
        };
        Ok(pixel_to_point(&geometry, geometry.screen_to_pixel(screen)))
    }

    /// The position (in pixels, as objects keep it) that puts the foot of a tile object at
    /// `foot` (in tile units), for its tile, size, flips and rotation.
    pub fn position_for_foot(
        &mut self,
        map: &Map,
        object: &MapObject,
        foot: DVec2,
    ) -> Result<DVec2, Error> {
        let geometry = map.geometry();
        let local = self.foot_in_frame(map, object)?;
        let screen = geometry.pixel_to_screen(point_to_pixel(&geometry, foot));
        Ok(geometry.screen_to_pixel(screen - rotated(local, object.rotation)))
    }

    /// A tile object's foot in its frame: screen pixels from its projected position, before it
    /// turns.
    fn foot_in_frame(&mut self, map: &Map, object: &MapObject) -> Result<DVec2, Error> {
        let not_tile = || Error::NotATileObject { id: object.id.0 };
        let tile = object.tile.ok_or_else(not_tile)?;
        let fraction = self.fraction(map, tile)?;
        let image =
            tile_object_box(&map.geometry(), object, map.tileset_of(tile)).ok_or_else(not_tile)?;
        Ok(image.min + fraction * image.size())
    }
}

/// How far from a cell's edge, in cells, a point still counts as on it: the noise of turning
/// and projecting positions, far below a pixel.
const ON_EDGE: f64 = 1e-6;

/// The cell a point stands in: the cell it is inside, a point on a cell's bottom edge in that
/// cell, a point on its left edge in that cell.
pub fn standing_cell(point: DVec2) -> IVec2 {
    let x = (point.x + ON_EDGE).floor();
    let y = (point.y - ON_EDGE).ceil() - 1.0;
    IVec2::new(to_i32(x), to_i32(y))
}

/// The pixel position (where objects keep their positions) of a point in tile units.
pub fn point_to_pixel(geometry: &MapGeometry, point: DVec2) -> DVec2 {
    if geometry.is_hex_grid() {
        let cell = standing_cell(point);
        let cell_box = cell_box(geometry, cell);
        cell_box.min + (point - cell.as_dvec2()) * cell_box.size()
    } else {
        geometry.tile_to_pixel(point)
    }
}

/// The point in tile units of a pixel position. On hexagonal and staggered maps it is a place in
/// the cell the pixel is in (a pixel on an outline in the cell whose centre is higher, then
/// further right), so its standing cell is that cell.
pub fn pixel_to_point(geometry: &MapGeometry, pixel: DVec2) -> DVec2 {
    if geometry.is_hex_grid() {
        let cell = cell_at_pixel(geometry, pixel);
        let cell_box = cell_box(geometry, cell);
        cell.as_dvec2() + (pixel - cell_box.min) / cell_box.size().max(DVec2::ONE)
    } else {
        geometry.pixel_to_tile(pixel)
    }
}

/// The bounding box of a hexagonal or staggered cell, in pixels (which are screen pixels on
/// these maps).
fn cell_box(geometry: &MapGeometry, cell: IVec2) -> DRect {
    let params = geometry.hex_params();
    DRect::from_min_size(
        geometry.tile_to_screen(cell.as_dvec2()),
        DVec2::new(f64::from(params.tile_width), f64::from(params.tile_height)),
    )
}

/// The hexagonal or staggered cell whose outline holds `pixel`, a pixel on outlines in the cell
/// whose centre is highest, then furthest right; the cell with the nearest centre when no
/// outline holds it (a pixel lost to rounding).
fn cell_at_pixel(geometry: &MapGeometry, pixel: DVec2) -> IVec2 {
    let nearest = geometry.pixel_to_tile(pixel).floor();
    let nearest = IVec2::new(to_i32(nearest.x), to_i32(nearest.y));
    std::iter::once(nearest)
        .chain(touching(geometry, nearest))
        .filter(|&cell| polygon_holds_or_bounds(&geometry.tile_polygon(cell), pixel))
        .min_by(|&a, &b| {
            let (a, b) = (cell_box(geometry, a), cell_box(geometry, b));
            a.min
                .y
                .total_cmp(&b.min.y)
                .then_with(|| b.min.x.total_cmp(&a.min.x))
        })
        .unwrap_or(nearest)
}

/// A coordinate as a cell index, saturating far outside any map.
pub(crate) fn to_i32(value: f64) -> i32 {
    // `as` saturates, and NaN becomes 0.
    value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// A tile in words: "tile 3 of “trees”".
fn tile_words(map: &Map, tile: Cell) -> String {
    match map.tileset_of(tile) {
        Some(tileset) => format!("tile {} of “{}”", tile.tile(), tileset.name),
        None => format!("tile {} of a tileset the map does not have", tile.tile()),
    }
}

fn decode(
    source: &ImageSource,
    transparent: Option<bt_core::Color>,
) -> Result<Arc<DecodedImage>, bt_image::Error> {
    let key = transparent.map(|c| ColorKey {
        r: c.r,
        g: c.g,
        b: c.b,
    });
    let decoded = match source {
        ImageSource::File(file) => bt_image::decode_file(&file.path, key),
        ImageSource::Embedded(embedded) => {
            bt_image::decode_bytes(&embedded.bytes, &embedded.format, key)
        }
    }?;
    Ok(Arc::new(decoded))
}

/// The foot of the part `rect` of `image` (all of it when `rect` is empty), drawn and upside
/// down, as parts of `rect`'s size; `None` when no pixel of it is visible.
fn measure(image: &DecodedImage, rect: IRect) -> Option<Measured> {
    let whole = IRect::from_min_size(
        IVec2::ZERO,
        IVec2::new(
            i32::try_from(image.width).ok()?,
            i32::try_from(image.height).ok()?,
        ),
    );
    let rect = if rect.is_empty() { whole } else { rect };
    let part = rect.intersection(whole);
    if part.is_empty() {
        return None;
    }
    let cut = cut_out(image, part, false)?;
    let upside_down = cut_out(image, part, true)?;
    let most = cut
        .rgba
        .chunks_exact(4)
        .filter_map(|p| p.get(3))
        .max()
        .copied()?;
    if most == 0 {
        return None;
    }
    // Opaque: at least three quarters of the most opaque pixel's alpha.
    let threshold = u8::try_from((u16::from(most) * 3).div_ceil(4).saturating_sub(1)).ok()?;
    let size = rect.size().as_dvec2();
    let offset = (part.min() - rect.min()).as_dvec2();
    let upright = bottom_center(&cut, threshold)?;
    let flipped = bottom_center(&upside_down, threshold)?;
    // Upside down, the part sits at the other end of the rectangle.
    let flipped_offset = DVec2::new(offset.x, f64::from(rect.h) - f64::from(part.h) - offset.y);
    Some(Measured {
        upright: (offset + upright) / size,
        flipped: (flipped_offset + flipped) / size,
    })
}

/// The pixels of `part` of `image`, upside down when asked.
fn cut_out(image: &DecodedImage, part: IRect, upside_down: bool) -> Option<DecodedImage> {
    let width = usize::try_from(image.width).ok()?;
    let (x, w) = (usize::try_from(part.x).ok()?, usize::try_from(part.w).ok()?);
    let mut rgba = Vec::with_capacity(w * usize::try_from(part.h).ok()? * 4);
    let rows: Vec<i32> = if upside_down {
        (part.y..part.y + part.h).rev().collect()
    } else {
        (part.y..part.y + part.h).collect()
    };
    for row in rows {
        let start = (usize::try_from(row).ok()? * width + x) * 4;
        rgba.extend_from_slice(image.rgba.get(start..start + w * 4)?);
    }
    Some(DecodedImage {
        width: u32::try_from(part.w).ok()?,
        height: u32::try_from(part.h).ok()?,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use bt_core::{
        FileRef, ImageRef, ObjectId, Orientation, StaggerAxis, TileData, TileId, Tileset,
        TilesetImage, TilesetSlot,
    };
    use bt_test_data::Scratch;
    use bt_test_data::fixtures::MapBuilder;
    use glam::UVec2;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A 32×48 tree: a crown, a four-pixel trunk at x 10–13 ending at row 40, and below it a
    /// soft shadow, wider and to the right, that a foot must not stand on.
    fn tree(shadow_alpha: u8) -> DecodedImage {
        let (width, height) = (32usize, 48usize);
        let mut rgba = vec![0u8; width * height * 4];
        let mut put = |x: usize, y: usize, alpha: u8| {
            let at = (y * width + x) * 4;
            rgba[at..at + 4].copy_from_slice(&[40, 120, 40, alpha]);
        };
        for y in 4..28 {
            for x in 2..24 {
                put(x, y, 255);
            }
        }
        for y in 28..40 {
            for x in 10..14 {
                put(x, y, 255);
            }
        }
        // The trunk's last row is anti-aliased.
        put(10, 40, 90);
        for y in 40..46 {
            for x in 14..30 {
                put(x, y, shadow_alpha);
            }
        }
        DecodedImage {
            width: 32,
            height: 48,
            rgba,
        }
    }

    /// A map with one image-collection tileset whose tile 0 is `image`, written to `dir`.
    fn map_with(dir: &Path, image: &DecodedImage, orientation: Orientation) -> (Map, Cell) {
        let path = dir.join("tree.png");
        bt_image::write_png(&path, image).unwrap();
        let mut tiles = BTreeMap::new();
        tiles.insert(
            TileId(0),
            TileData {
                image: Some(ImageRef::file(
                    FileRef::new("tree.png", path),
                    UVec2::new(image.width, image.height),
                )),
                ..TileData::default()
            },
        );
        let tileset = Tileset {
            name: "trees".into(),
            tile_width: image.width,
            tile_height: image.height,
            image: TilesetImage::Collection,
            tiles,
            next_tile_id: 1,
            ..Tileset::default()
        };
        let map = MapBuilder::new(UVec2::new(16, 16), UVec2::new(10, 10))
            .orientation(orientation)
            .tileset(Arc::new(tileset))
            .build()
            .unwrap();
        (map, Cell::new(TilesetSlot::new(0).unwrap(), TileId(0)))
    }

    fn sprite(tile: Cell, pos: DVec2) -> MapObject {
        MapObject {
            id: ObjectId(1),
            tile: Some(tile),
            pos,
            ..MapObject::default()
        }
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        (a - b).length() < 1e-9
    }

    #[test]
    fn the_foot_of_a_drawn_tree_is_the_middle_of_its_trunk_and_its_shadow_does_not_count() {
        let scratch = Scratch::new("bt-script-feet-tree").unwrap();
        for shadow in [60, 110, 150, 190] {
            let dir = scratch.join(format!("shadow-{shadow}"));
            std::fs::create_dir_all(&dir).unwrap();
            let (map, tile) = map_with(&dir, &tree(shadow), Orientation::Orthogonal);
            let mut feet = Feet::new();
            assert_eq!(
                feet.fraction(&map, tile).unwrap(),
                v(12.0 / 32.0, 40.0 / 48.0),
                "a shadow of alpha {shadow}"
            );
            let mirrored = tile.with_flip(Flip::H);
            assert_eq!(
                feet.fraction(&map, mirrored).unwrap(),
                v(20.0 / 32.0, 40.0 / 48.0)
            );
        }
        // A sprite drawn wholly semi-transparent still has a foot.
        let ghost = DecodedImage {
            width: 2,
            height: 2,
            rgba: vec![0, 0, 0, 120, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 120],
        };
        let dir = scratch.join("ghost");
        std::fs::create_dir_all(&dir).unwrap();
        let (map, tile) = map_with(&dir, &ghost, Orientation::Orthogonal);
        let mut feet = Feet::new();
        assert_eq!(feet.fraction(&map, tile).unwrap(), v(0.75, 1.0));
        assert_eq!(
            feet.fraction(&map, tile.with_flip(Flip::V)).unwrap(),
            v(0.25, 1.0),
            "upside down, the top-left pixel is lowest"
        );
    }

    #[test]
    fn images_that_cannot_be_measured_say_why() {
        let scratch = Scratch::new("bt-script-feet-errors").unwrap();
        let blank = DecodedImage {
            width: 4,
            height: 4,
            rgba: vec![0; 64],
        };
        let (map, tile) = map_with(scratch.path(), &blank, Orientation::Orthogonal);
        assert_eq!(
            Feet::new().fraction(&map, tile).unwrap_err().to_string(),
            "the image of tile 0 of “trees” has no visible pixel, so it has no foot"
        );
        std::fs::remove_file(scratch.join("tree.png")).unwrap();
        assert!(matches!(
            Feet::new().fraction(&map, tile),
            Err(Error::Image { .. })
        ));
        let point = MapObject {
            shape: Shape::Point,
            pos: v(24.0, 40.0),
            ..MapObject::default()
        };
        assert_eq!(
            Feet::new().position_for_foot(&map, &point, v(1.0, 1.0)),
            Err(Error::NotATileObject { id: 0 })
        );
    }

    #[test]
    fn sprites_are_placed_by_their_feet_and_stand_where_put() {
        let scratch = Scratch::new("bt-script-feet-place").unwrap();
        for orientation in [Orientation::Orthogonal, Orientation::Isometric] {
            let dir = scratch.join(format!("{orientation:?}"));
            std::fs::create_dir_all(&dir).unwrap();
            let (map, tile) = map_with(&dir, &tree(60), orientation);
            let mut feet = Feet::new();
            let foot = v(5.5, 6.0);
            for flip in [Flip::empty(), Flip::H] {
                for rotation in [0.0, 30.0] {
                    let mut tree = sprite(tile.with_flip(flip), DVec2::ZERO);
                    tree.rotation = rotation;
                    tree.pos = feet.position_for_foot(&map, &tree, foot).unwrap();
                    let stands = feet.standing_point(&map, &tree).unwrap();
                    assert!(
                        close(stands, foot),
                        "{orientation:?} {flip:?} {rotation}: {stands}"
                    );
                    assert_eq!(standing_cell(stands), IVec2::new(5, 5));
                }
            }
            if orientation == Orientation::Orthogonal {
                // Tiled anchors tile objects by their bottom-left: the trunk is 12 pixels in
                // and 8 up from it.
                let tree = sprite(tile, DVec2::ZERO);
                let pos = feet.position_for_foot(&map, &tree, foot).unwrap();
                assert_eq!(pos, v(5.5 * 16.0 - 12.0, 6.0 * 16.0 + 8.0));
            }
        }
    }

    #[test]
    fn a_point_on_a_cells_bottom_edge_stands_in_that_cell() {
        assert_eq!(standing_cell(v(10.5, 6.0)), IVec2::new(10, 5));
        assert_eq!(standing_cell(v(10.5, 5.99)), IVec2::new(10, 5));
        assert_eq!(
            standing_cell(v(10.5, 5.0)),
            IVec2::new(10, 4),
            "its top edge is the bottom of the cell above"
        );
        assert_eq!(standing_cell(v(10.0, 5.5)), IVec2::new(10, 5));
        assert_eq!(standing_cell(v(-0.5, 0.5)), IVec2::new(-1, 0));
        let square = MapGeometry::new(
            Orientation::Orthogonal,
            IVec2::new(16, 16),
            IVec2::new(4, 4),
        );
        assert_eq!(point_to_pixel(&square, v(10.5, 6.0)), v(168.0, 96.0));
        assert_eq!(pixel_to_point(&square, v(168.0, 96.0)), v(10.5, 6.0));
    }

    #[test]
    fn points_on_hexagonal_and_staggered_maps_are_places_in_their_cells() {
        let mut hexagonal =
            MapGeometry::new(Orientation::Hexagonal, IVec2::new(14, 12), IVec2::new(9, 9));
        hexagonal.hex_side_length = 6;
        let staggered =
            MapGeometry::new(Orientation::Staggered, IVec2::new(64, 32), IVec2::new(9, 9));
        let mut columns = staggered;
        columns.stagger_axis = StaggerAxis::X;
        for geometry in [hexagonal, staggered, columns] {
            for cell in [IVec2::new(3, 3), IVec2::new(4, 4), IVec2::new(2, 5)] {
                let centre = cell.as_dvec2() + 0.5;
                let bottom = cell.as_dvec2() + v(0.5, 1.0);
                for point in [centre, bottom] {
                    let back = pixel_to_point(&geometry, point_to_pixel(&geometry, point));
                    assert!(
                        close(back, point),
                        "{:?} {cell} {point}: {back}",
                        geometry.orientation
                    );
                    assert_eq!(standing_cell(back), cell);
                }
                // The middle of the cell's outline is its centre.
                let outline = geometry.tile_polygon(cell);
                let bounds = DRect::from_points(outline).unwrap();
                assert!(close(
                    point_to_pixel(&geometry, centre),
                    (bounds.min + bounds.max) * 0.5
                ));
            }
        }
    }
}
