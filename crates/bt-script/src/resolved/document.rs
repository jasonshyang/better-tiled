//! What the resolved JSON holds, field by field: the types `schema.json` describes, each field
//! named and ordered as it is written. Optional fields are left out when the map has nothing for
//! them; a field every element of a kind has is always written.

use std::collections::BTreeMap;

use glam::{DVec2, IVec2, UVec2};
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

/// Custom properties as written: by name, with JSON values.
pub(crate) type JsonProperties = serde_json::Map<String, serde_json::Value>;

/// The whole file.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ResolvedMap {
    /// The format's name and version.
    pub(crate) format: &'static str,
    /// The map's grid, class and properties.
    pub(crate) map: MapEntry,
    /// The tilesets, without AutoMap's rule tiles.
    pub(crate) tilesets: Vec<TilesetEntry>,
    /// The tiles shown, by key.
    pub(crate) tiles: BTreeMap<usize, TileEntry>,
    /// The layers, bottom first, each group before its layers.
    pub(crate) layers: Vec<LayerEntry>,
}

/// The map: its grid and where its cells are.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct MapEntry {
    /// The map file's name without its extension.
    pub(crate) name: String,
    /// The map file, relative to the JSON file.
    pub(crate) source: String,
    /// `orthogonal`, `isometric`, `staggered` or `hexagonal`.
    pub(crate) orientation: &'static str,
    /// Staggered and hexagonal maps: `x` or `y`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stagger_axis: Option<&'static str>,
    /// Staggered and hexagonal maps: `odd` or `even`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stagger_index: Option<&'static str>,
    /// Hexagonal maps: the straight side's length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hex_side_length: Option<i32>,
    /// `right-down`, `right-up`, `left-down` or `left-up`.
    pub(crate) render_order: &'static str,
    /// Whether the map has no fixed size.
    pub(crate) infinite: bool,
    /// The columns every tile layer's rows hold.
    pub(crate) columns: i32,
    /// The rows every tile layer holds.
    pub(crate) rows: i32,
    /// The cell `rows[0][0]` is.
    pub(crate) origin_cell: IVec2,
    /// Tiled's tile size.
    pub(crate) cell_size: UVec2,
    /// What the cells cover on screen.
    pub(crate) screen_rect: Bounds<i32>,
    /// Where every cell's centre is on screen.
    pub(crate) cell_centres: CellCentres,
    /// Where parallax scrolls around.
    pub(crate) parallax_origin: DVec2,
    /// The colour behind the map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) background_color: Option<String>,
    /// The map's class.
    #[serde(rename = "type")]
    pub(crate) class: String,
    /// The map's properties, resolved.
    pub(crate) properties: JsonProperties,
}

/// Where the cells' centres are: `first + c × column_step + r × row_step`, plus the shift of
/// odd rows or columns on staggered and hexagonal maps.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct CellCentres {
    /// The centre of cell 0,0.
    pub(crate) first: DVec2,
    /// One column further.
    pub(crate) column_step: DVec2,
    /// One row further.
    pub(crate) row_step: DVec2,
    /// Stagger axis y: how far odd rows are from where the steps put them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) odd_rows_shift: Option<DVec2>,
    /// Stagger axis x: how far odd columns are from where the steps put them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) odd_columns_shift: Option<DVec2>,
}

/// A tileset.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TilesetEntry {
    /// The name as bt writes it in `TILESET:ID`, unique in the map.
    pub(crate) name: String,
    /// The name in Tiled.
    pub(crate) tiled_name: String,
    /// `atlas` or `images`.
    pub(crate) kind: &'static str,
    /// Atlas tilesets: the image, relative to the JSON file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) image: Option<String>,
    /// Atlas tilesets: the image's declared size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) image_size: Option<UVec2>,
    /// A tile's size, or the largest image's.
    pub(crate) tile_size: UVec2,
    /// How many tiles there are.
    pub(crate) tile_count: u32,
    /// Atlas tilesets: the columns of tiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) columns: Option<u32>,
    /// Atlas tilesets: the rows of tiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rows: Option<u32>,
    /// Atlas tilesets: pixels around the tiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) margin: Option<u32>,
    /// Atlas tilesets: pixels between tiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) spacing: Option<u32>,
    /// Pixels tiles are drawn moved by.
    pub(crate) tile_offset: IVec2,
    /// The tileset's class.
    #[serde(rename = "type")]
    pub(crate) class: String,
    /// The tileset's properties, resolved.
    pub(crate) properties: JsonProperties,
}

