//! Tilesets: an atlas cut from one image, or a collection of images.

use std::collections::{BTreeMap, BTreeSet};

use bt_utils::IRect;
use glam::{DVec2, IVec2, UVec2};

use crate::color::Color;
use crate::file_format::FileFormat;
use crate::ids::TileId;
use crate::image_ref::ImageRef;
use crate::property::Properties;
use crate::tile::TileData;
use crate::wang::WangSet;

/// A tileset: shared between maps behind an `Arc`, replaced rather than changed in place.
#[derive(Clone, Debug, PartialEq)]
pub struct Tileset {
    /// The name.
    pub name: String,
    /// The class; empty for none.
    pub class: String,
    /// The width of a tile: the grid cell of an atlas, the largest image of a collection.
    pub tile_width: u32,
    /// The height of a tile.
    pub tile_height: u32,
    /// Pixels between an atlas's tiles.
    pub spacing: u32,
    /// Pixels around an atlas's tiles.
    pub margin: u32,
    /// Whether the tiles come from one image or one image each.
    pub image: TilesetImage,
    /// Where tiles are drawn relative to their cell, in pixels.
    pub tile_offset: IVec2,
    /// Where tile objects of this tileset are anchored.
    pub object_alignment: Alignment,
    /// The size tiles are drawn at on tile layers.
    pub tile_render_size: TileRenderSize,
    /// How a tile drawn at another size fills it.
    pub fill_mode: FillMode,
    /// The grid for overlays and collision editing, when it is not the tile size.
    pub grid: Option<TilesetGrid>,
    /// The colour behind the tiles in the tileset view.
    pub background_color: Option<Color>,
    /// The flipped and turned variants terrain filling may use.
    pub transformations: Transformations,
    /// Per-tile data: for an atlas, the tiles that have some; for a collection, every tile.
    pub tiles: BTreeMap<TileId, TileData>,
    /// A collection's display order when it is not by id (Tiled 1.7); empty for by id. Tiles it
    /// does not list follow, by id.
    pub tile_order: Vec<TileId>,
    /// Terrain sets.
    pub wang_sets: Vec<WangSet>,
    /// Custom properties.
    pub properties: Properties,
    /// The id the next tile added to a collection gets.
    pub next_tile_id: u32,
    /// How the tileset's own file was written, for a tileset in a file of its own.
    pub file: FileFormat,
}

impl Default for Tileset {
    fn default() -> Tileset {
        Tileset {
            name: String::new(),
            class: String::new(),
            tile_width: 0,
            tile_height: 0,
            spacing: 0,
            margin: 0,
            image: TilesetImage::Collection,
            tile_offset: IVec2::ZERO,
            object_alignment: Alignment::Unspecified,
            tile_render_size: TileRenderSize::Tile,
            fill_mode: FillMode::Stretch,
            grid: None,
            background_color: None,
            transformations: Transformations::empty(),
            tiles: BTreeMap::new(),
            tile_order: Vec::new(),
            wang_sets: Vec::new(),
            properties: Properties::new(),
            next_tile_id: 0,
            file: FileFormat::default(),
        }
    }
}

/// Where a tileset's tiles come from.
#[derive(Clone, Debug, PartialEq)]
pub enum TilesetImage {
    /// One image cut into a grid of tiles.
    Atlas {
        /// The image, with the size its file declares.
        image: ImageRef,
        /// The columns of tiles the image actually has (counted from the image that was read,
        /// as Tiled does; the declared size when it could not be read).
        columns: u32,
        /// The tiles the image actually has.
        tile_count: u32,
    },
    /// One image per tile ([`TileData::image`]).
    Collection,
}

/// A tile's image and the part of it the tile is.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TileImage<'tileset> {
    /// The image.
    pub image: &'tileset ImageRef,
    /// The tile's rectangle in the image, in pixels.
    pub rect: IRect,
}

/// Where tile objects are anchored.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// The map's default: bottom-left, or bottom-centre on isometric maps.
    #[default]
    Unspecified,
    /// The top-left corner.
    TopLeft,
    /// The middle of the top edge.
    Top,
    /// The top-right corner.
    TopRight,
    /// The middle of the left edge.
    Left,
    /// The centre.
    Center,
    /// The middle of the right edge.
    Right,
    /// The bottom-left corner.
    BottomLeft,
    /// The middle of the bottom edge.
    Bottom,
    /// The bottom-right corner.
    BottomRight,
}

