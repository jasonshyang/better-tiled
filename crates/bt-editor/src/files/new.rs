//! New maps and tilesets: what New Map and New Tileset make, checked before anything is made.

use std::path::PathBuf;
use std::sync::Arc;

use bt_core::{
    Color, FileRef, ImageRef, ImageSource, Layer, LayerDataFormat, LayerId, LayerKind, Map,
    Orientation, RenderOrder, StaggerAxis, TileLayer, Tileset, TilesetImage, TilesetRef,
};
use bt_edit::add_tileset;
use glam::{IVec2, UVec2};

use crate::document::DocumentId;
use crate::editor::{Editor, warn};
use crate::effect::Effect;
use crate::error::EditError;
use crate::notice::Notice;

/// What New Map makes (Tiled's New Map dialog): one tile layer, "Tile Layer 1", unless the map
/// is too large for one.
#[derive(Clone, Debug, PartialEq)]
pub struct NewMap {
    /// The grid's shape.
    pub orientation: Orientation,
    /// How tile layer data is written.
    pub layer_format: LayerDataFormat,
    /// The order tiles are drawn in.
    pub render_order: RenderOrder,
    /// The size in tiles (what an infinite map's file declares).
    pub size: UVec2,
    /// Whether tile layers extend without bounds.
    pub infinite: bool,
    /// The size of a grid cell in pixels.
    pub tile_size: UVec2,
}

impl Default for NewMap {
    fn default() -> NewMap {
        NewMap {
            orientation: Orientation::Orthogonal,
            layer_format: LayerDataFormat::Csv,
            render_order: RenderOrder::RightDown,
            size: UVec2::new(30, 20),
            infinite: false,
            tile_size: UVec2::new(32, 32),
        }
    }
}

/// A map whose tile layers would take more memory than this gets none (as in Tiled).
const MAX_LAYER_BYTES: u64 = 1 << 30;

impl NewMap {
    /// Why the map cannot be made, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if self.tile_size.x == 0 || self.tile_size.y == 0 {
            return Some("Tiles must be at least one pixel wide and high".to_owned());
        }
        if !self.infinite && (self.size.x == 0 || self.size.y == 0) {
            return Some("The map must be at least one tile wide and high".to_owned());
        }
        if i32::try_from(self.size.x.max(self.size.y)).is_err()
            || i32::try_from(self.tile_size.x.max(self.tile_size.y)).is_err()
        {
            return Some("The map is larger than a map can be".to_owned());
        }
        None
    }

    /// The map, with Tiled's defaults for what the dialog does not ask: the stagger and the
    /// side length of hexagons, the shear of oblique maps.
    pub fn map(&self) -> Map {
        let mut map = Map {
            orientation: self.orientation,
            render_order: self.render_order,
            width: self.size.x,
            height: self.size.y,
            tile_width: self.tile_size.x,
            tile_height: self.tile_size.y,
            infinite: self.infinite,
            ..Map::default()
        };
        map.file.layer_data = self.layer_format;
        if self.orientation == Orientation::Oblique {
            map.skew = IVec2::new(i32::try_from(self.tile_size.x / 2).unwrap_or(0), 0);
        }
        if self.orientation == Orientation::Hexagonal {
            let side = match map.stagger_axis {
                StaggerAxis::X => self.tile_size.x / 2,
                StaggerAxis::Y => self.tile_size.y / 2,
            };
            map.hex_side_length = i32::try_from(side).unwrap_or(0);
        }
        let bytes = u64::from(self.size.x) * u64::from(self.size.y) * 8;
        if bytes < MAX_LAYER_BYTES {
            let cells = if self.infinite {
                TileLayer::infinite(self.size)
            } else {
                TileLayer::finite(self.size)
            };
            map.layers.push(Layer::new(
                LayerId(1),
                "Tile Layer 1",
                LayerKind::Tile(cells),
            ));
            map.next_layer_id = 2;
        }
        map
    }
}

/// What New Tileset makes: an atlas cut from an image, or a collection to add images to.
#[derive(Clone, Debug, PartialEq)]
pub struct NewTileset {
    /// The tileset's name.
    pub name: String,
    /// The size of a tile in pixels (a collection's is its largest image's, later).
    pub tile_size: UVec2,
    /// Where the tiles come from.
    pub source: NewTilesetSource,
}

/// Where a new tileset's tiles come from.
#[derive(Clone, Debug, PartialEq)]
pub enum NewTilesetSource {
    /// One image cut into a grid.
    Atlas {
        /// The image file.
        image: PathBuf,
        /// Its size in pixels, as the window read it.
        image_size: UVec2,
        /// A colour drawn as transparent.
        transparent: Option<Color>,
        /// Pixels around the tiles.
        margin: u32,
        /// Pixels between the tiles.
        spacing: u32,
    },
    /// One image per tile, added later.
    Collection,
}