/// A tile with its flips: what a key in `tiles` stands for.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TileEntry {
    /// Its tileset's name in `tilesets`.
    pub(crate) tileset: String,
    /// Its id in the tileset.
    pub(crate) tile_id: u32,
    /// Where to draw it from.
    #[serde(flatten)]
    pub(crate) drawn_from: DrawnFrom,
    /// Mirrored left to right.
    pub(crate) flip_h: bool,
    /// Mirrored top to bottom.
    pub(crate) flip_v: bool,
    /// Mirrored along the diagonal (turned 60° on hexagonal maps).
    pub(crate) flip_d: bool,
    /// Hexagonal maps: turned 120°.
    pub(crate) rotated_hex_120: bool,
    /// The sprite it is, by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sprite: Option<String>,
    /// The terrain covering more than half of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) terrain: Option<String>,
    /// The terrain on each lane, set by set.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) terrains: Vec<TerrainLanes>,
    /// The tile's class.
    #[serde(rename = "type")]
    pub(crate) class: String,
    /// The tile's properties, resolved.
    pub(crate) properties: JsonProperties,
    /// Its animation's frames.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) animation: Vec<FrameEntry>,
    /// Its collision shapes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) collision: Vec<ObjectEntry>,
}

/// Where a tile is drawn from: an image and the part of it, and an atlas tile's place.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct DrawnFrom {
    /// The image, relative to the JSON file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) image: Option<String>,
    /// The part of the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_rect: Option<Bounds<i32>>,
    /// Atlas tilesets: the column and row in the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) atlas: Option<UVec2>,
}

/// The terrains on a tile's lanes in one terrain set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TerrainLanes {
    /// The set's name in Tiled.
    pub(crate) set: String,
    /// The lanes the set uses, by their words, each with its terrain's name or none.
    pub(crate) lanes: BTreeMap<&'static str, Option<String>>,
}

/// A frame of an animated tile.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct FrameEntry {
    /// The tile shown.
    pub(crate) tile_id: u32,
    /// Where to draw it from.
    #[serde(flatten)]
    pub(crate) drawn_from: DrawnFrom,
    /// How long it is shown.
    pub(crate) duration_ms: u32,
}

/// A layer, with its groups' settings applied.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct LayerEntry {
    /// Its id.
    pub(crate) id: u32,
    /// Its name.
    pub(crate) name: String,
    /// Its groups' names and its own, joined by `/`.
    pub(crate) path: String,
    /// `tiles`, `objects`, `image` or `group`.
    pub(crate) kind: &'static str,
    /// Shown, with every group holding it.
    pub(crate) visible: bool,
    /// Its opacity times its groups'.
    pub(crate) opacity: f64,
    /// Its offset plus its groups', in screen pixels.
    pub(crate) offset: DVec2,
    /// Its parallax factor times its groups'.
    pub(crate) parallax: DVec2,
    /// Its tint times its groups'.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tint: Option<String>,
    /// How it is drawn over what is below.
    pub(crate) blend_mode: &'static str,
    /// The layer's class.
    #[serde(rename = "type")]
    pub(crate) class: String,
    /// The layer's properties, resolved.
    pub(crate) properties: JsonProperties,
    /// What it holds.
    #[serde(flatten)]
    pub(crate) content: LayerContent,
}

/// What a layer holds, by its kind.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum LayerContent {
    /// Cells.
    Tiles {
        /// How many cells hold a tile.
        filled_cells: usize,
        /// Each row's keys.
        rows: Vec<Vec<usize>>,
    },
    /// Objects.
    Objects {
        /// `top-down` or `list`.
        draw_order: &'static str,
        /// The colour shapes are drawn in.
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        /// The objects.
        objects: Vec<ObjectEntry>,
    },
    /// An image.
    Image {
        /// The image, relative to the JSON file.
        #[serde(skip_serializing_if = "Option::is_none")]
        image: Option<String>,
        /// Its declared size.
        #[serde(skip_serializing_if = "Option::is_none")]
        image_size: Option<UVec2>,
        /// Repeated left and right.
        repeat_x: bool,
        /// Repeated up and down.
        repeat_y: bool,
    },
    /// Other layers, which follow it.
    Group {},
}

impl LayerContent {
    /// The layer's kind as `kind` writes it: `tiles`, `objects`, `image` or `group`.
    pub(crate) fn kind_word(&self) -> &'static str {
        match self {
            LayerContent::Tiles { .. } => "tiles",
            LayerContent::Objects { .. } => "objects",
            LayerContent::Image { .. } => "image",
            LayerContent::Group {} => "group",
        }
    }
}