impl Alignment {
    /// Where the anchor is in a box, as a fraction of its size from the top-left. `Unspecified`
    /// is bottom-left here; [`crate::MapGeometry::object_alignment`] resolves it per map.
    pub fn origin_fraction(self) -> DVec2 {
        match self {
            Alignment::TopLeft => DVec2::new(0.0, 0.0),
            Alignment::Top => DVec2::new(0.5, 0.0),
            Alignment::TopRight => DVec2::new(1.0, 0.0),
            Alignment::Left => DVec2::new(0.0, 0.5),
            Alignment::Center => DVec2::new(0.5, 0.5),
            Alignment::Right => DVec2::new(1.0, 0.5),
            Alignment::Unspecified | Alignment::BottomLeft => DVec2::new(0.0, 1.0),
            Alignment::Bottom => DVec2::new(0.5, 1.0),
            Alignment::BottomRight => DVec2::new(1.0, 1.0),
        }
    }
}

/// The size a tileset's tiles are drawn at on tile layers.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TileRenderSize {
    /// Their own size.
    #[default]
    Tile,
    /// The map's grid cell.
    Grid,
}

/// How a tile drawn at another size than its own fills it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FillMode {
    /// Stretched to fill it.
    #[default]
    Stretch,
    /// Scaled to fit, keeping its proportions.
    PreserveAspectFit,
}

/// The grid a tileset's tiles are edited on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TilesetGrid {
    /// Square or diamond cells.
    pub orientation: GridOrientation,
    /// The cell size in pixels.
    pub size: UVec2,
}

/// The shape of a tileset's grid cells.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridOrientation {
    /// Squares.
    #[default]
    Orthogonal,
    /// Diamonds.
    Isometric,
}

bitflags::bitflags! {
    /// The flipped and turned variants of tiles that terrain filling may use.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Transformations: u8 {
        /// Mirrored left-right.
        const HFLIP = 1 << 0;
        /// Mirrored top-bottom.
        const VFLIP = 1 << 1;
        /// Turned.
        const ROTATE = 1 << 2;
        /// A variant only where no plain tile fits.
        const PREFER_UNTRANSFORMED = 1 << 3;
    }
}

impl Tileset {
    /// Whether the tiles are one image each.
    pub fn is_collection(&self) -> bool {
        matches!(self.image, TilesetImage::Collection)
    }

    /// The tile size.
    pub fn tile_size(&self) -> UVec2 {
        UVec2::new(self.tile_width, self.tile_height)
    }

    /// How many tiles there are: an atlas's cells, a collection's images.
    pub fn tile_count(&self) -> u32 {
        match &self.image {
            TilesetImage::Atlas { tile_count, .. } => *tile_count,
            TilesetImage::Collection => u32::try_from(self.tiles.len()).unwrap_or(u32::MAX),
        }
    }

    /// An atlas's columns; 0 for a collection.
    pub fn columns(&self) -> u32 {
        match &self.image {
            TilesetImage::Atlas { columns, .. } => *columns,
            TilesetImage::Collection => 0,
        }
    }

    /// The columns of tiles an atlas image `width` pixels wide holds (Tiled's
    /// `Tileset::columnCountForWidth`); 0 when not one fits.
    pub fn columns_for_width(&self, width: u32) -> u32 {
        cells_along(width, self.tile_width, self.spacing, self.margin)
    }

    /// The rows of tiles an atlas image `height` pixels high holds.
    pub fn rows_for_height(&self, height: u32) -> u32 {
        cells_along(height, self.tile_height, self.spacing, self.margin)
    }

    /// Whether a tile with this id exists.
    pub fn has_tile(&self, id: TileId) -> bool {
        match &self.image {
            TilesetImage::Atlas { tile_count, .. } => id.0 < *tile_count,
            TilesetImage::Collection => self.tiles.contains_key(&id),
        }
    }

    /// The data of a tile, if it has any.
    pub fn tile(&self, id: TileId) -> Option<&TileData> {
        self.tiles.get(&id)
    }