impl NewTileset {
    /// Why the tileset cannot be made, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("The tileset needs a name".to_owned());
        }
        if self.tile_size.x == 0 || self.tile_size.y == 0 {
            return Some("Tiles must be at least one pixel wide and high".to_owned());
        }
        if let NewTilesetSource::Atlas { image_size, .. } = &self.source
            && self.tileset().tile_count() == 0
        {
            return Some(format!(
                "Not one {}×{} tile fits the {}×{} image",
                self.tile_size.x, self.tile_size.y, image_size.x, image_size.y
            ));
        }
        None
    }

    /// The tileset.
    pub fn tileset(&self) -> Tileset {
        let mut tileset = Tileset {
            name: self.name.trim().to_owned(),
            tile_width: self.tile_size.x,
            tile_height: self.tile_size.y,
            ..Tileset::default()
        };
        if let NewTilesetSource::Atlas {
            image,
            image_size,
            transparent,
            margin,
            spacing,
        } = &self.source
        {
            tileset.margin = *margin;
            tileset.spacing = *spacing;
            let columns = tileset.columns_for_width(image_size.x);
            let rows = tileset.rows_for_height(image_size.y);
            tileset.image = TilesetImage::Atlas {
                image: ImageRef {
                    // No written form yet: the writer writes it relative to the file.
                    source: ImageSource::File(FileRef::new("", image.clone())),
                    declared_size: *image_size,
                    transparent: *transparent,
                },
                columns,
                tile_count: columns * rows,
            };
        }
        tileset
    }
}

impl Editor {
    /// Opens a new, unsaved map made as New Map says.
    pub(crate) fn create_map(&mut self, new: &NewMap) -> Vec<Effect> {
        if let Some(problem) = new.problem() {
            return vec![Effect::Notify(Notice::error(problem))];
        }
        let doc = self.open_new_map(new.map());
        self.documents.set_current(doc);
        Vec::new()
    }

    /// Adds a new tileset to a map, embedded in it, as one step.
    pub(crate) fn create_tileset(&mut self, doc: DocumentId, new: &NewTileset) -> Vec<Effect> {
        if let Some(problem) = new.problem() {
            return vec![Effect::Notify(Notice::error(problem))];
        }
        let Some(asset) = self.documents.get(doc).map(|d| d.asset()) else {
            return vec![warn(EditError::NoSuchDocument(doc))];
        };
        let tileset = TilesetRef::embedded(Arc::new(new.tileset()));
        let result = self.transact(doc, "New Tileset", |tx| {
            let map = tx.map_mut(asset)?;
            add_tileset(map, tileset).map_err(EditError::from)
        });
        match result {
            Ok(_) => Vec::new(),
            Err(error) => vec![warn(error)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_map_has_one_tile_layer_and_tileds_defaults() {
        let map = NewMap::default().map();
        assert_eq!((map.width, map.height, map.tile_width), (30, 20, 32));
        assert_eq!(map.layers.len(), 1);
        assert_eq!(map.layers[0].name, "Tile Layer 1");
        assert_eq!(map.next_layer_id, 2);
        assert_eq!(map.file.layer_data, LayerDataFormat::Csv);
        assert!(map.validate().is_empty());
        let hex = NewMap {
            orientation: Orientation::Hexagonal,
            tile_size: UVec2::new(32, 28),
            ..NewMap::default()
        }
        .map();
        assert_eq!(
            hex.hex_side_length, 14,
            "half the tile height on a y-staggered map"
        );
        let infinite = NewMap {
            infinite: true,
            ..NewMap::default()
        }
        .map();
        assert!(infinite.layers[0].as_tile_layer().unwrap().is_infinite());
        assert!(infinite.validate().is_empty());
    }

    #[test]
    fn impossible_maps_and_tilesets_are_refused_with_a_reason() {
        let zero = NewMap {
            tile_size: UVec2::new(0, 32),
            ..NewMap::default()
        };
        assert!(zero.problem().unwrap().contains("pixel"));
        let tileset = NewTileset {
            name: "terrain".into(),
            tile_size: UVec2::new(64, 64),
            source: NewTilesetSource::Atlas {
                image: PathBuf::from("/images/terrain.png"),
                image_size: UVec2::new(32, 32),
                transparent: None,
                margin: 0,
                spacing: 0,
            },
        };
        assert_eq!(
            tileset.problem().as_deref(),
            Some("Not one 64×64 tile fits the 32×32 image")
        );
    }

    #[test]
    fn an_atlas_is_cut_into_the_tiles_its_image_holds() {
        let new = NewTileset {
            name: " terrain ".into(),
            tile_size: UVec2::new(16, 16),
            source: NewTilesetSource::Atlas {
                image: PathBuf::from("/images/terrain.png"),
                image_size: UVec2::new(70, 36),
                transparent: None,
                margin: 1,
                spacing: 2,
            },
        };
        let tileset = new.tileset();
        assert_eq!(tileset.name, "terrain");
        // (70 - 2 + 2) / 18 = 3 columns; (36 - 2 + 2) / 18 = 2 rows.
        assert_eq!((tileset.columns(), tileset.tile_count()), (3, 6));
    }

    #[test]
    fn a_new_tileset_is_one_step_of_the_map() {
        let mut editor = Editor::new();
        editor.apply(crate::intent::Intent::CreateMap(NewMap::default()));
        let doc = editor.current().unwrap();
        let new = NewTileset {
            name: "terrain".into(),
            tile_size: UVec2::new(32, 32),
            source: NewTilesetSource::Atlas {
                image: PathBuf::from("/images/terrain.png"),
                image_size: UVec2::new(256, 128),
                transparent: None,
                margin: 0,
                spacing: 0,
            },
        };
        editor.apply(crate::intent::Intent::CreateTileset { doc, tileset: new });
        assert_eq!(editor.map_of(doc).unwrap().tilesets.len(), 1);
        editor.apply(crate::intent::Intent::Run(crate::actions::ActionId::Undo));
        assert!(editor.map_of(doc).unwrap().tilesets.is_empty());
    }
}