/// An object of an object layer, or a collision shape of a tile (which has no screen fields).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ObjectEntry {
    /// Its id.
    pub(crate) id: u32,
    /// Its name.
    pub(crate) name: String,
    /// Its class, else its tile's.
    #[serde(rename = "type")]
    pub(crate) object_type: String,
    /// Its shape, or `tile`.
    pub(crate) kind: &'static str,
    /// The template it was made from, relative to the JSON file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) template: Option<String>,
    /// Whether it is shown.
    pub(crate) visible: bool,
    /// Its opacity.
    pub(crate) opacity: f64,
    /// Its position in map pixels, its layer's offset included.
    pub(crate) pos: DVec2,
    /// Its box's size, or the size a tile object is drawn at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<DVec2>,
    /// Degrees clockwise.
    pub(crate) rotation: f64,
    /// A polygon's or polyline's points, from `pos`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) points: Option<Vec<DVec2>>,
    /// Where an object of a layer is drawn; `None` for a collision shape.
    #[serde(flatten)]
    pub(crate) drawn: Option<Drawn>,
    /// A tile object's tile.
    #[serde(flatten)]
    pub(crate) shows: Option<ShownTile>,
    /// A text object's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<TextEntry>,
    /// Its properties, resolved.
    pub(crate) properties: JsonProperties,
}

/// Where an object is drawn on screen.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Drawn {
    /// Where its position is drawn.
    pub(crate) screen_pos: DVec2,
    /// What is drawn, turned and moved by its layer's offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) outline: Option<Vec<DVec2>>,
    /// Isometric maps: the outline in map pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pixel_outline: Option<Vec<DVec2>>,
    /// Tile and text objects: their box from `screen_pos`, before they turn.
    #[serde(rename = "box", skip_serializing_if = "Option::is_none")]
    pub(crate) upright_box: Option<Bounds<f64>>,
    /// Points: the cell the point stands in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cell: Option<IVec2>,
}

/// The tile a tile object shows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(crate) struct ShownTile {
    /// Its key in `tiles`, unflipped.
    pub(crate) tile: usize,
    /// Mirrored left to right.
    pub(crate) flip_h: bool,
    /// Mirrored top to bottom.
    pub(crate) flip_v: bool,
}

/// A text object's text and how it is set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TextEntry {
    /// The text.
    pub(crate) string: String,
    /// The font family.
    pub(crate) font_family: String,
    /// The font size in pixels.
    pub(crate) pixel_size: u32,
    /// The colour.
    pub(crate) color: String,
    /// Whether lines wrap.
    pub(crate) wrap: bool,
    /// Bold.
    pub(crate) bold: bool,
    /// Italic.
    pub(crate) italic: bool,
    /// Underlined.
    pub(crate) underline: bool,
    /// Struck out.
    pub(crate) strikeout: bool,
    /// Kerning used.
    pub(crate) kerning: bool,
    /// `left`, `center`, `right` or `justify`.
    pub(crate) halign: &'static str,
    /// `top`, `center` or `bottom`.
    pub(crate) valign: &'static str,
}

/// A rectangle, written `[left, top, width, height]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds<T> {
    /// The left edge.
    pub(crate) left: T,
    /// The top edge.
    pub(crate) top: T,
    /// The width.
    pub(crate) width: T,
    /// The height.
    pub(crate) height: T,
}

impl<T: Serialize> Serialize for Bounds<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&self.left)?;
        tuple.serialize_element(&self.top)?;
        tuple.serialize_element(&self.width)?;
        tuple.serialize_element(&self.height)?;
        tuple.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_are_written_as_four_numbers_and_empty_parts_are_left_out() {
        let bounds = Bounds {
            left: 1,
            top: 2,
            width: 3,
            height: 4,
        };
        assert_eq!(serde_json::to_string(&bounds).unwrap(), "[1,2,3,4]");
        let drawn_from = DrawnFrom {
            image: Some("a.png".into()),
            ..DrawnFrom::default()
        };
        let frame = FrameEntry {
            tile_id: 3,
            drawn_from,
            duration_ms: 100,
        };
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"tile_id":3,"image":"a.png","duration_ms":100}"#
        );
        let group = LayerContent::Group {};
        assert_eq!(serde_json::to_string(&group).unwrap(), "{}");
        let tiles = LayerContent::Tiles {
            filled_cells: 1,
            rows: vec![vec![0, 1]],
        };
        assert_eq!(
            serde_json::to_string(&tiles).unwrap(),
            r#"{"filled_cells":1,"rows":[[0,1]]}"#
        );
    }
}