    /// A tile's image and rectangle: its atlas cell, or its collection image (the whole image,
    /// at its declared size, unless the tile names a part of it).
    pub fn tile_image(&self, id: TileId) -> Option<TileImage<'_>> {
        match &self.image {
            TilesetImage::Atlas {
                image,
                columns,
                tile_count,
            } => {
                if id.0 >= *tile_count || *columns == 0 {
                    return None;
                }
                let column = i32::try_from(id.0 % columns).ok()?;
                let row = i32::try_from(id.0 / columns).ok()?;
                let size = self.tile_size().as_ivec2();
                let step = size + IVec2::splat(i32::try_from(self.spacing).ok()?);
                let margin = IVec2::splat(i32::try_from(self.margin).ok()?);
                let min = margin + IVec2::new(column, row) * step;
                Some(TileImage {
                    image,
                    rect: IRect::from_min_size(min, size),
                })
            }
            TilesetImage::Collection => {
                let tile = self.tiles.get(&id)?;
                let image = tile.image.as_ref()?;
                let rect = tile.rect.unwrap_or(IRect::from_min_size(
                    IVec2::ZERO,
                    image.declared_size.as_ivec2(),
                ));
                Some(TileImage { image, rect })
            }
        }
    }

    /// The size a tile is in pixels: an atlas's cell, a collection tile's image or part of it.
    pub fn tile_pixel_size(&self, id: TileId) -> UVec2 {
        match (&self.image, self.tile_image(id)) {
            (TilesetImage::Collection, Some(image)) => {
                image.rect.size().max(IVec2::ZERO).as_uvec2()
            }
            _ => self.tile_size(),
        }
    }

    /// The tile an animated tile shows `time_ms` into its animation; the tile itself when it is
    /// not animated.
    pub fn animated_tile(&self, id: TileId, time_ms: u64) -> TileId {
        let Some(frames) = self
            .tiles
            .get(&id)
            .map(|tile| &tile.animation)
            .filter(|f| !f.is_empty())
        else {
            return id;
        };
        let total: u64 = frames.iter().map(|f| u64::from(f.duration_ms)).sum();
        let mut left = time_ms.checked_rem(total).unwrap_or(0);
        for frame in frames {
            let duration = u64::from(frame.duration_ms);
            if left < duration {
                return frame.tile;
            }
            left -= duration;
        }
        frames.first().map_or(id, |f| f.tile)
    }

    /// Every tile's id in display order: by id, or for a collection in its [`Tileset::tile_order`].
    pub fn tile_ids(&self) -> Vec<TileId> {
        match &self.image {
            TilesetImage::Atlas { tile_count, .. } => (0..*tile_count).map(TileId).collect(),
            TilesetImage::Collection if self.tile_order.is_empty() => {
                self.tiles.keys().copied().collect()
            }
            TilesetImage::Collection => {
                let listed: BTreeSet<TileId> = self.tile_order.iter().copied().collect();
                let ordered = self
                    .tile_order
                    .iter()
                    .copied()
                    .filter(|id| self.tiles.contains_key(id));
                let rest = self.tiles.keys().copied().filter(|id| !listed.contains(id));
                ordered.chain(rest).collect()
            }
        }
    }

    /// Sets a collection's display order; an order by id is stored as none.
    pub fn set_tile_order(&mut self, order: Vec<TileId>) {
        let by_id =
            order.len() == self.tiles.len() && order.iter().copied().eq(self.tiles.keys().copied());
        self.tile_order = if by_id { Vec::new() } else { order };
    }

    /// Moves `tiles` to `location` in the display order (an index into the order before the
    /// move), one after the other so they keep the given order; ids do not change (libtiled's
    /// `Tileset::relocateTiles`). Returns whether the order changed.
    pub fn relocate_tiles(&mut self, tiles: &[TileId], location: usize) -> bool {
        let before = self.tile_ids();
        let mut order = before.clone();
        let mut location = location.min(order.len().saturating_sub(1));
        for id in tiles {
            let Some(from) = order.iter().position(|t| t == id) else {
                continue;
            };
            let tile = order.remove(from);
            order.insert(location.min(order.len()), tile);
            if from > location {
                location += 1;
            }
        }
        if order == before {
            return false;
        }
        self.set_tile_order(order);
        true
    }

    /// Whether `other` can stand in for this tileset by Tiled's rule for tilesets embedded in a
    /// map (libtiled's `Tileset::findSimilarTileset`): the same tile count, image, tile size,
    /// spacing, margin and tile offset, and for a collection the same image for every tile.
    pub fn is_similar_to(&self, other: &Tileset) -> bool {
        let same_images = match (&self.image, &other.image) {
            (TilesetImage::Atlas { image: x, .. }, TilesetImage::Atlas { image: y, .. }) => {
                x.is_same_image_as(y)
            }
            (TilesetImage::Collection, TilesetImage::Collection) => {
                self.tiles.len() == other.tiles.len()
                    && self
                        .tiles
                        .iter()
                        .zip(&other.tiles)
                        .all(|((id_a, x), (id_b, y))| {
                            id_a == id_b
                                && match (&x.image, &y.image) {
                                    (Some(x), Some(y)) => x.is_same_image_as(y),
                                    (None, None) => true,
                                    _ => false,
                                }
                        })
            }
            _ => false,
        };
        same_images
            && self.tile_count() == other.tile_count()
            && self.tile_size() == other.tile_size()
            && self.spacing == other.spacing
            && self.margin == other.margin
            && self.tile_offset == other.tile_offset
    }
}

/// How many cells of `cell` pixels with `spacing` between them and `margin` around them fit in
/// `length` pixels: Tiled's `(length - margin + spacing) / (cell + spacing)`, never negative.
fn cells_along(length: u32, cell: u32, spacing: u32, margin: u32) -> u32 {
    let step = i64::from(cell) + i64::from(spacing);
    if step <= 0 {
        return 0;
    }
    let fitting = (i64::from(length) - i64::from(margin) + i64::from(spacing)).div_euclid(step);
    u32::try_from(fitting.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_ref::FileRef;
    use crate::image_ref::ImageSource;
    use crate::tile::Frame;

    fn collection(ids: &[u32]) -> Tileset {
        let mut tileset = Tileset::default();
        for &id in ids {
            tileset.tiles.insert(TileId(id), TileData::default());
        }
        tileset
    }

    fn atlas(columns: u32, tile_count: u32) -> Tileset {
        let image = ImageRef::file(FileRef::new("tiles.png", "/tiles.png"), UVec2::new(64, 48));
        Tileset {
            tile_width: 16,
            tile_height: 16,
            spacing: 2,
            margin: 1,
            image: TilesetImage::Atlas {
                image,
                columns,
                tile_count,
            },
            ..Tileset::default()
        }
    }

    #[test]
    fn similar_tilesets_share_their_image_and_grid() {
        let tileset = atlas(3, 6);
        assert!(tileset.is_similar_to(&atlas(3, 6)));
        assert!(!tileset.is_similar_to(&atlas(3, 5)), "another tile count");
        let mut offset = atlas(3, 6);
        offset.tile_offset = IVec2::new(0, 4);
        assert!(!tileset.is_similar_to(&offset));
        let mut moved = atlas(3, 6);
        if let TilesetImage::Atlas { image, .. } = &mut moved.image {
            image.source = ImageSource::File(FileRef::new("other.png", "/other.png"));
        }
        assert!(!tileset.is_similar_to(&moved), "another image");
        assert!(!tileset.is_similar_to(&Tileset::default()));
        assert!(Tileset::default().is_similar_to(&Tileset::default()));
    }

    #[test]
    fn atlas_cells_follow_the_margin_and_spacing() {
        let tileset = atlas(3, 6);
        assert_eq!(tileset.columns_for_width(64), 3);
        assert_eq!(tileset.rows_for_height(48), 2);
        assert_eq!(tileset.columns_for_width(10), 0, "no tile fits");
        assert_eq!(
            tileset.tile_image(TileId(4)).map(|t| t.rect),
            Some(IRect {
                x: 19,
                y: 19,
                w: 16,
                h: 16
            })
        );
        assert_eq!(tileset.tile_image(TileId(6)), None);
        assert!(tileset.has_tile(TileId(5)) && !tileset.has_tile(TileId(6)));
        assert_eq!(tileset.tile_ids(), (0..6).map(TileId).collect::<Vec<_>>());
    }

    #[test]
    fn animations_cycle() {
        let mut tileset = atlas(3, 6);
        let animation = vec![
            Frame {
                tile: TileId(1),
                duration_ms: 100,
            },
            Frame {
                tile: TileId(2),
                duration_ms: 50,
            },
        ];
        tileset.tiles.insert(
            TileId(0),
            TileData {
                animation,
                ..TileData::default()
            },
        );
        let at = |ms| tileset.animated_tile(TileId(0), ms);
        assert_eq!(
            (at(0), at(99), at(100), at(149), at(150)),
            (TileId(1), TileId(1), TileId(2), TileId(2), TileId(1))
        );
        assert_eq!(tileset.animated_tile(TileId(3), 40), TileId(3));
    }

    #[test]
    fn rearranged_tiles_keep_their_ids() {
        let mut t = collection(&[0, 1, 2, 3, 4]);
        let ids = |t: &Tileset| t.tile_ids().into_iter().map(|id| id.0).collect::<Vec<_>>();
        assert!(
            t.relocate_tiles(&[TileId(3), TileId(4)], 1),
            "backwards: 3 and 4 before 1"
        );
        assert_eq!(ids(&t), [0, 3, 4, 1, 2]);
        assert!(
            t.relocate_tiles(&[TileId(0)], 2),
            "forwards: 0 where 4 is, after it"
        );
        assert_eq!(ids(&t), [3, 4, 0, 1, 2]);
        assert!(!t.relocate_tiles(&[TileId(0)], 2), "already there");
        t.tiles.insert(TileId(7), TileData::default());
        t.tiles.remove(&TileId(4));
        assert_eq!(
            ids(&t),
            [3, 0, 1, 2, 7],
            "added tiles follow, removed ones drop out"
        );
        t.set_tile_order([0, 1, 2, 3, 7].map(TileId).to_vec());
        assert!(t.tile_order.is_empty(), "the order by id is stored as none");
        assert_eq!(t.tile_count(), 5);
    }
}
